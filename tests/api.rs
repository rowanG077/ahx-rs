//! Public storage, parsing, ownership and error contracts.

use ahx::{
    DecoderState, Error, InvalidData, Metadata, PacketDecoder, SliceDecoder, UnsupportedData,
    MAX_FRAME_BYTES, SAMPLES_PER_FRAME,
};

const INPUT: &[u8] = include_bytes!("fixtures/continuity.ahx");
const GOLDEN: &[u8] = include_bytes!("fixtures/continuity.pcm");

fn expected() -> Vec<i16> {
    GOLDEN
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]))
        .collect()
}

#[test]
fn header_preflight_and_full_validation_are_distinct() {
    let metadata = Metadata::parse(INPUT).unwrap();
    assert_eq!(Metadata::required_header_len(&INPUT[..20]).unwrap(), 32);
    assert_eq!(metadata.header_len(), 32);
    assert_eq!(metadata.channels(), 1);
    assert_eq!(metadata.samples(), 2304);
    assert_eq!(metadata.sample_rate(), 32000);
    for end in 0..metadata.header_len() {
        assert!(matches!(
            Metadata::parse(&INPUT[..end]),
            Err(Error::Truncated)
        ));
    }
    assert_eq!(Metadata::parse(&INPUT[..32]).unwrap(), metadata);
    let mut corrupt = INPUT.to_vec();
    corrupt[31] = 0;
    assert_eq!(Metadata::required_header_len(&corrupt).unwrap(), 32);
    assert!(matches!(Metadata::parse(&corrupt), Err(Error::Invalid(_))));
    corrupt[4] = 0x11;
    assert!(matches!(
        Metadata::parse(&corrupt),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn longest_header_is_bounded_and_preserves_payload() {
    let mut input = INPUT[..20].to_vec();
    input[2..4].copy_from_slice(&u16::MAX.to_be_bytes());
    input.resize(65539 - 6, 0);
    input.extend_from_slice(b"(c)CRI");
    input.extend_from_slice(&INPUT[32..]);
    let mut d = SliceDecoder::new(&input).unwrap();
    assert_eq!(d.metadata().header_len(), 65539);
    let mut pcm = Vec::new();
    while let Some(block) = d.next_block().unwrap() {
        pcm.extend_from_slice(block);
    }
    assert_eq!(pcm, expected());
}

#[test]
fn exact_storage_and_default_storage_produce_identical_pcm() {
    let mut pcm = [0x1234; SAMPLES_PER_FRAME + 7];
    let mut small = [0x2345; SAMPLES_PER_FRAME - 1];
    let error = SliceDecoder::with_buffer(INPUT, &mut small[..]).unwrap_err();
    assert!(matches!(
        error,
        Error::BufferTooSmall { required: SAMPLES_PER_FRAME, provided }
            if provided == SAMPLES_PER_FRAME - 1
    ));
    assert!(small.iter().all(|&s| s == 0x2345));
    let mut input = INPUT.to_vec();
    input.extend_from_slice(b"trailing");
    let mut borrowed = SliceDecoder::with_buffer(&input, &mut pcm[..]).unwrap();
    let mut owned = SliceDecoder::new(INPUT).unwrap();
    while let Some(block) = owned.next_block().unwrap() {
        assert_eq!(borrowed.next_block().unwrap().unwrap(), block);
    }
    assert!(borrowed.next_block().unwrap().is_none());
    assert!(borrowed.next_block().unwrap().is_none());
    let (rest, storage) = borrowed.into_inner();
    assert_eq!(rest, b"trailing");
    assert_eq!(&storage[SAMPLES_PER_FRAME..], &[0x1234; 7]);
}

#[test]
fn packets_report_exact_consumption_and_preserve_history() {
    let mut d = PacketDecoder::new();
    let mut pcm = [0; SAMPLES_PER_FRAME];
    let mut rest = &INPUT[Metadata::parse(INPUT).unwrap().header_len()..];
    let mut output = Vec::new();
    for _ in 0..2 {
        let packet = d.decode_into(rest, &mut pcm).unwrap();
        let _: &[i16; SAMPLES_PER_FRAME] = packet.pcm();
        assert!(packet.consumed_bytes() <= MAX_FRAME_BYTES);
        output.extend_from_slice(packet.pcm());
        rest = &rest[packet.consumed_bytes()..];
    }
    assert_eq!(output, expected());
    assert_eq!(rest, b"\0\x80\x01\0\x0cAHXE(c)CRI\0\0");
}

#[test]
fn every_truncated_packet_and_short_output_can_be_retried() {
    let data = &INPUT[32..];
    let mut pcm = [0; SAMPLES_PER_FRAME];
    let first = PacketDecoder::new()
        .decode_into(data, &mut pcm)
        .unwrap()
        .consumed_bytes();
    let first_expected = pcm;
    for end in 0..first {
        let mut decoder = PacketDecoder::new();
        let mut output = [0x3456; SAMPLES_PER_FRAME + 1];
        assert!(
            matches!(
                decoder.decode_into(&data[..end], &mut output),
                Err(Error::Truncated)
            ),
            "prefix {end}"
        );
        assert!(output.iter().all(|&v| v == 0x3456));
        assert_eq!(
            decoder.decode_into(data, &mut output).unwrap().pcm(),
            &first_expected
        );
        assert_eq!(output[SAMPLES_PER_FRAME], 0x3456);
        let before = output;
        assert!(matches!(
            decoder.decode_into(&data[first..], &mut output[..SAMPLES_PER_FRAME - 1]),
            Err(Error::BufferTooSmall { .. })
        ));
        assert_eq!(output, before);
        assert_eq!(
            decoder
                .decode_into(&data[first..], &mut output)
                .unwrap()
                .pcm(),
            &expected()[SAMPLES_PER_FRAME..]
        );
    }
}

#[test]
fn invalid_quantizers_preserve_synthesis_and_output() {
    let mut decoder = PacketDecoder::new();
    let mut pcm = [0; SAMPLES_PER_FRAME];
    let first = decoder
        .decode_into(&INPUT[32..], &mut pcm)
        .unwrap()
        .consumed_bytes();
    // Allocation 1 in subband 0 (5-bit grouped radix-3), SCFSI 2 and scale 0.
    // The first quantizer is 31, outside the 27 valid grouped values.
    let mut bad = [0u8; MAX_FRAME_BYTES];
    bad[..4].copy_from_slice(&[0xff, 0xf5, 0xe0, 0xc0]);
    bad[4] = 0x10;
    fn set(bits: &mut [u8], start: usize, width: usize, value: u32) {
        for i in 0..width {
            bits[(start + i) / 8] |=
                (((value >> (width - 1 - i)) & 1) as u8) << (7 - (start + i) % 8);
        }
    }
    let allocation_end = 32 + 4 * 4 + 7 * 3 + 19 * 2;
    set(&mut bad, allocation_end, 2, 2);
    set(&mut bad, allocation_end + 2 + 6, 5, 31);
    let before = pcm;
    assert!(matches!(
        decoder.decode_into(&bad, &mut pcm),
        Err(Error::Invalid(InvalidData::GroupedQuantizer))
    ));
    assert_eq!(pcm, before);
    assert_eq!(
        decoder
            .decode_into(&INPUT[32 + first..], &mut pcm)
            .unwrap()
            .pcm(),
        &expected()[SAMPLES_PER_FRAME..]
    );
}

#[test]
fn footer_errors_are_reported_before_the_final_block_and_poison_the_reader() {
    let mut input = INPUT.to_vec();
    *input.last_mut().unwrap() = 1;
    let mut decoder = SliceDecoder::new(&input).unwrap();
    assert_eq!(
        decoder.next_block().unwrap().unwrap().len(),
        SAMPLES_PER_FRAME
    );
    assert!(matches!(
        decoder.next_block(),
        Err(Error::Invalid(InvalidData::EndMarker))
    ));
    assert!(matches!(decoder.next_block(), Err(Error::Failed)));
    assert_eq!(decoder.state(), DecoderState::Failed);
    let mut short = INPUT.to_vec();
    short[12..16].copy_from_slice(&1152u32.to_be_bytes());
    assert!(SliceDecoder::new(&short).unwrap().next_block().is_err());
}

#[test]
fn changing_storage_views_return_an_error_without_panicking() {
    struct Shrinking {
        pcm: [i16; SAMPLES_PER_FRAME],
        calls: usize,
    }
    impl AsMut<[i16]> for Shrinking {
        fn as_mut(&mut self) -> &mut [i16] {
            self.calls += 1;
            if self.calls == 1 {
                &mut self.pcm
            } else {
                &mut self.pcm[..0]
            }
        }
    }
    let mut decoder = SliceDecoder::with_buffer(
        INPUT,
        Shrinking {
            pcm: [0; SAMPLES_PER_FRAME],
            calls: 0,
        },
    )
    .unwrap();
    assert!(matches!(
        decoder.next_block(),
        Err(Error::BufferTooSmall { provided: 0, .. })
    ));
    assert!(matches!(decoder.next_block(), Err(Error::Failed)));
}

#[test]
fn state_fits_in_fixed_storage() {
    assert!(core::mem::size_of::<SliceDecoder<'_>>() < 10 * 1024);
    assert!(core::mem::size_of::<PacketDecoder>() < 7 * 1024);
}

#[test]
fn progress_reaches_finished_only_after_the_trimmed_final_block_and_footer() {
    use core::num::NonZeroU32;

    let mut input = INPUT.to_vec();
    input[12..16].copy_from_slice(&1201u32.to_be_bytes());
    let mut decoder = SliceDecoder::new(&input).unwrap();
    assert_eq!(
        decoder.state(),
        DecoderState::Decoding {
            remaining_samples: NonZeroU32::new(1201).unwrap(),
        }
    );
    assert_eq!(decoder.next_block().unwrap().unwrap().len(), 1152);
    assert_eq!(
        decoder.state(),
        DecoderState::Decoding {
            remaining_samples: NonZeroU32::new(49).unwrap(),
        }
    );
    assert_eq!(decoder.next_block().unwrap().unwrap().len(), 49);
    assert_eq!(decoder.state(), DecoderState::Finished);
    assert!(decoder.next_block().unwrap().is_none());
    assert_eq!(decoder.state(), DecoderState::Finished);
}

#[test]
fn typed_header_errors_retain_invalid_values() {
    let mut input = INPUT.to_vec();
    for rate in [0u32, 8000, u32::MAX] {
        input[8..12].copy_from_slice(&rate.to_be_bytes());
        assert!(matches!(
            Metadata::parse(&input),
            Err(Error::Unsupported(UnsupportedData::SampleRate(actual))) if actual == rate
        ));
    }
    input[8..12].copy_from_slice(&32000u32.to_be_bytes());
    for count in [0u32, i32::MAX as u32 + 1, u32::MAX] {
        input[12..16].copy_from_slice(&count.to_be_bytes());
        assert!(matches!(
            Metadata::parse(&input),
            Err(Error::Invalid(InvalidData::SampleCount(actual))) if actual == count
        ));
    }
}

#[test]
fn terminal_states_never_access_caller_storage_again() {
    struct Counted {
        pcm: [i16; SAMPLES_PER_FRAME],
        calls: usize,
    }
    impl AsMut<[i16]> for Counted {
        fn as_mut(&mut self) -> &mut [i16] {
            self.calls += 1;
            &mut self.pcm
        }
    }
    for valid in [false, true] {
        let mut input = INPUT.to_vec();
        if !valid {
            *input.last_mut().unwrap() = 1;
        }
        let mut decoder = SliceDecoder::with_buffer(
            &input,
            Counted {
                pcm: [0; SAMPLES_PER_FRAME],
                calls: 0,
            },
        )
        .unwrap();
        decoder.next_block().unwrap();
        assert_eq!(decoder.next_block().is_ok(), valid);
        for _ in 0..3 {
            if valid {
                assert!(decoder.next_block().unwrap().is_none());
            } else {
                assert!(matches!(decoder.next_block(), Err(Error::Failed)));
            }
        }
        assert_eq!(decoder.into_inner().1.calls, 3);
    }
}

#[test]
fn caught_storage_panics_leave_the_decoder_failed() {
    struct Panicking {
        pcm: [i16; SAMPLES_PER_FRAME],
        accessed: bool,
    }
    impl AsMut<[i16]> for Panicking {
        fn as_mut(&mut self) -> &mut [i16] {
            // Constructor sizing succeeds. Access at the first block panics.
            if core::mem::replace(&mut self.accessed, true) {
                panic!("storage panic");
            }
            &mut self.pcm
        }
    }
    let mut decoder = SliceDecoder::with_buffer(
        INPUT,
        Panicking {
            pcm: [0; SAMPLES_PER_FRAME],
            accessed: false,
        },
    )
    .unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = decoder.next_block();
    }))
    .is_err());
    assert_eq!(decoder.state(), DecoderState::Failed);
    assert!(matches!(decoder.next_block(), Err(Error::Failed)));
}

