//! AHX's Layer II allocation, scale selection, and dequantization stages.
// Derived from mpg123 1.33.7; LGPL-2.1-or-later. See COPYING and NOTICE.

use crate::{
    bits::BitReader,
    error::{Error, InvalidData, Result},
    input::Input,
    tables,
};

use GroupedQuantizer::{Levels3, Levels5, Levels9};
use Quantizer::{Grouped, Ungrouped};
use UngroupedQuantizer::{
    Bits10, Bits11, Bits12, Bits13, Bits14, Bits3, Bits4, Bits5, Bits6, Bits7, Bits8, Bits9,
};

pub(crate) type SubbandSamples = [[f32; 32]; 36];

#[derive(Clone, Copy)]
enum GroupedQuantizer {
    Levels3,
    Levels5,
    Levels9,
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum UngroupedQuantizer {
    Bits3 = 3,
    Bits4,
    Bits5,
    Bits6,
    Bits7,
    Bits8,
    Bits9,
    Bits10,
    Bits11,
    Bits12,
    Bits13,
    Bits14,
}

impl UngroupedQuantizer {
    fn read(self, bits: &mut BitReader<'_, impl Input>) -> Result<u32> {
        match self {
            Self::Bits3 => bits.read::<3>(),
            Self::Bits4 => bits.read::<4>(),
            Self::Bits5 => bits.read::<5>(),
            Self::Bits6 => bits.read::<6>(),
            Self::Bits7 => bits.read::<7>(),
            Self::Bits8 => bits.read::<8>(),
            Self::Bits9 => bits.read::<9>(),
            Self::Bits10 => bits.read::<10>(),
            Self::Bits11 => bits.read::<11>(),
            Self::Bits12 => bits.read::<12>(),
            Self::Bits13 => bits.read::<13>(),
            Self::Bits14 => bits.read::<14>(),
        }
    }
}

#[derive(Clone, Copy)]
enum Quantizer {
    Grouped(GroupedQuantizer),
    Ungrouped(UngroupedQuantizer),
}

impl Quantizer {
    fn decode(self, bits: &mut BitReader<'_, impl Input>, scale: ScaleFactor) -> Result<[f32; 3]> {
        let mut samples = [0.; 3];
        match self {
            Self::Ungrouped(quantizer) => {
                let width = quantizer as u8;
                let bias = (1i32 << (width - 1)) - 1;
                let multiplier = tables::MULS[usize::from(width)][scale.index()];
                for sample in &mut samples {
                    *sample = (quantizer.read(bits)? as i32 - bias) as f32 * multiplier;
                }
            }
            Self::Grouped(quantizer) => {
                let (mut value, indices): (u32, &[usize]) = match quantizer {
                    GroupedQuantizer::Levels3 => (bits.read::<5>()?, &[1, 0, 2]),
                    GroupedQuantizer::Levels5 => (bits.read::<7>()?, &[17, 18, 0, 19, 20]),
                    GroupedQuantizer::Levels9 => {
                        (bits.read::<10>()?, &[21, 1, 22, 23, 0, 24, 25, 2, 26])
                    }
                };
                let radix = indices.len() as u32;
                if value >= radix.pow(3) {
                    return Err(Error::Invalid(InvalidData::GroupedQuantizer));
                }
                for sample in &mut samples {
                    *sample = tables::MULS[indices[(value % radix) as usize]][scale.index()];
                    value /= radix;
                }
            }
        }
        Ok(samples)
    }
}

const FULL_ALLOCATION: [Option<Quantizer>; 16] = [
    None,
    Some(Grouped(Levels3)),
    Some(Grouped(Levels5)),
    Some(Ungrouped(Bits3)),
    Some(Grouped(Levels9)),
    Some(Ungrouped(Bits4)),
    Some(Ungrouped(Bits5)),
    Some(Ungrouped(Bits6)),
    Some(Ungrouped(Bits7)),
    Some(Ungrouped(Bits8)),
    Some(Ungrouped(Bits9)),
    Some(Ungrouped(Bits10)),
    Some(Ungrouped(Bits11)),
    Some(Ungrouped(Bits12)),
    Some(Ungrouped(Bits13)),
    Some(Ungrouped(Bits14)),
];

const SHORT_ALLOCATION: [Option<Quantizer>; 8] = [
    None,
    Some(Grouped(Levels3)),
    Some(Grouped(Levels5)),
    Some(Grouped(Levels9)),
    Some(Ungrouped(Bits4)),
    Some(Ungrouped(Bits5)),
    Some(Ungrouped(Bits6)),
    Some(Ungrouped(Bits7)),
];

#[derive(Clone, Copy)]
enum ScaleSelection {
    Independent,
    ReuseFirst,
    ReuseAll,
    ReuseLast,
}

impl ScaleSelection {
    fn read(bits: &mut BitReader<'_, impl Input>) -> Result<Self> {
        // Two booleans enumerate all four codes without an unreachable fallback.
        Ok(match (bits.read_bit()?, bits.read_bit()?) {
            (false, false) => Self::Independent,
            (false, true) => Self::ReuseFirst,
            (true, false) => Self::ReuseAll,
            (true, true) => Self::ReuseLast,
        })
    }

