use crate::gladiator::ext_plugins::{ALL_EXT_METAS, EchoExtMetaPubInfo};
use crate::gladiator::prelude::*;
use crate::services::states::EchoState;
use ahash::HashMap;
use echo_macros::EchoBusinessError;
use frunk::hlist;
use maplit::hashset;
use smallvec::SmallVec;
use std::borrow::Borrow;
use std::sync::Weak as WeakArc;

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum EchoBakerError {
    #[error("Echo baker pre-check failed")]
    PreCheckFailed,
    #[error("Gladiator post inner error")]
    GladiatorPostInner,
    #[error(transparent)]
    GladiatorPipelineInner(#[from] GladiatorPipelineError),
}

pub type EchoBakerResult<T> = Result<T, EchoBakerError>;

pub struct AddOuterEchoRes {
    pub safe_echo: String,
    pub res_ids: Option<SmallVec<[i64; 5]>>,
}

pub struct EchoBaker<'a> {
    builder: ammonia::Builder<'a>,
}

impl<'a> EchoBaker<'a> {
    pub fn new() -> Self {
        #[rustfmt::skip]
        let tiptap_tags = hashset![
            "blockquote", "p", "pre", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "hr",
            "br", "strong", "em", "s", "u", "code", "a", "span"
        ];
        let mut builder = ammonia::Builder::default();
        builder
            .add_tags(tiptap_tags) // also can be replaced ori tags I think
            .url_schemes(hashset!["http", "https"])
            .add_tag_attributes("a", &["target"])
            .add_tag_attributes("code", ["class"])
            .add_generic_attributes(&["style"]) // FIXME: strict check it (and tiptap)!
            .filter_style_properties(hashset!["color"])
            .add_generic_attributes(&["echo-pm", "echo-ext-id"])
            .add_generic_attribute_prefixes(["echo-ext-meta-"]);
        Self { builder }
    }

    pub fn add_outer_echo<P, E>(
        &self,
        echo: &str,
        user_permissions: P,
        ext_ids: E,
    ) -> EchoBakerResult<AddOuterEchoRes>
    where
        P: IntoIterator,
        P::Item: Borrow<i64>,
        E: IntoIterator,
        E::Item: Borrow<u32>,
    {
        let permissions = user_permissions
            .into_iter()
            .map(|x| x.borrow().to_string())
            .collect();
        let ext_ids = ext_ids
            .into_iter()
            .map(|x| x.borrow().to_string())
            .collect();
        let ts = GladiatorTransformer::new(&permissions, &ext_ids);
        let safe_echo = self.builder.clean(echo).to_string();
        let mut checker = IncomingEchoCheckCons::new();
        let mut res_ids = IncomingEchoResExtractorCons::new();
        let mut chain = hlist![&mut checker, &mut res_ids, GladiatorNoopEnd];
        ts.transform(&safe_echo, &mut chain)?;
        if let Some(err) = checker.error_ref() {
            tracing::error!("Add outer echo SSR error: {:?}", err);
            return Err(EchoBakerError::GladiatorPostInner);
        }
        Ok(AddOuterEchoRes {
            safe_echo,
            res_ids: res_ids.res_ids_take(),
        })
    }

    pub fn post_inner_echo<P, E>(
        &self,
        state: WeakArc<EchoState>,
        echo: &str,
        user_id: i64,
        user_permissions: P,
        ext_ids: E,
    ) -> EchoBakerResult<String>
    where
        P: IntoIterator,
        P::Item: Borrow<i64>,
        E: IntoIterator,
        E::Item: Borrow<u32>,
    {
        let permissions = user_permissions
            .into_iter()
            .map(|x| x.borrow().to_string())
            .collect();
        let ext_ids = ext_ids
            .into_iter()
            .map(|x| x.borrow().to_string())
            .collect();
        let ts = GladiatorTransformer::new(&permissions, &ext_ids);
        let safe_echo = self.builder.clean(echo).to_string();
        let mut ssr_cons = OutGoingEchoSSRCons::new(state, user_id);
        let mut chain = hlist![OutGoingEchoFilterCons, &mut ssr_cons, GladiatorCollectEnd];
        let output = ts.transform(&safe_echo, &mut chain)?;
        // TODO: use ammonia to filter final result again?
        if let Some(err) = ssr_cons.error() {
            tracing::error!("Post inner echo SSR error: {:?}", err);
            return Err(EchoBakerError::GladiatorPostInner);
        }
        Ok(output)
    }

