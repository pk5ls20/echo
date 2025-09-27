#![doc = include_str!("gladiator/README.md")]

mod ext_plugins;
mod pipeline;

use echo_macros::EchoBusinessError;
use markup5ever::{Attribute, QualName};
use markup5ever_rcdom::{Handle, NodeData};
use std::cell::RefCell;
use std::num::ParseIntError;

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum GladiatorPipelineError {
    #[error(transparent)]
    SerializeDom(#[from] std::io::Error),
    #[error(transparent)]
    FromUTF8(#[from] std::string::FromUtf8Error),
}

pub type GladiatorPipelineResult<T> = Result<T, GladiatorPipelineError>;

pub struct ElementStandardNode<'a> {
    node: &'a Handle,
    has_permission: bool,
}

impl<'a> ElementStandardNode<'a> {
    pub fn forget_child(&self) {
        self.node.children.borrow_mut().drain(..).for_each(|child| {
            child.parent.set(None);
        });
    }
}

pub struct ElementExtNode<'a> {
    inner: ElementStandardNode<'a>,
    // so fxxk u lifetime
    ext_id: Result<u32, ParseIntError>,
    ext_has_permission: bool,
}

impl<'a> ElementStandardNode<'a> {
    pub fn split(&self) -> (&QualName, &RefCell<Vec<Attribute>>) {
        match self.node.data {
            NodeData::Element {
                ref name,
                ref attrs,
                ..
            } => (name, attrs),
            _ => {
                // TODO: still unreachable?
                unreachable!("spilt_element assumes that NodeData is always an NodeData::Element!")
            }
        }
    }
}