    fn read_factors(self, bits: &mut BitReader<'_, impl Input>) -> Result<[ScaleFactor; 3]> {
        let first = ScaleFactor::read(bits)?;
        Ok(match self {
            Self::Independent => [first, ScaleFactor::read(bits)?, ScaleFactor::read(bits)?],
            Self::ReuseFirst => [first, first, ScaleFactor::read(bits)?],
            Self::ReuseAll => [first; 3],
            Self::ReuseLast => {
                let last = ScaleFactor::read(bits)?;
                [first, last, last]
            }
        })
    }
}

// Only a six-bit field can construct a scale-table index.
#[derive(Clone, Copy)]
struct ScaleFactor(u8);

impl ScaleFactor {
    fn read(bits: &mut BitReader<'_, impl Input>) -> Result<Self> {
        Ok(Self(bits.read::<6>()? as u8))
    }

    fn index(self) -> usize {
        usize::from(self.0)
    }
}

#[derive(Clone, Copy)]
enum Band<S> {
    Silent,
    Coded { quantizer: Quantizer, scaling: S },
}

// MPEG stores allocation, scale-selection codes, and scale factors in separate
// passes. Each pass consumes its input state; only complete parameters can decode.
struct BandParameters<S>([Band<S>; 30]);

impl<S> BandParameters<S> {
    fn try_map<T>(self, mut map: impl FnMut(S) -> Result<T>) -> Result<BandParameters<T>> {
        let mut mapped = core::array::from_fn(|_| Band::Silent);
        for (output, band) in mapped.iter_mut().zip(self.0) {
            *output = match band {
                Band::Silent => Band::Silent,
                Band::Coded { quantizer, scaling } => Band::Coded {
                    quantizer,
                    scaling: map(scaling)?,
                },
            };
        }
        Ok(BandParameters(mapped))
    }
}

impl BandParameters<()> {
    fn read(bits: &mut BitReader<'_, impl Input>) -> Result<Self> {
        let mut bands = [Band::Silent; 30];
        for (index, band) in bands.iter_mut().enumerate() {
            let quantizer = match index {
                0..=3 => FULL_ALLOCATION[bits.read::<4>()? as usize],
                4..=10 => SHORT_ALLOCATION[bits.read::<3>()? as usize],
                _ => SHORT_ALLOCATION[bits.read::<2>()? as usize],
            };
            if let Some(quantizer) = quantizer {
                *band = Band::Coded {
                    quantizer,
                    scaling: (),
                };
            }
        }
        Ok(Self(bands))
    }

    fn read_selections(
        self,
        bits: &mut BitReader<'_, impl Input>,
    ) -> Result<BandParameters<ScaleSelection>> {
        self.try_map(|()| ScaleSelection::read(bits))
    }
}

impl BandParameters<ScaleSelection> {
    fn read_factors(
        self,
        bits: &mut BitReader<'_, impl Input>,
    ) -> Result<BandParameters<[ScaleFactor; 3]>> {
        self.try_map(|selection| selection.read_factors(bits))
    }
}

impl BandParameters<[ScaleFactor; 3]> {
    fn read_samples(self, bits: &mut BitReader<'_, impl Input>) -> Result<SubbandSamples> {
        let mut decoded = [[0.; 32]; 36];
        let (granules, _) = decoded.as_chunks_mut::<3>();
        for (group, granule) in granules.iter_mut().enumerate() {
            for (index, band) in self.0.iter().enumerate() {
                if let Band::Coded { quantizer, scaling } = band {
                    let samples = quantizer.decode(bits, scaling[group / 4])?;
                    for (output, sample) in granule.iter_mut().zip(samples) {
                        output[index] = sample;
                    }
                }
            }
        }
        Ok(decoded)
    }
}

pub(crate) fn read_subbands(bits: &mut BitReader<'_, impl Input>) -> Result<SubbandSamples> {
    BandParameters::read(bits)?
        .read_selections(bits)?
        .read_factors(bits)?
        .read_samples(bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::SliceInput;

    #[test]
    fn scale_selection_reuses_the_correct_factors_and_consumes_only_encoded_fields() {
        // The wire has a two-bit selection followed by six-bit scale factors.
        // Distinct factors expose both reuse order and accidentally overreading.
        for (code, expected, consumed) in [
            (0u32, [3, 5, 7], 7),
            (1, [3, 3, 5], 6),
            (2, [3, 3, 3], 5),
            (3, [3, 5, 5], 6),
        ] {
            let word = (code << 30) | (3 << 24) | (5 << 18) | (7 << 12);
            let bytes = word.to_be_bytes();
            let mut input = SliceInput(&bytes);
            let mut bits = BitReader::new(&mut input);
            let factors = ScaleSelection::read(&mut bits)
                .unwrap()
                .read_factors(&mut bits)
                .unwrap();
            assert_eq!(factors.map(ScaleFactor::index), expected);
            assert_eq!(bits.consumed_bytes(), consumed);
        }
    }

    #[test]
    fn grouped_quantizers_reject_unused_wire_codes_at_every_radix() {
        for (quantizer, width, radix) in [(Levels3, 5, 3u32), (Levels5, 7, 5), (Levels9, 10, 9)] {
            for code in [0, radix.pow(3) - 1, radix.pow(3), (1 << width) - 1] {
                let bytes = (code << (32 - width)).to_be_bytes();
                let mut input = SliceInput(&bytes);
                let mut bits = BitReader::new(&mut input);
                let result = Grouped(quantizer).decode(&mut bits, ScaleFactor(0));
                if code < radix.pow(3) {
                    assert!(result.is_ok());
                } else {
                    assert!(matches!(
                        result,
                        Err(Error::Invalid(InvalidData::GroupedQuantizer))
                    ));
                }
            }
        }
    }
}
