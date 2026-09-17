//! Exact synthetic reference and malformed-input coverage.

use ahx::{Error, SliceDecoder as Decoder};

const INPUT: &[u8] = include_bytes!("fixtures/continuity.ahx");
const PCM: &[u8] = include_bytes!("fixtures/continuity.pcm");

fn decode(input: &[u8]) -> Result<Vec<i16>, Error> {
    let mut d = Decoder::new(input)?;
    let mut out = Vec::new();
    while let Some(b) = d.next_block()? {
        out.extend_from_slice(b);
    }
    Ok(out)
}

#[test]
fn canonical_pcm_and_synthesis_continuity() {
    let expected: Vec<_> = PCM
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]))
        .collect();
    assert_eq!(decode(INPUT).unwrap(), expected);
    assert!(expected[1152..].iter().any(|s| *s != 0));
    let mut d = Decoder::new(INPUT).unwrap();
    assert_eq!(d.metadata().samples(), 2304);
    assert_eq!(d.next_block().unwrap().unwrap().len(), 1152);
    assert_eq!(d.next_block().unwrap().unwrap().len(), 1152);
    assert!(d.next_block().unwrap().is_none());
    assert!(d.next_block().unwrap().is_none());
}

#[test]
fn final_frame_is_trimmed_without_delay_or_resampling() {
    let mut input = INPUT.to_vec();
    input[12..16].copy_from_slice(&1201u32.to_be_bytes());
    let pcm = decode(&input).unwrap();
    assert_eq!(pcm, decode(INPUT).unwrap()[..1201]);
    for rate in [32000u32, 44100, 48000] {
        input[8..12].copy_from_slice(&rate.to_be_bytes());
        let d = Decoder::new(input.as_slice()).unwrap();
        assert_eq!(d.metadata().sample_rate(), rate);
        assert_eq!(decode(&input).unwrap(), pcm);
    }
}

#[test]
fn every_truncation_fails_and_errors_poison_the_decoder() {
    for n in 0..INPUT.len() {
        assert!(decode(&INPUT[..n]).is_err(), "accepted prefix {n}");
    }
    let mut bad = INPUT.to_vec();
    bad[32] = 0;
    let mut d = Decoder::new(bad.as_slice()).unwrap();
    assert!(d.next_block().is_err());
    assert!(matches!(d.next_block(), Err(Error::Failed)));
}

#[test]
fn unsupported_profiles_and_length_bounds() {
    for (offset, value) in [(4, 0x11), (7, 2), (18, 5), (19, 8), (2, 0), (3, 0), (31, 0)] {
        let mut b = INPUT.to_vec();
        b[offset] = value;
        if b != INPUT {
            assert!(decode(&b).is_err());
        }
    }
    for samples in [0u32, u32::MAX, 2305] {
        let mut b = INPUT.to_vec();
        b[12..16].copy_from_slice(&samples.to_be_bytes());
        assert!(decode(&b).is_err());
    }
    let mut b = INPUT.to_vec();
    b[8..12].copy_from_slice(&8000u32.to_be_bytes());
    assert!(decode(&b).is_err());
}

#[test]
fn deterministic_mutation_fuzz_is_bounded_and_never_panics() {
    let mut rng = 0x12345678u32;
    for _ in 0..10000 {
        let mut b = INPUT.to_vec();
        for _ in 0..4 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let i = rng as usize % b.len();
            b[i] ^= (rng >> 24) as u8;
        }
        if let Ok(mut d) = Decoder::new(b.as_slice()) {
            for _ in 0..4 {
                match d.next_block() {
                    Ok(Some(_)) => {}
                    _ => break,
                }
            }
        }
    }
}
