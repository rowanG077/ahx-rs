use core::num::NonZeroU32;

use crate::{
    error::{Error, InvalidData, Result, UnsupportedData},
    input::{Input, SliceInput},
};

const COPYRIGHT: &[u8; 6] = b"(c)CRI";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleRate {
    Hz32000,
    Hz44100,
    Hz48000,
}

impl SampleRate {
    fn parse(hz: u32) -> Result<Self> {
        match hz {
            32000 => Ok(Self::Hz32000),
            44100 => Ok(Self::Hz44100),
            48000 => Ok(Self::Hz48000),
            _ => Err(Error::Unsupported(UnsupportedData::SampleRate(hz))),
        }
    }

    const fn hz(self) -> u32 {
        match self {
            Self::Hz32000 => 32000,
            Self::Hz44100 => 44100,
            Self::Hz48000 => 48000,
        }
    }
}

// A valid prefix is not yet valid Metadata: the complete header's trailing
// copyright signature must also be present and correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeaderPrefix {
    sample_rate: SampleRate,
    samples: NonZeroU32,
    header_len: usize,
}

impl HeaderPrefix {
    fn parse(input: &[u8]) -> Result<Self> {
        let header = input.get(..Metadata::PREFIX_LEN).ok_or(Error::Truncated)?;
        if header[..2] != [0x80, 0] {
            return Err(Error::Invalid(InvalidData::FileSignature));
        }
        if header[4..8] != [0x10, 0, 0, 1] || header[18..20] != [6, 0] {
            return Err(Error::Unsupported(UnsupportedData::Profile));
        }
        let header_len = usize::from(u16::from_be_bytes([header[2], header[3]]))
            .checked_add(4)
            .filter(|&len| len >= Metadata::PREFIX_LEN + COPYRIGHT.len())
            .ok_or(Error::Invalid(InvalidData::HeaderLength))?;
        let sample_rate = SampleRate::parse(u32::from_be_bytes([
            header[8], header[9], header[10], header[11],
        ]))?;
        let count = u32::from_be_bytes([header[12], header[13], header[14], header[15]]);
        let samples = NonZeroU32::new(count)
            .filter(|n| n.get() <= i32::MAX as u32)
            .ok_or(Error::Invalid(InvalidData::SampleCount(count)))?;
        Ok(Self {
            sample_rate,
            samples,
            header_len,
        })
    }
}

/// Validated, immutable AHX timing and header layout.
///
/// AHX's declared rate controls playback. The fixed MPEG frame header's rate is
/// not used for playback; no resampling or startup delay removal is performed.
/// Instances exist only after validating the complete header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
    header: HeaderPrefix,
}

impl Metadata {
    /// Bytes required to discover the complete, variable-length header size.
    pub const PREFIX_LEN: usize = 20;

    /// Parse a complete header from a prefix of an AHX file, without allocation.
    /// Bytes after the header are ignored. Frame contents are checked during decoding.
    pub fn parse(input: &[u8]) -> Result<Self> {
        Self::from_input(&mut SliceInput(input))
    }

    /// Read the complete header, leaving the reader at its first compressed frame.
    #[cfg(feature = "std")]
    pub fn read<R: std::io::Read>(reader: &mut R) -> Result<Self> {
        Self::from_input(&mut crate::input::ReaderInput(reader))
    }

    /// Discover header bytes needed from the first [`Self::PREFIX_LEN`] bytes.
    /// The complete header still needs validation with [`Self::parse`].
    pub fn required_header_len(prefix: &[u8]) -> Result<usize> {
        HeaderPrefix::parse(prefix).map(|header| header.header_len)
    }

    pub(crate) fn from_input(input: &mut impl Input) -> Result<Self> {
        let mut bytes = [0; Self::PREFIX_LEN];
        input.read_exact(&mut bytes)?;
        let header = HeaderPrefix::parse(&bytes)?;
        // Bounded scratch and chunked reads, even for the longest encoded header.
        let mut scratch = [0; 256];
        let mut remaining = header.header_len - Self::PREFIX_LEN - COPYRIGHT.len();
        while remaining != 0 {
            let count = remaining.min(scratch.len());
            input.read_exact(&mut scratch[..count])?;
            remaining -= count;
        }
        input.read_exact(&mut scratch[..COPYRIGHT.len()])?;
        if &scratch[..COPYRIGHT.len()] != COPYRIGHT {
            return Err(Error::Invalid(InvalidData::CopyrightSignature));
        }
        Ok(Self { header })
    }

    pub(crate) const fn sample_count(self) -> NonZeroU32 {
        self.header.samples
    }

    /// Declared playback rate in Hz.
    pub const fn sample_rate(self) -> u32 {
        self.header.sample_rate.hz()
    }

    /// Declared mono PCM16 sample count, including startup synthesis samples.
    /// Always in `1..=i32::MAX` for successfully parsed metadata.
    pub const fn samples(self) -> u32 {
        self.header.samples.get()
    }

    /// Number of output channels (one for the supported profile).
    pub const fn channels(self) -> u16 {
        1
    }

    /// Byte offset of the first MPEG frame, including the complete AHX header.
    pub const fn header_len(self) -> usize {
        self.header.header_len
    }
}
