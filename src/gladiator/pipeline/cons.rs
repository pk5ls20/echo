use crate::gladiator::ext_plugins::{
    EchoExtError, EchoExtHandler, EchoExtMeta, EchoExtResult, EchoResourceExt, fuzz_hw, render,
    validate_attr,
};
use crate::gladiator::pipeline::GladiatorPipelineCons;
use crate::gladiator::{ElementExtNode, GladiatorElement};
use crate::services::states::EchoState;
use echo_macros::EchoBusinessError;
use html5ever::{ParseOpts, local_name, ns, parse_fragment};
use markup5ever::tendril::TendrilSink;
use markup5ever::{Attribute, LocalName, QualName};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use smallvec::SmallVec;
use std::rc::Rc;
use std::sync::Weak as WeakArc;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum IncomingCheckConsError {
    #[error("You do not have permission to upload this echo.")]
    PermissionDenied,
    #[error("The echo extension ID does not match the permission list.")]
    ExtPermissionNotMatched,
    #[error("Recursive echo elements are not allowed. (current depth: {0} > 1)")]
    RecursionEchoElement(usize),
    #[error("Can not parse echo-ext-id to usize")]
    InvalidExtID,
    #[error(transparent)]
    ExtCheckError(#[from] EchoExtError),
}

/// Used to check that the incoming echo matches the requirements in [`IncomingCheckConsError`]
/// ## Interior mutability (Safety)
/// Won't change internal elements
#[derive(Debug)]
pub struct IncomingEchoCheckCons {
    error: Option<IncomingCheckConsError>,
}

pub type IncomingEchoCheckResult<T> = Result<T, IncomingCheckConsError>;

impl IncomingEchoCheckCons {
    pub fn new() -> Self {
        Self { error: None }
    }

    pub fn check_passed(&self) -> bool {
        self.error.is_none()
    }

    pub fn error_ref(&self) -> Option<&IncomingCheckConsError> {
        self.error.as_ref()
    }

    pub fn error_take(&mut self) -> Option<IncomingCheckConsError> {
        self.error.take()
    }

    fn inner_process(
        &self,
        elem: &GladiatorElement<'_>,
        depth: usize,
    ) -> IncomingEchoCheckResult<()> {
        if depth > 1 {
            return Err(IncomingCheckConsError::RecursionEchoElement(depth));
        }
        if !elem.has_permission() {
            return Err(IncomingCheckConsError::PermissionDenied);
        }
        match elem {
            GladiatorElement::Standard(_) => {}
            GladiatorElement::Extended(node) => {
                if !node.ext_has_permission {
                    return Err(IncomingCheckConsError::ExtPermissionNotMatched);
                }
                let ext_id = node
                    .ext_id
                    .as_ref()
                    .map_err(|_| IncomingCheckConsError::InvalidExtID)?;
                let (_, attrs) = node.inner.split();
                let attrs = attrs.borrow();
                validate_attr(*ext_id, &attrs)?;
            }
        }
        Ok(())
    }
}

impl GladiatorPipelineCons for IncomingEchoCheckCons {
    fn process(&mut self, elem: &GladiatorElement<'_>, depth: usize) {
        if self.error.is_none()
            && let Err(e) = self.inner_process(elem, depth)
        {
            self.error = Some(e);
        }
    }
}

/// A trivial extractor for retrieving **permitted** resource extensions within [`ElementExtNode`] (currently with ext_id=1)
/// ### TODO:
/// This implementation currently introduces complexity to the entire rendering pipeline:
/// - Should each processing stage explicitly return a Result? (Advantage: clearer processing logic;
///   Disadvantage: harder to decouple errors)
/// - The resource extension's ext_id is currently **hard-coded**
/// ## Interior mutability (Safety)
/// I'm just an extractor
pub struct IncomingEchoResExtractorCons {
    res_id: Option<SmallVec<[i64; 5]>>,
    failed_extract_count: usize,
    failed_parse_count: usize,
}

impl IncomingEchoResExtractorCons {
    pub fn new() -> Self {
        Self {
            res_id: None,
            failed_extract_count: 0,
            failed_parse_count: 0,
        }
    }

    #[inline]
    pub fn is_success(&self) -> bool {
        self.failed_extract_count == 0 && self.failed_parse_count == 0
    }

    pub fn res_ids_ref(&self) -> Option<&[i64]> {
        self.res_id.as_deref()
    }

    pub fn res_ids_take(&mut self) -> Option<SmallVec<[i64; 5]>> {
        self.res_id.take()
    }
}

impl GladiatorPipelineCons for IncomingEchoResExtractorCons {
    fn process(&mut self, elem: &GladiatorElement<'_>, _: usize) {
        if let GladiatorElement::Extended(node) = elem
            && node.ext_has_permission
            && node.inner.has_permission
            && let Ok(ext_id) = node.ext_id
            // TODO: AVOID hard-coding here
            && ext_id == EchoResourceExt::ID
        {
            let (_, attr) = node.inner.split();
            let attr = attr.borrow();
            match EchoResourceExt::get_meta_from_attr(&attr, "res-id") {
                Ok(res_id) => match res_id.parse::<i64>() {
                    Ok(id) => self.res_id.get_or_insert_with(SmallVec::new).push(id),
                    Err(_) => self.failed_extract_count += 1,
                },
                Err(_) => self.failed_parse_count += 1,
            }
        }
    }
}

/// Used to filter out DOM trees that don't meet requirements
/// ## Behavior and Processing:
/// - Base elements must satisfy permission requirements; those failing will have their inner layers
///   pruned, with attr assigned only to `echo-s`.
/// - Extended elements must satisfy both permission requirements and the extension list; those
///   failing will have their inner layers pruned, with attr assigned only to `echo-ext-fuzz-hw`
/// ## Interior mutability **(Unsafe)**
/// When the current tree does not meet the requirements, **subtrees of this tree will be removed**
#[derive(Debug)]
pub struct OutGoingEchoFilterCons;

impl OutGoingEchoFilterCons {
    #[allow(clippy::only_used_in_recursion)]
    fn maybe_text_len(&self, node: &Handle) -> usize {
        let len = match &node.data {
            NodeData::Text { contents } => contents.borrow().graphemes(true).count(),
            _ => node
                .children
                .borrow()
                .iter()
                .map(|c| self.maybe_text_len(c))
                .sum(),
        };
        len.next_multiple_of(3)
    }
}

impl GladiatorPipelineCons for OutGoingEchoFilterCons {
    fn process(&mut self, elem: &GladiatorElement<'_>, _: usize) {
        match elem {
            GladiatorElement::Standard(element_node) => {
                if !element_node.has_permission {
                    let (_, attrs) = element_node.split();
                    let mut attrs_mut = attrs.borrow_mut();
                    *attrs_mut = vec![Attribute {
                        name: QualName::new(None, ns!(), LocalName::from("echo-s")),
                        value: self.maybe_text_len(element_node.node).to_string().into(),
                    }];
                    element_node.forget_child()
                }
            }
            GladiatorElement::Extended(element_node) => {
                let inner_node = &element_node.inner;
                if !(inner_node.has_permission && element_node.ext_has_permission) {
                    let (_, attrs) = inner_node.split();
                    let mut attrs_mut = attrs.borrow_mut();
                    // TODO: need a better way to handle this
                    let (fuzz_h, fuzz_w) =
                        fuzz_hw(element_node.ext_id.as_ref().copied().unwrap_or_default());
                    *attrs_mut = vec![Attribute {
                        name: QualName::new(None, ns!(), LocalName::from("echo-ext-fuzz-hw")),
                        value: format!("{}x{}", fuzz_h, fuzz_w).into(),
                    }];
                    inner_node.forget_child()
                }
            }
        }
    }
}

pub struct OutGoingEchoSSRConsCtx {
    pub user_id: i64,
}

/// Perform SSR rendering on the `ext` portion of the output echo.
/// ## Interior mutability **(Unsafe)**
/// Will add the rendered content to the DOM tree
pub struct OutGoingEchoSSRCons {
    state: WeakArc<EchoState>,
    ctx: OutGoingEchoSSRConsCtx,
    error: Option<EchoExtError>,
}

impl OutGoingEchoSSRCons {
    pub fn new(state: WeakArc<EchoState>, user_id: i64) -> Self {
        Self {
            state,
            ctx: OutGoingEchoSSRConsCtx { user_id },
            error: None,
        }
    }

    #[cfg(test)]
    pub fn new_with_dummy_state(user_id: i64) -> Self {
        Self {
            state: WeakArc::new(),
            ctx: OutGoingEchoSSRConsCtx { user_id },
            error: None,
        }
    }

    #[inline]
    pub fn check_passed(&self) -> bool {
        self.error.is_none()
    }

    #[inline]
    pub fn error(&self) -> Option<&EchoExtError> {
        self.error.as_ref()
    }
}

impl OutGoingEchoSSRCons {
    fn render<'a>(
        state: WeakArc<EchoState>,
        ctx: &OutGoingEchoSSRConsCtx,
        node: &'a ElementExtNode<'a>,
    ) -> EchoExtResult<()> {
        let (_, attrs) = node.inner.split();
        let attrs = attrs.borrow();
        // first, we need check if echo-ext-fuzz-hw exists
        if EchoResourceExt::get_from_attr(&attrs, "hw", "echo-ext-fuzz-").is_ok() {
            tracing::debug!("Skip rendering for echo-ext-fuzz-hw");
            return Ok(());
        }
        let ext_id = node
            .ext_id
            .as_ref()
            .map_err(|_| EchoExtError::ExtIdTransUsize)?;
        let rendered_html = render(*ext_id, state, ctx, &attrs)?;
        tracing::debug!("Output => id: {}, html: {}", ext_id, &rendered_html);
        let frag_dom = parse_fragment(
            RcDom::default(),
            ParseOpts::default(),
            QualName::new(None, ns!(html), local_name!("div")),
            vec![],
            true,
        )
        .one(rendered_html);
        let target = node.inner.node;
        if let Some(html) = frag_dom.document.children.borrow_mut().pop() {
            for child in html.children.borrow_mut().drain(..) {
                child.parent.set(Some(Rc::downgrade(target)));
                target.children.borrow_mut().push(child);
            }
        }
        Ok(())
    }
}

impl GladiatorPipelineCons for OutGoingEchoSSRCons {
    fn process(&mut self, elem: &GladiatorElement<'_>, _: usize) {
        match elem {
            GladiatorElement::Extended(node)
                if self.error.is_none()
                    && let Err(e) = Self::render(self.state.clone(), &self.ctx, node) =>
            {
                self.error = Some(e);
            }
            _ => {}
        };
    }
}
