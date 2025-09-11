use crate::models::resource::{ResourceUploadHeader, UploadCreateReqMetaInfo};
use crate::services::states::EchoState;
use crate::services::states::cache::{MokaExpiration, MokaVal};
use crate::utils::hex_ext::HexString;
use crate::utils::stream_pipeline::stream_pipeline;
use bitvec::prelude::*;
use bytes::{Buf, Bytes, BytesMut};
use futures::{Stream, StreamExt};
use prost::Message;
use sha1::{Digest, Sha1};
use std::cmp::min;
use std::fmt::Debug;
use std::io::Seek;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::{env, fs};
use tempfile::NamedTempFile;
use time::Duration as TimeDuration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::codec::Decoder;
use uuid::Uuid;

#[derive(Debug)]
pub struct ResourceUploadLimits {
    /// The size passed to [`ResourceUploadProtocol`] must satisfy `8192 < {flush_stream_size} < chunk_size`. <br/>
    /// A reasonable flush_stream_size helps strike the right balance between memory consumption and disk I/O.
    pub flush_stream_size: NonZeroU32,
    /// Compute the header’s worst-case size under realistic assumptions by building a maximal-size proto message on the fly
    pub max_head_size: u32,
    /// Equivalent to the current maximum chunk size
    pub max_body_size: u32,
}

/// b"qwq" + head_len (u32, be) + body_len (u32, be) + head + body
pub enum ResourceUploadFrame {
    Header(Bytes),
    BodyChunk(Bytes),
    End,
}

impl Debug for ResourceUploadFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceUploadFrame::Header(b) => {
                write!(f, "ResourceUploadFrame::Header({:02x?})", b.as_ref())
            }
            ResourceUploadFrame::BodyChunk(b) => {
                write!(f, "ResourceUploadFrame::BodyChunk({:02x?})", b.as_ref())
            }
            ResourceUploadFrame::End => write!(f, "ResourceUploadFrame::End"),
        }
    }
}

#[derive(Debug)]
pub enum ResourceUploadStage {
    NeedMagicAndPrefix,
    NeedHeader { head_len: u32, body_len: u32 },
    StreamBody { remaining: u32 },
    EmitEnd,
    Done,
}

#[derive(Debug)]
pub struct ResourceUploadProtocol {
    limits: ResourceUploadLimits,
    stage: ResourceUploadStage,
}

