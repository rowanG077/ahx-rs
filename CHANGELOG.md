# Changelog

## Unreleased

- Decode unencrypted mono CRI AHX type `0x10` to PCM16 at declared 32,000,
  44,100 and 48,000 Hz rates, preserving startup synthesis samples and trimming
  the final frame to the declared duration.
- Provide allocation-free packet and slice decoders without `std`, plus a
  standard-I/O streaming decoder. Use fixed output arrays or caller-owned PCM
  storage with explicit size and ownership contracts.
- Expose validated metadata, variable-header sizing and exact packet consumption.
  Packet results carry their complete PCM length in the type. Packet errors
  preserve output and synthesis history for retry; container errors invalidate
  the reader. Expose explicit decoding, finished and failed states, and typed
  reasons for unsupported profiles, malformed data and truncation.
- Add a PCM16 WAV extraction CLI that preserves the declared rate and refuses
  to overwrite existing files.
- Include redistributable canonical fixtures, malformed-input and API contract
  tests, bounded fuzzing, and checks across feature sets, profiles and platforms.
- Add tag-triggered release preparation, a reviewed release PR with validated
  package artifacts, and publication after merge, matching the H4M release flow.
