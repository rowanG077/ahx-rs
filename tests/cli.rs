//! WAV extraction and output preservation.
#![cfg(feature = "std")]

use std::{fs, process::Command};

#[test]
fn wav_extraction_matches_canonical_and_never_overwrites() {
    let root = std::env::temp_dir().join(format!("ahx-cli-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("input.ahx");
    let output = root.join("output.wav");
    let bytes = include_bytes!("fixtures/continuity.ahx");
    fs::write(&input, bytes).unwrap();
    let run = |out: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_ahx"))
            .arg(&input)
            .arg(out)
            .output()
            .unwrap()
    };
    assert!(run(&output).status.success());
    let wav = fs::read(&output).unwrap();
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 32000);
    assert_eq!(&wav[44..], include_bytes!("fixtures/continuity.pcm"));
    assert!(!run(&output).status.success());
    assert_eq!(fs::read(&output).unwrap(), wav);
    assert!(!run(&input).status.success());
    assert_eq!(fs::read(&input).unwrap(), bytes);
    fs::remove_dir_all(&root).unwrap();
}
