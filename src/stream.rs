use core::num::NonZeroU32;

use crate::{
    error::{Error, InvalidData, Result},
    input::{Input, SliceInput},
    packet::frame_buffer,
    Metadata, PacketDecoder, SAMPLES_PER_FRAME,
};

const FOOTER: &[u8; 17] = b"\0\x80\x01\0\x0cAHXE(c)CRI\0\0";

/// The mutually exclusive states of a complete-file decoder.
///
/// Returned by [`SliceDecoder::state`] and the standard I/O decoder's `state` method.
/// A decoding stream always has samples remaining. `Finished` is reached only
/// after validating the end marker; `Failed` cannot resume decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderState {
    /// More declared PCM remains to be decoded.
    Decoding {
        /// Mono PCM16 samples still to be returned, including the next block.
        remaining_samples: NonZeroU32,
    },
    /// All declared samples and the end marker were successfully decoded.
    Finished,
    /// An error invalidated the stream. Construct a new decoder to restart.
    Failed,
}

struct Stream<S, P> {
    input: S,
    metadata: Metadata,
    state: DecoderState,
    codec: PacketDecoder,
    pcm: P,
}

impl<S: Input, P: AsMut<[i16]>> Stream<S, P> {
    fn new(mut input: S, mut pcm: P) -> Result<Self> {
        frame_buffer(pcm.as_mut())?;
        let metadata = Metadata::from_input(&mut input)?;
        Ok(Self {
            input,
            metadata,
            state: DecoderState::Decoding {
                remaining_samples: metadata.sample_count(),
            },
            codec: PacketDecoder::new(),
            pcm,
        })
    }

    fn next_block(&mut self) -> Result<Option<&[i16]>> {
        let remaining = match self.state {
            DecoderState::Decoding { remaining_samples } => remaining_samples,
            DecoderState::Finished => return Ok(None),
            DecoderState::Failed => return Err(Error::Failed),
        };
        // Commit a resumable state only on success. This also poisons the stream
        // if caller-provided I/O or storage panics and the caller catches the unwind.
        self.state = DecoderState::Failed;
        let pcm = frame_buffer(self.pcm.as_mut())?;
        self.codec.decode_from(&mut self.input, pcm)?;
        let count = remaining.get().min(SAMPLES_PER_FRAME as u32);
        let next = match NonZeroU32::new(remaining.get() - count) {
            Some(remaining_samples) => DecoderState::Decoding { remaining_samples },
            None => {
                let mut footer = [0; FOOTER.len()];
                self.input.read_exact(&mut footer)?;
                if &footer != FOOTER {
                    return Err(Error::Invalid(InvalidData::EndMarker));
                }
                DecoderState::Finished
            }
        };
        self.state = next;
        Ok(Some(&pcm[..count as usize]))
    }
}

/// Allocation-free decoder over a complete AHX byte slice.
///
/// [`Self::new`] owns a fixed PCM array. [`Self::with_buffer`] accepts caller-owned
/// storage instead. Neither constructor requires `std` or `alloc`. Input is
/// consumed only as frames are decoded; trailing bytes after the footer remain unread.
pub struct SliceDecoder<'input, P = [i16; SAMPLES_PER_FRAME]> {
    inner: Stream<SliceInput<'input>, P>,
}

impl<P> core::fmt::Debug for SliceDecoder<'_, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SliceDecoder")
            .field("metadata", &self.inner.metadata)
            .field("state", &self.inner.state)
            .field("remaining_bytes", &self.inner.input.0.len())
            .finish_non_exhaustive()
    }
}

impl<'input, P> SliceDecoder<'input, P> {
    /// Validated file timing and header layout.
    pub const fn metadata(&self) -> Metadata {
        self.inner.metadata
    }

    /// Current decoding progress, successful completion, or terminal failure.
    pub const fn state(&self) -> DecoderState {
        self.inner.state
    }

    /// Recover unconsumed input and PCM storage without rewinding.
    /// After successful EOF the input begins immediately after the AHX end marker.
    pub fn into_inner(self) -> (&'input [u8], P) {
        (self.inner.input.0, self.inner.pcm)
    }
}

impl<'input> SliceDecoder<'input> {
    /// Validate the header and initialize fixed synthesis and PCM arrays.
    pub fn new(input: &'input [u8]) -> Result<Self> {
        Self::with_buffer(input, [0; SAMPLES_PER_FRAME])
    }
}

impl<'input, P: AsMut<[i16]>> SliceDecoder<'input, P> {
    /// Use caller-owned storage with at least [`SAMPLES_PER_FRAME`] `i16` elements.
    ///
    /// Short buffers are rejected before parsing input or modifying output. Borrow
    /// a slice to retain ownership on constructor failure. Storage length is checked
    /// again on each block; shorter views return an error and invalidate the decoder.
    pub fn with_buffer(input: &'input [u8], pcm: P) -> Result<Self> {
        Ok(Self {
            inner: Stream::new(SliceInput(input), pcm)?,
        })
    }

    /// Decode the next borrowed mono PCM16 block, or `None` at validated EOF.
    ///
    /// The final block is trimmed to the declared sample count; its footer is
    /// checked before the block is returned. Calls after successful EOF return
    /// `None`. Any error invalidates the reader; later calls return [`Error::Failed`].
    /// An unwinding panic in caller-provided storage also leaves it failed.
    pub fn next_block(&mut self) -> Result<Option<&[i16]>> {
        self.inner.next_block()
    }
}

/// Streaming AHX decoder with fixed synthesis and PCM arrays, available with `std`.
///
/// No allocation or seeking is performed by this decoder. Wrap files in
/// `std::io::BufReader` for efficient small reads. Reader-owned buffering and
/// output handling are outside the decoder's allocation guarantee.
#[cfg(feature = "std")]
pub struct Decoder<R> {
    inner: Stream<crate::input::ReaderInput<R>, [i16; SAMPLES_PER_FRAME]>,
}

#[cfg(feature = "std")]
impl<R> core::fmt::Debug for Decoder<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Decoder")
            .field("metadata", &self.inner.metadata)
            .field("state", &self.inner.state)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "std")]
impl<R> Decoder<R> {
    /// Validated file timing and header layout.
    pub const fn metadata(&self) -> Metadata {
        self.inner.metadata
    }

    /// Current decoding progress, successful completion, or terminal failure.
    pub const fn state(&self) -> DecoderState {
        self.inner.state
    }

    /// Recover the reader at its current position, without rewinding.
    pub fn into_inner(self) -> R {
        self.inner.input.0
    }
}

#[cfg(feature = "std")]
impl<R: std::io::Read> Decoder<R> {
    /// Validate the header and initialize fixed-size storage without allocating.
    pub fn new(reader: R) -> Result<Self> {
        Ok(Self {
            inner: Stream::new(crate::input::ReaderInput(reader), [0; SAMPLES_PER_FRAME])?,
        })
    }

    /// Decode up to [`SAMPLES_PER_FRAME`] mono PCM16 elements into borrowed storage.
    ///
    /// No startup samples are discarded and no resampling occurs. The final block
    /// is trimmed to the declared duration after validating the footer. Errors
    /// invalidate the reader; calls after an error return [`Error::Failed`].
    /// An unwinding panic in caller I/O also leaves it failed.
    /// Calls after successful EOF return `None` without reading further input.
    pub fn next_block(&mut self) -> Result<Option<&[i16]>> {
        self.inner.next_block()
    }
}
