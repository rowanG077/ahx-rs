# Synthetic reference fixture

`continuity.ahx` contains two synthetic AHX frames (2,304 mono samples), with a
32-byte unencrypted type `0x10` header declaring 32,000 Hz. The second frame
exercises synthesis history from the first. It contains no original game audio.

`continuity.pcm` is the corresponding raw signed PCM16 in little-endian order,
produced by the pinned aarch64 vgmstream r2117 / mpg123 1.33.7 NEON64 float
reference. It includes the startup samples; no resampling or delay trimming
was applied. Both fixtures may be redistributed under this crate's license.

The tests also derive shorter final durations, alternative declared rates,
maximum-length headers, truncated inputs, and malformed data from this fixture.
