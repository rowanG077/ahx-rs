#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(mut decoder) = ahx::SliceDecoder::new(data) {
        for _ in 0..32 {
            match decoder.next_block() {
                Ok(Some(_)) => {}
                _ => break,
            }
        }
    }
    let mut decoder = ahx::PacketDecoder::new();
    let mut pcm = [0x1234; ahx::SAMPLES_PER_FRAME];
    let before = pcm;
    if decoder.decode_into(data, &mut pcm).is_err() {
        assert_eq!(pcm, before);
    }
    let _ = ahx::Metadata::required_header_len(data);
});
