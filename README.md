# ahx

Safe Rust decoding for unencrypted mono CRI AHX type `0x10` (version 6). The
library supports declared 32,000, 44,100 and 48,000 Hz rates, fixed `FFF5E0C0`
MPEG headers, and the CRI end marker. Encryption, stereo, ADX and type `0x11`
are explicitly unsupported. PCM16 matches the pinned aarch64 vgmstream/mpg123
reference, including startup synthesis samples and declared-duration trimming.
No resampling is performed.

## Features and storage

| Feature | APIs | Allocation |
| --- | --- | --- |
| None (`default-features = false`) | `Metadata`, `PacketDecoder`, `SliceDecoder` | No `std` or `alloc`; fixed arrays or caller-owned PCM |
| `std` (default) | Also `Decoder<R: Read>` and WAV CLI | Decoder still uses no heap allocation |

The synthesis state and default PCM array use less than 10 KiB per decoder.
Packet validation additionally uses a bounded 4.5 KiB subband array on the stack.
Memory is independent of stream duration. Caller input, I/O buffering and output
handling are outside these guarantees. There is no `alloc` feature because none
of the decoding APIs require an allocator.

The `no_std` synthesis path uses the pure Rust `libm` crate with its architecture
and assembly paths disabled. The `std` path uses `f32::mul_add`. Both preserve
single-rounding fused arithmetic; ordinary multiplication followed by addition
can change the final PCM16. There is no C, FFI, or native decoding dependency.

## Decode a byte slice without an allocator

```toml
ahx = { path = "/path/to/ahx-rs", default-features = false }
```

```rust
use ahx::{Error, SliceDecoder};

fn decode(input: &[u8]) -> Result<(), Error> {
    let mut decoder = SliceDecoder::new(input)?;
    let metadata = decoder.metadata();
    while let Some(pcm) = decoder.next_block()? {
        // Consume borrowed mono PCM16 before requesting the next block.
    }
    Ok(())
}
```

`Metadata::parse(input)?` validates just the complete AHX header, without decoding
or allocating. Use its `sample_rate()`, `samples()`, `channels()` and `header_len()`
accessors. Headers are variable length. For incremental input,
`Metadata::required_header_len(prefix)?` examines the first `Metadata::PREFIX_LEN`
bytes and reports how many bytes must be collected before calling `parse`.
This sizing query validates the fixed prefix, not the complete header.

Default decoders own their PCM array, so no caller sizing is necessary. To use
an arena or reusable buffer, provide **`SAMPLES_PER_FRAME` (1,152) `i16` elements**:

```rust
use ahx::{Error, SliceDecoder, SAMPLES_PER_FRAME};

fn decode(input: &[u8]) -> Result<(), Error> {
    let mut pcm = [0i16; SAMPLES_PER_FRAME];
    let mut decoder = SliceDecoder::with_buffer(input, &mut pcm[..])?;
    while let Some(samples) = decoder.next_block()? {
        // Only this prefix contains the declared output for the current block.
    }
    Ok(())
}
```

Buffer requirements are fixed and need no limits argument. The full frame buffer
is required even if the declared final output is shorter. Insufficient storage
returns `Error::BufferTooSmall { required, provided }` in `i16` elements before
output is modified. Extra storage beyond the full-frame prefix is unchanged;
samples past a trimmed final block may contain synthesized data and are not output.
Borrow a slice to retain it on constructor failure. `into_inner()` recovers the
unconsumed input and PCM storage. Custom `AsMut<[i16]>` storage is rechecked on each
block; a view shorter than 1,152 elements returns an error and invalidates the decoder.

## Decode packets or stream standard I/O

`PacketDecoder::new()` owns only synthesis history. Call
`decode_into(input, pcm)?` with a complete AHX MPEG frame prefix. The result exposes
`pcm()` as `&[i16; SAMPLES_PER_FRAME]` and `consumed_bytes()`; the latter includes
the four-byte MPEG header and the final padded byte. Frames are variable length,
bounded by `MAX_FRAME_BYTES`.
Following frames and container data remain unconsumed. Continue with the same
decoder to preserve synthesis history.

