#![doc = include_str!("../README.md")]
#![no_std]
#![forbid(unsafe_code)]

#[cfg(any(feature = "std", test))]
extern crate std;

mod bits;
mod dct;
mod error;
mod input;
mod metadata;
mod packet;
mod quantization;
mod stream;
mod synthesis;
mod tables;

pub use error::{Error, InvalidData, UnsupportedData};
pub use metadata::Metadata;
pub use packet::{DecodedPacket, PacketDecoder};
#[cfg(feature = "std")]
pub use stream::Decoder;
pub use stream::{DecoderState, SliceDecoder};

/// Mono PCM16 elements produced by one complete compressed frame.
/// Caller-owned output buffers need at least this many `i16` elements, even
/// when the container trims the final block to a shorter declared duration.
pub const SAMPLES_PER_FRAME: usize = 1152;

/// Maximum compressed frame bytes, including its four-byte MPEG header.
/// Useful for bounded custom streaming input. Frames are variable length;
/// [`DecodedPacket::consumed_bytes`] reports the length actually consumed.
pub const MAX_FRAME_BYTES: usize = 0x414;
