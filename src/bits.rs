//! Bounded, most-significant-bit-first reads from an AHX frame payload.

use crate::{
    error::{Error, InvalidData, Result},
    input::Input,
    packet::FRAME_HEADER,
    MAX_FRAME_BYTES,
};

#[derive(Clone, Copy)]
#[repr(u8)]
enum BitPosition {
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
}

impl BitPosition {
    fn next(self) -> Option<Self> {
        match self {
            Self::Seven => Some(Self::Six),
            Self::Six => Some(Self::Five),
            Self::Five => Some(Self::Four),
            Self::Four => Some(Self::Three),
            Self::Three => Some(Self::Two),
            Self::Two => Some(Self::One),
            Self::One => Some(Self::Zero),
            Self::Zero => None,
        }
    }
}

#[derive(Clone, Copy)]
enum BufferedBits {
    Empty,
    Pending { byte: u8, position: BitPosition },
}

pub(crate) struct BitReader<'a, R> {
    input: &'a mut R,
    buffered: BufferedBits,
    payload_bytes: usize,
}

impl<'a, R: Input> BitReader<'a, R> {
    pub(crate) fn new(input: &'a mut R) -> Self {
        Self {
            input,
            buffered: BufferedBits::Empty,
            payload_bytes: 0,
        }
    }

    pub(crate) fn read_bit(&mut self) -> Result<bool> {
        let (byte, position) = match self.buffered {
            BufferedBits::Empty => {
                if self.payload_bytes == MAX_FRAME_BYTES - FRAME_HEADER.len() {
                    return Err(Error::Invalid(InvalidData::FrameLength));
                }
                let mut byte = [0];
                self.input.read_exact(&mut byte)?;
                self.payload_bytes += 1;
                (byte[0], BitPosition::Seven)
            }
            BufferedBits::Pending { byte, position } => (byte, position),
        };
        self.buffered = match position.next() {
            Some(position) => BufferedBits::Pending { byte, position },
            None => BufferedBits::Empty,
        };
        Ok(byte & (1 << position as u8) != 0)
    }

    pub(crate) fn read<const WIDTH: u8>(&mut self) -> Result<u32> {
        const { assert!(WIDTH > 0 && WIDTH <= u32::BITS as u8) };
        let mut value = 0;
        for _ in 0..WIDTH {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Ok(value)
    }

    pub(crate) fn consumed_bytes(&self) -> usize {
        FRAME_HEADER.len() + self.payload_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::SliceInput;

    #[test]
    fn reads_across_byte_boundaries_without_consuming_trailing_input() {
        let mut input = SliceInput(&[0xd3, 0x69, 0xa5, 0x5a]);
        let mut bits = BitReader::new(&mut input);
        assert_eq!(bits.read::<3>().unwrap(), 0b110);
        assert_eq!(bits.consumed_bytes(), 5);
        assert_eq!(bits.read::<9>().unwrap(), 0b100110110);
        assert_eq!(bits.consumed_bytes(), 6);
        assert_eq!(bits.read::<8>().unwrap(), 0b10011010);
        assert_eq!(bits.consumed_bytes(), 7);
        assert_eq!(input.0, &[0x5a]); // Four padding bits share the final byte.

        let mut input = SliceInput(&[0xd3, 0x69, 0xa5, 0x5a]);
        let mut bits = BitReader::new(&mut input);
        assert_eq!(bits.read::<32>().unwrap(), 0xd369_a55a);
        assert!(matches!(bits.read_bit(), Err(Error::Truncated)));
    }

    #[test]
    fn enforces_the_byte_bound_before_touching_more_input() {
        let data = [0xff; MAX_FRAME_BYTES];
        let mut input = SliceInput(&data);
        let mut bits = BitReader::new(&mut input);
        for _ in 0..MAX_FRAME_BYTES - FRAME_HEADER.len() {
            assert_eq!(bits.read::<8>().unwrap(), 0xff);
        }
        assert_eq!(bits.consumed_bytes(), MAX_FRAME_BYTES);
        assert!(matches!(
            bits.read_bit(),
            Err(Error::Invalid(InvalidData::FrameLength))
        ));
        assert_eq!(input.0.len(), FRAME_HEADER.len());
    }
}