pub enum GladiatorElement<'a> {
    Standard(ElementStandardNode<'a>),
    Extended(ElementExtNode<'a>),
}

impl<'a> GladiatorElement<'a> {
    pub fn has_permission(&self) -> bool {
        match self {
            GladiatorElement::Standard(node) => node.has_permission,
            GladiatorElement::Extended(node) => node.inner.has_permission,
        }
    }
}

#[allow(unused_imports)]
pub mod prelude {
    pub use super::GladiatorPipelineError;
    pub use super::ext_plugins::ALL_EXT_IDS;
    pub use super::pipeline::GladiatorTransformer;
    pub use super::pipeline::cons::{
        IncomingCheckConsError, IncomingEchoCheckCons, IncomingEchoResExtractorCons,
        OutGoingEchoFilterCons, OutGoingEchoSSRCons,
    };
    pub use super::pipeline::ends::{GladiatorCollectEnd, GladiatorNoopEnd};
    pub use ahash::HashSet;
    pub use frunk::hlist;
}

#[cfg(test)]
mod test {
    use super::prelude::IncomingCheckConsError::PermissionDenied;
    use super::prelude::*;
    use smallvec::smallvec;
    use unicode_segmentation::UnicodeSegmentation;

    macro_rules! assert_pm {
        ($output:expr, $pm:expr) => {
            assert!($output.contains(&format!(r#"echo-pm="{}""#, $pm)));
        };
    }

    macro_rules! assert_s {
        ($output:expr, $ori_str:expr) => {
            assert!($output.contains(&format!(r#"echo-s="{}""#, $ori_str.graphemes(true).count())));
        };
    }

    fn into_set<T: ToString + Clone>(items: &[T]) -> HashSet<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn simple_span() {
        let input = r#"<span echo-pm="2">i-want-hidden</span>"#;
        let permission_ids = into_set::<i32>(&[]);
        let ext_ids = into_set::<i32>(&[]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut chain = hlist![&mut checker, OutGoingEchoFilterCons, GladiatorCollectEnd];
        let output = ts.transform(input, &mut chain).unwrap();
        let passed = checker.check_passed();
        tracing::info!("Simple span => {}", &output);
        assert_eq!(passed, false);
        assert_eq!(output, r#"<span echo-s="15"></span>"#);
    }

    #[test]
    fn unaffected_span() {
        let input = "<span>keep</span>";
        let permission_ids = into_set(&[1, 2]);
        let ext_ids = into_set::<i32>(&[]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut chain = hlist![&mut checker, OutGoingEchoFilterCons, GladiatorCollectEnd];
        let output = ts.transform(input, &mut chain).unwrap();
        let passed = checker.check_passed();
        assert_eq!(passed, true);
        assert!(output.contains("keep"));
        assert!(!output.contains("echo-s"));
    }

    #[test]
    fn sibling_pm_spans() {
        let input =
        // language=html
        r#"
            <div>
                <span echo-pm="1">foo</span>
                <span echo-pm="2">barbaz</span>
            </div>
        "#;
        let permission_ids = into_set(&[1]);
        let ext_ids = into_set::<i32>(&[]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut chain = hlist![&mut checker, OutGoingEchoFilterCons, GladiatorCollectEnd];
        let output = ts.transform(input, &mut chain).unwrap();
        let passed = checker.check_passed();
        assert_eq!(passed, false);
        assert_pm!(&output, 1);
        assert_s!(&output, "barbaz");
        assert!(output.contains("foo"));
        assert!(!output.contains("barbaz"));
    }

    #[test]
    fn recursive_check() {
        let input =
            // language=html
            r#"
                <div class="qwq">
                    <span echo-pm="1"><span echo-pm="1"></span></span>
                </div>
            "#;
        let permission_ids = into_set::<i32>(&[1]);
        let ext_ids = into_set::<i32>(&[3]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut chain = hlist![&mut checker, GladiatorNoopEnd];
        ts.transform(input, &mut chain).unwrap();
        let error = checker.error_ref();
        tracing::debug!("=> Stage1 {:?}", error);
        assert_eq!(
            error,
            Some(&IncomingCheckConsError::RecursionEchoElement(2))
        );
        let input =
            // language=html
            r#"
                <div
                  echo-pm="1"
                  echo-ext-id="3"
                  echo-ext-meta-id="411907897"
                  echo-ext-meta-autoplay="true"
                >
                  <div
                    echo-pm="1"
                    echo-ext-id="3"
                    echo-ext-meta-id="411907897"
                    echo-ext-meta-autoplay="true"
                  ></div>
                </div>
            "#;
        let permission_ids = into_set::<i32>(&[]);
        let ext_ids = into_set::<i32>(&[]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut chain = hlist![&mut checker, GladiatorNoopEnd];
        ts.transform(input, &mut chain).unwrap();
        let error = checker.error_ref();
        tracing::debug!("=> Stage2 {:?}", error);
        assert_eq!(error, Some(&PermissionDenied));
    }

    #[test]
    fn fuzz_demo() {
        let input =
        // language=html
        r#"
            <h2>Hi <span echo-pm="1">there</span>,</h2>
            <p>
                this is a <strong><em><s>ba</s></em></strong>
                <span echo-pm="1"><strong><em><s>sic </s></em></strong>example of <strong>Tiptap</strong></span>.
            </p>
            <ul>
                <li><p>That’s a bullet list item.</p></li>
                <li><p>Here’s another one.</p></li>
            </ul>
            <pre><code class="language-css">body { display: none; }</code></pre>
            <blockquote>
                <p>Wow, that’s <span echo-pm="2">amazing</span>. – Mom</p>
            </blockquote>
            <p></p>
            <div
                echo-pm="1"
                echo-ext-id="2"
                echo-ext-meta-vid="av170001"
                echo-ext-meta-autoplay="false"
                echo-ext-meta-simple="false">
            </div>
            <div
                echo-pm="1"
                echo-ext-id="2"
                echo-ext-meta-vid="av170001"
                echo-ext-meta-autoplay="false"
                echo-ext-meta-simple="false">
            </div>
        "#;
        let ext_ids = into_set::<i32>(&[]);
        // case 1
        let permission_ids = into_set(&[2]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut renderer = OutGoingEchoSSRCons::new_with_dummy_state();
        let mut chain = hlist![
            &mut checker,
            OutGoingEchoFilterCons,
            &mut renderer,
            GladiatorCollectEnd
        ];
        let output = ts.transform(input, &mut chain).unwrap();
        let passed = checker.check_passed();
        assert_eq!(passed, false);
        assert!(!output.contains("example of"));
        assert!(output.contains("amazing"));
        // case 2
        let permission_ids = into_set(&[1, 2]);
        let owned_permission_set = into_set(&[2]);
        let ts = GladiatorTransformer::new(&permission_ids, &owned_permission_set);
        let mut checker = IncomingEchoCheckCons::new();
        let mut renderer = OutGoingEchoSSRCons::new_with_dummy_state();
        let mut chain = hlist![
            &mut checker,
            OutGoingEchoFilterCons,
            &mut renderer,
            GladiatorCollectEnd
        ];
        ts.transform(input, &mut chain).unwrap();
        let passed = checker.check_passed();
        assert_eq!(passed, true);
        // case 3
        let permission_ids = into_set(&[1, 2, 3, 4, 114514]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut renderer = OutGoingEchoSSRCons::new_with_dummy_state();
        let mut chain = hlist![
            &mut checker,
            OutGoingEchoFilterCons,
            &mut renderer,
            GladiatorCollectEnd
        ];
        ts.transform(input, &mut chain).unwrap();
        let passed = checker.check_passed();
        assert_eq!(passed, false);
    }

    #[test]
    fn ext_res_collector() {
        let input =
            // language=html
            r#"
                <div
                    echo-pm="1"
                    echo-ext-id="1"
                    echo-ext-meta-res-id="114514">
                </div>
                <div
                    echo-pm="2"
                    echo-ext-id="1"
                    echo-ext-meta-res-id="1919810">
                </div>
            "#;
        let permission_ids = into_set(&[1]);
        let ext_ids = into_set(&[1]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut res_collector = IncomingEchoResExtractorCons::new();
        let mut chain = hlist![
            &mut checker,
            &mut res_collector,
            OutGoingEchoFilterCons,
            GladiatorCollectEnd
        ];
        let output = ts.transform(input, &mut chain).unwrap();
        tracing::info!("output => {}", &output);
        let checker_err = checker.error_ref();
        tracing::info!("checker.error() => {:#?}", &checker_err);
        assert_eq!(checker_err.is_none(), false);
        let res_ids = res_collector.res_ids_take();
        assert_eq!(res_ids, Some(smallvec![114_514i64]));
    }

    #[test]
    fn ext_basic_ssr() {
        let input =
            // language=html
            r#"
                <div
                    echo-pm="1"
                    echo-ext-id="2"
                    echo-ext-meta-vid="av170001"
                    echo-ext-meta-autoplay="false"
                    echo-ext-meta-simple="false">
                </div>
            "#;
        let permission_ids = into_set(&[1]);
        let ext_ids = into_set(&[2]);
        let ts = GladiatorTransformer::new(&permission_ids, &ext_ids);
        let mut checker = IncomingEchoCheckCons::new();
        let mut res_collector = IncomingEchoResExtractorCons::new();
        let mut renderer = OutGoingEchoSSRCons::new_with_dummy_state();
        let mut chain = hlist![
            &mut checker,
            &mut res_collector,
            OutGoingEchoFilterCons,
            &mut renderer,
            GladiatorCollectEnd
        ];
        let output = ts.transform(input, &mut chain).unwrap();
        tracing::info!("output => {}", &output);
        let (checker_err, renderer_err) = (checker.error_ref(), renderer.error());
        tracing::info!("checker.error() => {:#?}", &checker_err);
        tracing::info!("renderer.error() => {:#?}", &renderer_err);
        assert_eq!(checker_err.is_none() && renderer_err.is_none(), true);
        assert!(output.contains("//player.bilibili.com/player.html"));
        let res_ids = res_collector.res_ids_ref();
        assert_eq!(res_ids, None);
    }
}
