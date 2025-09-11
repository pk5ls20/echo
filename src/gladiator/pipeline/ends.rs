use crate::gladiator::pipeline::GladiatorPipelineEnd;
use crate::gladiator::{GladiatorPipelineError, GladiatorPipelineResult};
use html5ever::serialize;
use markup5ever_rcdom::{RcDom, SerializableHandle};

#[derive(Debug)]
pub struct GladiatorNoopEnd;

impl GladiatorPipelineEnd for GladiatorNoopEnd {
    type Output = ();

    #[inline]
    fn postprocess(&self, _: &RcDom) -> GladiatorPipelineResult<Self::Output> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct GladiatorCollectEnd;

impl GladiatorPipelineEnd for GladiatorCollectEnd {
    type Output = String;

    fn postprocess(&self, dom: &RcDom) -> GladiatorPipelineResult<Self::Output> {
        let mut out = Vec::new();
        let mut children = dom.document.children.borrow_mut();
        children.drain(..).try_for_each(|handle| {
            let serializable: SerializableHandle = handle.into();
            serialize(&mut out, &serializable, Default::default())
        })?;
        String::from_utf8(out).map_err(GladiatorPipelineError::FromUTF8)
    }
}