impl ResourceUploadProtocol {
    pub fn new(limits: ResourceUploadLimits) -> Self {
        // TODO: better handling?
        if limits.flush_stream_size.get() < 8192 {
            panic!("flush_stream_size must be at least 8192 bytes");
        }
        Self {
            limits,
            stage: ResourceUploadStage::NeedMagicAndPrefix,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceUploadProtocolError {
    #[error(transparent)]
    StdIoError(#[from] std::io::Error),
    #[error("invalid magic, expected b\"qwq\"")]
    InvalidMagic,
    #[error("header too large: {got} > {max}")]
    HeadTooLarge { got: u32, max: u32 },
    #[error("body too large: {got} > {max}")]
    BodyTooLarge { got: u32, max: u32 },
}

impl Decoder for ResourceUploadProtocol {
    type Item = ResourceUploadFrame;
    type Error = ResourceUploadProtocolError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match self.stage {
                ResourceUploadStage::NeedMagicAndPrefix => {
                    if src.len() < 3 + 4 + 4 {
                        tracing::trace!("Not enough data for frame (stage 1)!");
                        return Ok(None);
                    }
                    let qwq = src.split_to(3).freeze();
                    if qwq.as_ref() != b"qwq" {
                        return Err(ResourceUploadProtocolError::InvalidMagic);
                    }
                    let head_len = src.get_u32();
                    let body_len = src.get_u32();
                    if head_len > self.limits.max_head_size {
                        return Err(ResourceUploadProtocolError::HeadTooLarge {
                            got: head_len,
                            max: self.limits.max_head_size,
                        });
                    }
                    if body_len > self.limits.max_body_size {
                        return Err(ResourceUploadProtocolError::BodyTooLarge {
                            got: body_len,
                            max: self.limits.max_body_size,
                        });
                    }
                    self.stage = ResourceUploadStage::NeedHeader { head_len, body_len };
                }
                ResourceUploadStage::NeedHeader { head_len, body_len } => {
                    if (src.len() as u32) < head_len {
                        tracing::trace!("Not enough data for frame (stage 2)!");
                        return Ok(None);
                    }
                    let header = src.split_to(head_len as usize).freeze();
                    self.stage = ResourceUploadStage::StreamBody {
                        remaining: body_len,
                    };
                    // src.reserve(body_len as usize);
                    return Ok(Some(ResourceUploadFrame::Header(header)));
                }
                ResourceUploadStage::StreamBody { ref mut remaining } => {
                    if *remaining == 0 {
                        self.stage = ResourceUploadStage::EmitEnd;
                        continue;
                    }
                    let available = min(src.len() as u32, *remaining);
                    if available == 0 {
                        return Ok(None);
                    }
                    let to_take = min(available, self.limits.flush_stream_size.get());
                    let to_take_chunk = src.split_to(to_take as usize).freeze();
                    *remaining -= to_take;
                    return Ok(Some(ResourceUploadFrame::BodyChunk(to_take_chunk)));
                }
                ResourceUploadStage::EmitEnd => {
                    self.stage = ResourceUploadStage::Done;
                    return Ok(Some(ResourceUploadFrame::End));
                }
                ResourceUploadStage::Done => return Ok(None),
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UploadTrackerError {
    #[error(transparent)]
    StdIO(std::io::Error),
    #[error(transparent)]
    TokioIO(#[from] tokio::io::Error),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error("Too many chunks!")]
    TooManyChunks,
    #[error(transparent)]
    FromHex(#[from] hex::FromHexError),
    #[error(transparent)]
    Consistency(#[from] ConsistencyViolationError),
    #[error(transparent)]
    TryFromInt(#[from] std::num::TryFromIntError),
    #[error(transparent)]
    DecodeProst(#[from] prost::DecodeError),
    #[error(transparent)]
    SemaphoreAcquireError(#[from] tokio::sync::AcquireError),
    #[error(transparent)]
    ResourceUploadProtocol(#[from] ResourceUploadProtocolError),
    #[error("unreachable stream path {0} in upload stream!")]
    UnreachableStreamPath(&'static str),
    #[error("unexpected end of resource stream!")]
    UnexceptedResourceStreamEOF,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConsistencyViolationError {
    // mismatches
    #[error("upload_session_id mismatch! expected={expected} got={got}")]
    SessionIdMismatch { expected: Uuid, got: Uuid },
    // chunks
    #[error("chunk_bytes_offset out of bounds: offset={offset} size={size}")]
    ChunkOffsetOutOfBounds { offset: u64, size: u64 },
    #[error("mismatch chunk length! expected={chunk_size} got={chunk_length} at offset={offset}")]
    InvalidChunkLength {
        offset: u64,
        chunk_size: NonZeroU32,
        chunk_length: u32,
    },
    #[error("chunk not aligned! offset={offset} chunk_size={chunk_size}")]
    ChunkNotAligned { offset: u64, chunk_size: NonZeroU32 },
    #[error("chunk sha1 mismatch! offset={offset} expected={expected:?} got={got:?}")]
    ChunkSha1Mismatch {
        offset: u64,
        expected: String,
        got: String,
    },
    #[error("failed to infer file mime type!")]
    FailedInferMimeType,
    #[error("file mime type mismatch! expected={expected} got={got}")]
    InvalidMimeType { expected: String, got: &'static str },
    // Final
    #[error("upload not complete at BytesRetained! received={received_bytes}")]
    BytesRetained { received_bytes: u64 },
    #[error("upload not complete at seen! undone chunks: {undone:?}")]
    SeenBytesRetained { undone: Vec<usize> },
    #[error("file sha1 mismatch! expected={expected} got={got}")]
    FileSha1Mismatch { expected: String, got: String },
}

pub type UploadTrackerResult<T> = Result<T, UploadTrackerError>;

/// [`UploadTracker`] guarantees **overall thread safety**
pub struct UploadTracker {
    // metadata
    session_id: Uuid,
    file_name: String,
    file_mime_type: String,
    file_ext: parking_lot::Mutex<Option<String>>,
    file_size: u64,
    file_sha1: [u8; 20],
    chunk_size: NonZeroU32,
    tmp_file: NamedTempFile,
    final_storage_path: PathBuf,
    // states
    seen: BitVec<AtomicUsize>,
    chunk_guard: parking_lot::Mutex<Vec<Arc<Semaphore>>>,
    exclusive_file_lock: parking_lot::Mutex<()>,
    received_bytes: AtomicU64,
}

struct UploadTrackerCtx {
    permit: OwnedSemaphorePermit,
    header: ResourceUploadHeader,
    sha1: Sha1,
    chunk_idx: usize,
    chunk_inner_offset: u64,
}

enum UploadTrackerACState {
    WaitingHeader,
    ReceivingBody(Option<UploadTrackerCtx>),
    Done(Option<UploadTrackerCtx>),
}

// TODO: Further platform-specific optimisations for Unix/Windows?
impl UploadTracker {
    pub async fn new(
        file_meta: UploadCreateReqMetaInfo,
        session_id: Uuid,
        chunk_size: NonZeroU32,
        tmp_storage_path: &Option<PathBuf>,
        final_storage_path: PathBuf,
    ) -> UploadTrackerResult<Self> {
        let file_size = file_meta.file_size;
        let num_chunks = file_size.div_ceil(chunk_size.get() as u64);
        let n_usize = usize::try_from(num_chunks).map_err(|_| UploadTrackerError::TooManyChunks)?;
        let tmp_file =
            NamedTempFile::new_in(tmp_storage_path.as_ref().unwrap_or(&env::temp_dir()))?;
        tmp_file
            .as_file()
            .set_len(file_size)
            .map_err(UploadTrackerError::StdIO)?;
        let file_sha1: [u8; 20] = hex::decode(file_meta.file_sha1)?
            .try_into()
            .map_err(|_| UploadTrackerError::FromHex(hex::FromHexError::InvalidStringLength))?;
        Ok(Self {
            session_id,
            file_name: file_meta.file_name,
            file_mime_type: file_meta.file_mime_type,
            file_ext: parking_lot::Mutex::new(None),
            file_size,
            file_sha1,
            chunk_size,
            seen: BitVec::repeat(false, n_usize),
            chunk_guard: parking_lot::Mutex::new(
                (0..n_usize).map(|_| Arc::new(Semaphore::new(1))).collect(),
            ),
            exclusive_file_lock: parking_lot::Mutex::new(()),
            received_bytes: AtomicU64::new(0),
            tmp_file,
            final_storage_path,
        })
    }

    fn chunk_idx(&self, chunk_bytes_offset: u64) -> UploadTrackerResult<usize> {
        if chunk_bytes_offset >= self.file_size {
            return Err(ConsistencyViolationError::ChunkOffsetOutOfBounds {
                offset: chunk_bytes_offset,
                size: self.file_size,
            }
            .into());
        }
        let cs = self.chunk_size.get() as u64;
        if !chunk_bytes_offset.is_multiple_of(cs) {
            return Err(ConsistencyViolationError::ChunkNotAligned {
                offset: chunk_bytes_offset,
                chunk_size: self.chunk_size,
            }
            .into());
        }
        let idx_u64 = chunk_bytes_offset / cs;
        let idx = usize::try_from(idx_u64).map_err(|_| UploadTrackerError::TooManyChunks)?;
        Ok(idx)
    }

    #[inline]
    fn expected_chunk_len(&self, offset: u64) -> u64 {
        min(self.chunk_size.get() as u64, self.file_size - offset)
    }

    pub async fn accept_chunk_stream<S>(&self, stream: &mut S) -> UploadTrackerResult<()>
    where
        S: Stream<Item = Result<ResourceUploadFrame, ResourceUploadProtocolError>> + Unpin,
    {
        let mut state = UploadTrackerACState::WaitingHeader;
        loop {
            match stream.next().await {
                Some(Ok(frame)) => match (&mut state, frame) {
                    (UploadTrackerACState::WaitingHeader, ResourceUploadFrame::Header(content)) => {
                        let h = ResourceUploadHeader::decode(content)
                            .map_err(UploadTrackerError::DecodeProst)?;
                        if h.upload_session_id != self.session_id.as_bytes() {
                            return Err(ConsistencyViolationError::SessionIdMismatch {
                                expected: self.session_id,
                                got: Uuid::from_slice(&h.upload_session_id)?,
                            }
                            .into());
                        }
                        if h.chunk_length == 0 {
                            return Err(ConsistencyViolationError::InvalidChunkLength {
                                offset: h.chunk_bytes_offset,
                                chunk_size: self.chunk_size,
                                chunk_length: h.chunk_length,
                            }
                            .into());
                        }
                        if h.chunk_bytes_offset + h.chunk_length as u64 > self.file_size {
                            return Err(ConsistencyViolationError::ChunkOffsetOutOfBounds {
                                offset: h.chunk_bytes_offset + h.chunk_length as u64,
                                size: self.file_size,
                            }
                            .into());
                        }
                        if h.chunk_length as u64 != self.expected_chunk_len(h.chunk_bytes_offset) {
                            return Err(ConsistencyViolationError::InvalidChunkLength {
                                offset: h.chunk_bytes_offset,
                                chunk_size: self.chunk_size,
                                chunk_length: h.chunk_length,
                            }
                            .into());
                        }
                        let chunk_idx = self.chunk_idx(h.chunk_bytes_offset)?;
                        let sem = self.chunk_guard.lock().get(chunk_idx).cloned().unwrap();
                        let permit = sem
                            .acquire_owned()
                            .await
                            .map_err(UploadTrackerError::SemaphoreAcquireError)?;
                        state = UploadTrackerACState::ReceivingBody(Some(UploadTrackerCtx {
                            permit,
                            sha1: Sha1::new(),
                            chunk_idx,
                            chunk_inner_offset: 0,
                            header: h,
                        }));
                    }
                    (
                        UploadTrackerACState::ReceivingBody(Some(ctx)),
                        ResourceUploadFrame::BodyChunk(chunk),
                    ) => {
                        if ctx.chunk_idx == 0 && ctx.chunk_inner_offset == 0 {
                            let typ = infer::get(&chunk);
                            match typ {
                                Some(typ) if typ.mime_type() != self.file_mime_type => {
                                    return Err(ConsistencyViolationError::InvalidMimeType {
                                        expected: self.file_mime_type.clone(),
                                        got: typ.mime_type(),
                                    }
                                    .into());
                                }
                                Some(typ) => {
                                    self.file_ext.lock().replace(typ.extension().to_string());
                                }
                                None => {
                                    return Err(
                                        ConsistencyViolationError::FailedInferMimeType.into()
                                    );
                                }
                            }
                        }
                        let file_offset = ctx.header.chunk_bytes_offset + ctx.chunk_inner_offset;
                        ctx.sha1.update(&chunk);
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::FileExt as _;
                            self.tmp_file
                                .as_file()
                                .write_at(&chunk, file_offset)
                                .map_err(UploadTrackerError::StdIO)?;
                        }
                        #[cfg(windows)]
                        {
                            use std::os::windows::fs::FileExt as _;
                            self.tmp_file
                                .as_file()
                                .seek_write(&chunk, file_offset)
                                .map_err(UploadTrackerError::StdIO)?;
                        }
                        ctx.chunk_inner_offset += chunk.len() as u64;
                    }
                    (UploadTrackerACState::ReceivingBody(ctx), ResourceUploadFrame::End) => {
                        let mut ctx = ctx.take().ok_or(
                            UploadTrackerError::UnreachableStreamPath("state machine transform"),
                        )?;
                        if ctx.chunk_inner_offset != ctx.header.chunk_length as u64 {
                            return Err(ConsistencyViolationError::InvalidChunkLength {
                                offset: ctx.header.chunk_bytes_offset,
                                chunk_size: self.chunk_size,
                                chunk_length: ctx.header.chunk_length,
                            }
                            .into());
                        }
                        let final_chunk_sha1 = ctx.sha1.finalize_reset();
                        if final_chunk_sha1.as_slice() != ctx.header.chunk_sha1 {
                            return Err(ConsistencyViolationError::ChunkSha1Mismatch {
                                offset: ctx.header.chunk_bytes_offset,
                                expected: ctx.header.chunk_sha1.hex(),
                                got: final_chunk_sha1.hex(),
                            }
                            .into());
                        }
                        self.seen.as_bitslice().set_aliased(ctx.chunk_idx, true);
                        self.received_bytes
                            .fetch_add(ctx.header.chunk_length as u64, Ordering::Release);
                        state = UploadTrackerACState::Done(Some(ctx));
                    }
                    _ => {
                        return Err(UploadTrackerError::UnreachableStreamPath(
                            "pattern matching",
                        ));
                    }
                },
                Some(Err(e)) => return Err(UploadTrackerError::ResourceUploadProtocol(e)),
                None => {
                    return match state {
                        UploadTrackerACState::Done(Some(_)) => Ok(()),
                        _ => Err(UploadTrackerError::UnexceptedResourceStreamEOF),
                    };
                }
            }
        }
    }

    // TODO: can we have better raii impl? I prefer guard (lock) closure (ctx)...
    pub async fn merge(&mut self) -> UploadTrackerResult<()> {
        let guard = self.exclusive_file_lock.lock();
        if self.seen.count_zeros() > 0 {
            let undone = self.seen.iter_zeros().collect();
            return Err(ConsistencyViolationError::SeenBytesRetained { undone }.into());
        }
        let received_bytes = self.received_bytes.load(Ordering::Acquire);
        if received_bytes != self.file_size {
            return Err(ConsistencyViolationError::BytesRetained { received_bytes }.into());
        }
        self.tmp_file.as_file().sync_all()?;
        self.tmp_file.rewind()?;
        let mut sha1_hash = Sha1::new();
        stream_pipeline(&mut self.tmp_file, |data| {
            sha1_hash.update(data);
        })?;
        let file_sha1 = sha1_hash.finalize();
        if file_sha1.as_slice() != self.file_sha1 {
            return Err(ConsistencyViolationError::FileSha1Mismatch {
                expected: self.file_sha1.hex(),
                got: file_sha1.hex(),
            }
            .into());
        }
        let mut final_file_path = self.final_storage_path.join(self.session_id.to_string());
        final_file_path.set_extension(self.file_ext.lock().as_ref().cloned().unwrap_or_default());
        drop(guard);
        Ok(())
    }

    pub async fn commit(self) -> UploadTrackerResult<(String, String)> {
        let UploadTracker {
            tmp_file,
            file_name,
            session_id,
            file_ext,
            final_storage_path,
            exclusive_file_lock,
            ..
        } = self;
        let guard = exclusive_file_lock.lock();
        let mut final_file_path = final_storage_path.join(session_id.to_string());
        let ext = file_ext.lock().take().unwrap_or_default();
        final_file_path.set_extension(&ext);
        if let Some(parent) = final_file_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            tracing::warn!("Failed to create final storage directory: {}", e);
        }
        fs::copy(tmp_file.path(), &final_file_path)?;
        drop(guard);
        Ok((file_name, ext))
    }
}

pub struct UploadTrackerService {
    state: Arc<EchoState>,
}

impl UploadTrackerService {
    pub async fn new(state: Arc<EchoState>) -> UploadTrackerResult<Self> {
        Ok(Self { state })
    }

    pub async fn get_tracker(&self, session_id: &Uuid) -> Option<MokaVal<Arc<UploadTracker>>> {
        self.state
            .cache
            .get_upload_tracker_session(*session_id)
            .await
    }

    pub async fn init_tracker(
        &self,
        file_meta: UploadCreateReqMetaInfo,
        upload_chunk_size: NonZeroU32,
        session_id: &Uuid,
    ) -> UploadTrackerResult<()> {
        let tracker = UploadTracker::new(
            file_meta,
            *session_id,
            upload_chunk_size,
            &self.state.config.resource.tmp_file_path,
            self.state.config.resource.local_storage_path.clone(),
        )
        .await?;
        self.set_tracker(session_id, Arc::new(tracker), None).await;
        Ok(())
    }

    pub async fn set_tracker(
        &self,
        session_id: &Uuid,
        tracker: Arc<UploadTracker>,
        ttl: Option<MokaExpiration>,
    ) {
        self.state
            .cache
            .set_upload_tracker_session(
                *session_id,
                (
                    ttl.unwrap_or(MokaExpiration::new(TimeDuration::minutes(30))),
                    tracker,
                ),
            )
            .await;
    }

    pub async fn remove_tracker(&self, session_id: &Uuid) -> Option<Arc<UploadTracker>> {
        self.state
            .cache
            .remove_upload_tracker_session(*session_id)
            .await
    }
}