    #[inline]
    pub fn all_ext_ids() -> &'static [u32] {
        ALL_EXT_IDS
    }

    #[inline]
    pub fn all_ext_metas() -> &'static HashMap<u32, EchoExtMetaPubInfo> {
        &ALL_EXT_METAS
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn fuzz_test_add_outer_echo() {
        let helper = EchoBaker::new();
        let echo =
        // language=html
        r#"
            <button onclick="alert('xss4')">click</button>
            <img src=x " onerror=alert('xss') x="
            <img src="data:text/html;base64,xss" alt="w">
            <scr\u0069pt><scr\u0069pt>alert('')</scr\u0069pt>
            <math><mi xlink:href="javascript:alert('xss7')">test</mi></math>
            <h2 onmouseover="alert('xss via H2 mouseover')">
               Hi <span echo-pm="1">there</span>,
            </h2>
            <details ontoggle="alert('xss: details.ontoggle')"><summary>www</summary><div>prpr</div></details>
            <script>alert('xss via <script> tag');</script>
            <p>
                this is a <strong><em><s>ba</s></em></strong>
                <span echo-pm="1"><strong><em><s>sic </s></em></strong>example of <strong>Tiptap</strong></span>.
            </p>
            <svg width="114514" height="1919810" onload="alert('xss via SVG onload')"></svg>
            <ul>
                <a href="javascript:alert('xss via JS URI')">www</a>
                <img src="https://example.com/image.png" onerror="alert('xss')" alt="www">
                <li><p>That’s a bullet list item.</p></li>
                <li><p>Here’s another one.</p></li>
            </ul>
            <pre><code class="language-css">body { display: none; }</code></pre>
            <pre><code class="language-js">
              </code>
              <script>
                console.log('xss in code block!');
              </script>
            </pre>
            <blockquote>
                <p>Wow, that’s <span echo-pm="2">amazing</span>. – Mom</p>
            </blockquote>
            <p></p>
        "#;
        let result = helper.add_outer_echo(echo, &[1, 2], &[1, 2, 3]);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(!result.safe_echo.contains("xss"));
    }

    #[test]
    fn fuzz_test_post_inner_echo() {
        let helper = EchoBaker::new();
        let echo =
        // qwq <div echo-pm="1" echo-ext-id="1" echo-ext-meta-res-id="1"></div>
        // language=html
        r#"
            <p>
              Test <span echo-pm="1">echowww</span> with <strong>inner</strong> content.
            </p>
            <span echo-pm="3">echoqwq</span>
            <div
              echo-pm="2"
              echo-ext-id="2"
              echo-ext-meta-vid="av170001"
              echo-ext-meta-autoplay="false"
              echo-ext-meta-simple="false">
            </div>
            <div>
              <blockquote>
                <p>Wow, that’s <span echo-pm="2">amazing</span>. – Mom</p>
              </blockquote>
              <div
                echo-pm="2"
                echo-ext-id="3"
                echo-ext-meta-id="27984428"
                echo-ext-meta-autoplay="false">
              </div>
            </div>
        "#;
        let result = helper.post_inner_echo(WeakArc::new(), echo, 1, &[1, 2], &[2]);
        tracing::debug!("Post inner echo result: {:?}", result);
        assert_eq!(result.is_ok(), true);
        let result = result.unwrap();
        assert_eq!(result.contains("echowww"), true);
        assert_eq!(result.contains("echoqwq"), false);
        assert_eq!(result.contains("player.bilibili.com/player.html"), true);
        assert_eq!(result.contains("music.163.com"), false);
    }
}