All packet validation precedes synthesis. Errors preserve output and history, so
`Truncated` can be retried after appending input, or `BufferTooSmall` after supplying
larger storage. This layer does not parse AHX headers or footers, apply sample-rate
metadata, or trim output. Complete-file decoders handle those responsibilities.

```rust,no_run
# #[cfg(feature = "std")]
# fn example() -> Result<(), Box<dyn std::error::Error>> {
use std::{fs::File, io::BufReader};
let mut decoder = ahx::Decoder::new(BufReader::new(File::open("voice.ahx")?))?;
while let Some(pcm) = decoder.next_block()? {
    // Consume the borrowed mono samples.
}
# Ok(())
# }
```

`Metadata::read(&mut reader)?` reads only the header. Neither it nor the decoder
requires seeking. Slice and standard-I/O decoders validate the end marker before
returning the final trimmed block and leave bytes after it unread. Call through
`None` to validate the whole stream. Successful EOF is repeatable. A container
decode error permanently invalidates the instance because input may have been
consumed; subsequent calls return `Error::Failed`. Reconstruct it to restart.
If a panic in caller I/O or storage is caught, the decoder also remains failed.
I/O unexpected EOF maps to `Error::Truncated`; other standard I/O errors retain
their source through `Error::Io`.

Use `state()` on either complete-file decoder to inspect progress:

- `DecoderState::Decoding { remaining_samples }` carries a `NonZeroU32` count.
- `DecoderState::Finished` means the final block and end marker were validated.
- `DecoderState::Failed` means further decoding is unavailable.

The last successful `next_block()` sets `Finished` before returning its PCM.
Later calls return `None` without touching input or caller storage. Failures likewise
remain terminal. `Error::Invalid(InvalidData)` and `Error::Unsupported(UnsupportedData)`
provide matchable reasons, including rejected sample counts and rates; error
handling never requires comparing message strings.

Internally, parsing consumes typed allocation and scale-factor stages before a
validated frame can access synthesis history. Silent bands have no quantizer or
scale factors. Grouped and ungrouped quantizers are separate variants with valid
bit widths, and synthesis accepts fixed-size output arrays.

## Extract, develop and release

```sh
cargo run --release -- voice.ahx voice.wav
cargo test --locked
cargo test --locked --no-default-features
cargo test --locked --release
cargo fmt --all -- --check
cargo fmt --manifest-path fuzz/Cargo.toml -- --check
cargo fmt --manifest-path tests/no-std/Cargo.toml -- --check
cargo tree --edges normal --no-default-features
```

The CLI writes PCM16 WAV at the declared rate and refuses to overwrite input or
existing output. Tests use redistributable synthetic AHX and canonical PCM,
covering continuity, final trimming, truncations, malformed packets, retry and
buffer contracts. The bounded `fuzz/` target exercises allocation-free APIs.
A separate host static library consumer links without a global allocator.
No original game assets are distributed. The sibling Resonance repository's
`tools/decoder-reference` compares every local corpus payload with pinned outputs.

`nix develop` supplies Rust and formatting tools. CI checks Linux x86_64/aarch64
and macOS aarch64, default and allocation-free builds, debug/release reference
fixtures, package contents, and the release automation. See
[RELEASING.md](RELEASING.md) for the tag → release PR → validated publication flow.

## License and provenance

LGPL-2.1-or-later; see [COPYING](./COPYING), [NOTICE](./NOTICE),
[AUTHORS.mpg123](AUTHORS.mpg123), and [LICENSE.vgmstream](LICENSE.vgmstream).
The narrow Layer II backend derives from mpg123 1.33.7, preserving the pinned
NEON64 float synthesis operation order and constants in safe Rust. Generated
tables and DCT code are committed; their offline maintenance script is not a
build step. `libm` is MIT-licensed and has no production dependencies.

Symphonia 0.5.5 MP2 was evaluated with matching framing and PCM conversion, but
one 50,886-sample clip differed at 32 PCM16 values. This backend preserves the
required exact output instead. The canonical comparison configuration and input
hashes are retained by Resonance's reference tooling.
