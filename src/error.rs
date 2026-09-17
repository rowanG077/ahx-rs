use core::fmt;

/// A structural violation of the supported AHX format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvalidData {
    /// The input does not begin with the AHX file signature.
    FileSignature,
    /// The encoded header length cannot contain the required fields and signature.
    HeaderLength,
    /// The complete header lacks its CRI copyright signature.
    CopyrightSignature,
    /// The declared sample count is zero or exceeds the supported signed range.
    SampleCount(u32),
    /// A packet does not begin with the supported fixed MPEG header.
    FrameHeader,
    /// A grouped quantizer contains a value outside its radix's three-digit range.
    GroupedQuantizer,
    /// A compressed frame exceeds the format's byte bound.
    FrameLength,
    /// The final packet is not followed by the CRI end marker.
    EndMarker,
}

impl fmt::Display for InvalidData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileSignature => f.write_str("file signature"),
            Self::HeaderLength => f.write_str("header length"),
            Self::CopyrightSignature => f.write_str("copyright signature"),
            Self::SampleCount(count) => write!(f, "declared sample count {count}"),
            Self::FrameHeader => f.write_str("MPEG frame header"),
            Self::GroupedQuantizer => f.write_str("grouped quantizer"),
            Self::FrameLength => f.write_str("frame length"),
            Self::EndMarker => f.write_str("end marker"),
        }
    }
}

/// A codec profile or playback rate outside the supported AHX subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnsupportedData {
    /// The header is not unencrypted mono type `0x10`, version 6.
    Profile,
    /// The declared playback rate in Hz is unsupported.
    SampleRate(u32),
}

impl fmt::Display for UnsupportedData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile => f.write_str("expected unencrypted mono type 0x10 version 6"),
            Self::SampleRate(rate) => write!(f, "declared sample rate {rate} Hz"),
        }
    }
}

/// A decoding, profile, caller-buffer, or input/output error.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Input ended inside a header, frame, or end marker.
    Truncated,
    /// The input uses a codec profile or rate that this decoder does not support.
    Unsupported(UnsupportedData),
    /// The input violates the supported format.
    Invalid(InvalidData),
    /// A container decode previously failed; construct a new decoder to restart.
    Failed,
    /// Caller-owned PCM storage cannot hold a complete frame.
    BufferTooSmall {
        /// Required number of `i16` elements.
        required: usize,
        /// Provided number of `i16` elements.
        provided: usize,
    },
    /// The standard I/O reader failed for a reason other than unexpected EOF.
    #[cfg(feature = "std")]
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated AHX data"),
            Self::Unsupported(reason) => write!(f, "unsupported AHX: {reason}"),
            Self::Invalid(reason) => write!(f, "invalid AHX: {reason}"),
            Self::Failed => f.write_str("AHX decoder cannot continue after an error"),
            Self::BufferTooSmall { required, provided } => write!(
                f,
                "PCM buffer needs {required} i16 elements, received {provided}"
            ),
            #[cfg(feature = "std")]
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        #[cfg(feature = "std")]
        if let Self::Io(error) = self {
            return Some(error);
        }
        None
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Self::Truncated
        } else {
            Self::Io(error)
        }
    }
}

pub(crate) type Result<T> = core::result::Result<T, Error>;