#[cfg(feature = "std")]
#[test]
fn caught_io_panics_leave_the_decoder_failed() {
    use std::io::{self, Read};

    struct Panicking<'a>(&'a [u8]);
    impl Read for Panicking<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.0.len() <= INPUT.len() - 32 {
                panic!("reader panic");
            }
            self.0.read(out)
        }
    }
    let mut decoder = ahx::Decoder::new(Panicking(INPUT)).unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = decoder.next_block();
    }))
    .is_err());
    assert_eq!(decoder.state(), DecoderState::Failed);
    assert!(matches!(decoder.next_block(), Err(Error::Failed)));
}

#[cfg(feature = "std")]
#[test]
fn standard_io_fragmentation_errors_and_position_match_the_slice_api() {
    use std::io::{self, Read};
    struct Fragmented<'a>(&'a [u8], bool);
    impl Read for Fragmented<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.1 = !self.1;
            if self.1 {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let len = out.len().min(3);
            self.0.read(&mut out[..len])
        }
    }
    let mut input = INPUT.to_vec();
    input.extend_from_slice(b"trailing");
    let mut decoder = ahx::Decoder::new(Fragmented(&input, false)).unwrap();
    let mut output = Vec::new();
    while let Some(pcm) = decoder.next_block().unwrap() {
        output.extend_from_slice(pcm);
    }
    assert_eq!(output, expected());
    assert_eq!(decoder.state(), DecoderState::Finished);
    assert!(decoder.next_block().unwrap().is_none());
    assert_eq!(decoder.into_inner().0, b"trailing");
    let mut cursor = io::Cursor::new(INPUT);
    let info = Metadata::read(&mut cursor).unwrap();
    assert_eq!(cursor.position(), info.header_len() as u64);
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
    let error = Metadata::read(&mut Broken).unwrap_err();
    assert!(matches!(&error, Error::Io(e) if e.kind() == io::ErrorKind::PermissionDenied));
    assert!(std::error::Error::source(&error).is_some());
    assert!(matches!(
        Metadata::read(&mut &INPUT[..4]),
        Err(Error::Truncated)
    ));
}
