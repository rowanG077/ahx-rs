use crate::{
    bits::BitReader,
    error::{Error, InvalidData, Result},
    input::{Input, SliceInput},
    quantization::{self, SubbandSamples},
    synthesis::Synthesis,
    SAMPLES_PER_FRAME,
};

pub(crate) const FRAME_HEADER: [u8; 4] = [0xff, 0xf5, 0xe0, 0xc0];

/// One complete decoded packet, borrowing the caller's PCM storage.
#[derive(Debug)]
pub struct DecodedPacket<'a> {
    pcm: &'a [i16; SAMPLES_PER_FRAME],
    consumed: usize,
}

impl<'a> DecodedPacket<'a> {
    /// The complete frame's 1,152 mono PCM16 elements, with its size in the type.
    pub fn pcm(&self) -> &'a [i16; SAMPLES_PER_FRAME] {
        self.pcm
    }

    /// Compressed bytes consumed, including the MPEG header and final padded byte.
    /// Any following frame or container footer is left untouched.
    pub const fn consumed_bytes(&self) -> usize {
        self.consumed
    }
}

/// Allocation-free decoder for AHX's fixed-profile MPEG frame packets.
///
/// Supply consecutive frames from a single stream to preserve synthesis history.
/// This API does not parse an AHX header/footer or trim the final frame. Use
/// [`crate::SliceDecoder`] or the standard I/O decoder for complete AHX files.
/// All packet validation precedes synthesis. Errors preserve both output and
/// synthesis state, allowing a truncated input or small output buffer to be retried.
pub struct PacketDecoder {
    synthesis: Synthesis,
}

impl core::fmt::Debug for PacketDecoder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PacketDecoder").finish_non_exhaustive()
    }
}

impl Default for PacketDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketDecoder {
    /// Initialize zeroed synthesis history without allocating.
    pub fn new() -> Self {
        Self {
            synthesis: Synthesis::new(),
        }
    }

    /// Decode the first complete frame in `input` into caller-owned PCM16.
    ///
    /// Requires [`SAMPLES_PER_FRAME`] output elements. Extra output is unchanged.
    /// Extra input is not consumed. `Truncated` means the first frame needs more
    /// input; retain this prefix and retry with more bytes, up to [`crate::MAX_FRAME_BYTES`].
    pub fn decode_into<'a>(
        &mut self,
        input: &[u8],
        pcm: &'a mut [i16],
    ) -> Result<DecodedPacket<'a>> {
        let pcm = frame_buffer(pcm)?;
        let consumed = self.decode_from(&mut SliceInput(input), pcm)?;
        Ok(DecodedPacket { pcm, consumed })
    }

    pub(crate) fn decode_from(
        &mut self,
        input: &mut impl Input,
        pcm: &mut [i16; SAMPLES_PER_FRAME],
    ) -> Result<usize> {
        let frame = ParsedFrame::read(input)?;
        // Synthesis is infallible and can only see fully validated parameters
        // and a complete output array. Parsing never receives mutable history.
        frame.synthesize(&mut self.synthesis, pcm);
        Ok(frame.consumed)
    }
}

pub(crate) fn frame_buffer(pcm: &mut [i16]) -> Result<&mut [i16; SAMPLES_PER_FRAME]> {
    let provided = pcm.len();
    pcm.first_chunk_mut().ok_or(Error::BufferTooSmall {
        required: SAMPLES_PER_FRAME,
        provided,
    })
}

struct ParsedFrame {
    bands: SubbandSamples,
    consumed: usize,
}

impl ParsedFrame {
    fn read(input: &mut impl Input) -> Result<Self> {
        let mut header = [0; FRAME_HEADER.len()];
        input.read_exact(&mut header)?;
        if header != FRAME_HEADER {
            return Err(Error::Invalid(InvalidData::FrameHeader));
        }
        let mut bits = BitReader::new(input);
        let bands = quantization::read_subbands(&mut bits)?;
        Ok(Self {
            bands,
            consumed: bits.consumed_bytes(),
        })
    }

    fn synthesize(&self, synthesis: &mut Synthesis, pcm: &mut [i16; SAMPLES_PER_FRAME]) {
        let (blocks, _) = pcm.as_chunks_mut::<32>();
        for (band, output) in self.bands.iter().zip(blocks) {
            synthesis.decode(band, output);
        }
    }
}
