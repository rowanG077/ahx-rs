//! Extract AHX audio to a PCM16 WAV file.

use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    process::ExitCode,
};

const USAGE: &str = "Usage: ahx INPUT.ahx OUTPUT.wav\nExtract mono PCM16 audio at the declared sample rate. Existing files are never overwritten.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ahx: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = args.next().ok_or(USAGE)?;
    if input == "--help" || input == "-h" {
        println!("{USAGE}");
        return Ok(());
    }
    let output = args.next().ok_or(USAGE)?;
    if args.next().is_some() {
        return Err(USAGE.into());
    }

    let mut decoder = ahx_rs::Decoder::new(BufReader::new(File::open(input)?))?;
    let metadata = decoder.metadata();
    let riff_size = metadata
        .samples()
        .checked_mul(2)
        .and_then(|size| size.checked_add(36))
        .ok_or("declared audio is too large for PCM16 WAV")?;
    let mut writer = BufWriter::new(File::create_new(output)?);
    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_size.to_le_bytes())?;
    writer.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x01\0")?;
    writer.write_all(&metadata.sample_rate().to_le_bytes())?;
    writer.write_all(&(metadata.sample_rate() * 2).to_le_bytes())?;
    writer.write_all(b"\x02\0\x10\0data")?;
    writer.write_all(&(riff_size - 36).to_le_bytes())?;
    while let Some(pcm) = decoder.next_block()? {
        for sample in pcm {
            writer.write_all(&sample.to_le_bytes())?;
        }
    }
    writer.flush()?;
    Ok(())
}
