use crate::gladiator::{
    ElementExtNode, ElementStandardNode, GladiatorElement, GladiatorPipelineResult,
};
use ahash::HashSet;
use frunk::{HCons, HNil};
use html5ever::driver::parse_fragment_for_element;
use html5ever::{LocalName, ParseOpts, local_name, ns};
use markup5ever::QualName;
use markup5ever::interface::create_element;
use markup5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

pub mod cons;
pub mod ends;

pub trait GladiatorPipelineCons: Sized {
    /// In fact, the [`Handle`] inside [`GladiatorElement`] uses [`std::cell::RefCell`], so it is partially mutable.
    /// Be sure to **pay attention to the stacking order** when using it
    fn process(&mut self, elem: &GladiatorElement, depth: usize);
}

pub trait GladiatorPipelineEnd {
    type Output;
    /// Similarly, note the runtime borrowability of internal elements of [`RcDom`]
    fn postprocess(&self, _: &RcDom) -> GladiatorPipelineResult<Self::Output>;
}

impl<T> GladiatorPipelineCons for &mut T
where
    T: GladiatorPipelineCons,
{
    #[inline]
    fn process(&mut self, elem: &GladiatorElement<'_>, depth: usize) {
        (**self).process(elem, depth);
    }
}

impl<E> GladiatorPipelineEnd for &mut E
where
    E: GladiatorPipelineEnd,
{
    type Output = E::Output;

    #[inline]
    fn postprocess(&self, dom: &RcDom) -> GladiatorPipelineResult<Self::Output> {
        (**self).postprocess(dom)
    }
}

pub trait PipelineChain {
    type Output;
    fn process_one(&mut self, elem: &GladiatorElement, depth: usize);
    fn postprocess_end(&self, dom: &RcDom) -> GladiatorPipelineResult<Self::Output>;
}

impl<E> PipelineChain for HCons<E, HNil>
where
    E: GladiatorPipelineEnd,
{
    type Output = E::Output;

    #[inline]
    fn process_one(&mut self, _: &GladiatorElement<'_>, _: usize) {
        // no-op
    }

    #[inline]
    fn postprocess_end(&self, dom: &RcDom) -> GladiatorPipelineResult<Self::Output> {
        self.head.postprocess(dom)
    }
}

impl<H, T> PipelineChain for HCons<H, T>
where
    H: GladiatorPipelineCons,
    T: PipelineChain,
{
    type Output = T::Output;

    #[inline]
    fn process_one(&mut self, elem: &GladiatorElement<'_>, depth: usize) {
        self.head.process(elem, depth);
        self.tail.process_one(elem, depth);
    }

    #[inline]
    fn postprocess_end(&self, dom: &RcDom) -> GladiatorPipelineResult<Self::Output> {
        self.tail.postprocess_end(dom)
    }
}

/// Driver for `GladiatorPipeline`, which runs a processing pipeline composed of one or more processing
/// nodes that implement [`GladiatorPipelineCons`] and a terminating node that implements [`GladiatorPipelineEnd`]. <br/>
/// For detailed usage, see the `mod test` section in `../gladiator.rs`.
/// ## **Note:**
/// Because the elements passed internally by `impl GladiatorPipelineCons` and `impl GladiatorPipelineEnd`
/// all (directly or indirectly) contain [`std::cell::RefCell`], **please compose the pipeline carefully and write
/// thorough tests for the higher-level methods to <u>avoid undefined behavior!</u>**
pub struct GladiatorTransformer<'a> {
    permissions: &'a HashSet<String>,
    ext_ids: &'a HashSet<String>,
}

impl<'a> GladiatorTransformer<'a> {
    pub fn new(permissions: &'a HashSet<String>, ext_ids: &'a HashSet<String>) -> Self {
        Self {
            permissions,
            ext_ids,
        }
    }

    pub fn transform<L>(&self, input: &str, pipelines: &mut L) -> GladiatorPipelineResult<L::Output>
    where
        L: PipelineChain,
    {
        let sink = RcDom::default();
        let ctx_elem = create_element(
            &sink,
            QualName::new(None, ns!(html), LocalName::from("body")),
            Vec::new(),
        );
        let dom = parse_fragment_for_element(sink, ParseOpts::default(), ctx_elem, false, None)
            .from_utf8()
            .one(input.as_bytes());
        self.process_node(&dom.document, pipelines, 1);
        pipelines.postprocess_end(&dom)
    }

    fn process_node<L>(&self, node: &Handle, pipelines: &mut L, depth: usize)
    where
        L: PipelineChain,
    {
        // Meeting the definition means satisfying a valid [`GladiatorElement`]
        let mut is_valid_gladiator_element = false;
        if let NodeData::Element { name, attrs, .. } = &node.data
            && name.ns == ns!(html)
            && let Some(has_permission) = {
                attrs
                    .borrow()
                    .iter()
                    .find(|a| *a.name.local == *"echo-pm")
                    .map(|a| self.permissions.get(a.value.as_ref()).is_some())
            }
        {
            is_valid_gladiator_element = true;
            match name.local {
                local_name!("span") => pipelines.process_one(
                    &GladiatorElement::Standard(ElementStandardNode {
                        node,
                        has_permission,
                    }),
                    depth,
                ),
                local_name!("div")
                    if let Some(ext_id) = {
                        let attrs_ref = attrs.borrow();
                        attrs_ref
                            .iter()
                            .find(|a| a.name.local == *"echo-ext-id")
                            .map(|a| a.value.to_owned())
                    } =>
                {
                    let ext_has_permission = self.ext_ids.get(ext_id.as_ref()).is_some();
                    let ext_id = ext_id.as_ref().parse::<u32>();
                    pipelines.process_one(
                        &GladiatorElement::Extended(ElementExtNode {
                            inner: ElementStandardNode {
                                node,
                                has_permission,
                            },
                            ext_id,
                            ext_has_permission,
                        }),
                        depth,
                    )
                }
                _ => {
                    // TODO: should we just skip it (like below)?
                    is_valid_gladiator_element = false;
                    tracing::warn!(
                        "Unsupported gladiator element: {:?}, skipping...",
                        &node.data
                    );
                }
            }
        };
        // TODO:
        // Optimizing Recursion: Currently, we don't actually need recursion in all cases,
        // but due to constraints in trait design, it appears we have no choice but to continue
        // recursing indefinitely...
        for child in node.children.borrow().iter() {
            self.process_node(
                child,
                pipelines,
                if is_valid_gladiator_element {
                    depth + 1
                } else {
                    depth
                },
            )
        }
    }
}
