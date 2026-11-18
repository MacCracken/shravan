# Development Roadmap

**Current: v2.12.5.** Shipped history lives in `CHANGELOG.md`; this file holds where shravan stands and
what is left.

## Where we are

| Codec | Decode | Encode | Checked against |
|-------|--------|--------|-----------------|
| WAV | PCM 1–32-bit (whole-byte containers), float 32/64, EXTENSIBLE, piped (unknown sizes) | PCM 8/16/24/32, float 32/64 | ffmpeg, exact, one-shot and streamed |
| AIFF | PCM 1–32-bit; AIFF-C NONE/twos/sowt/fl32/fl64 | PCM | ffmpeg, exact, one-shot and streamed |
| FLAC | all subframe types to 32-bit, seek-table range decode, MD5 verify | levels 0–8, as `flac -0` to `-8` | `flac`, exact; the encoder within 0.06% of `flac -L`'s size, its output decoded exactly by libFLAC and ffmpeg |
| ALAC | full, in MP4 and QuickTime | none | ffmpeg, bit-exact |
| MP3 | MPEG-1/2/2.5 Layer I/II/III, free format, gapless from the LAME tag | none | ISO 11172-4 / 13818-4 conformance vectors within 1 LSB; ffmpeg exact on Layer III; mpg123 on Layer II |
| AAC-LC | any channel layout (configurations 1–7 and 11–14, a PCE), all 13 rates, every LC tool | mono and stereo, all 13 rates: 3GPP psychoacoustics, block switching, TNS, M/S, intensity, PNS, a rate loop with a reservoir | Decode: ffmpeg, exact. Encode: ffmpeg and faad decode it, within 0.02 of ffmpeg's own encoder in correlation |
| Opus | every TOC configuration; FEC, PLC, DTX, every mode, bandwidth, frame-size and channel switch; multistream (families 0, 1, 2, 255) | SILK (mono and stereo, 8–16 kHz internal from 8–48 kHz input, complexity 0–10, FEC, DTX); CELT (libopus's `celt_encode_with_ec`: every frame size, rate, complexity, rate control and stereo mode, the hybrid's start band); the Opus layer for `OPUS_APPLICATION_RESTRICTED_SILK` | libopus: decode exact (CELT concealment within 3 LSB); encode: `celt_encode_with_ec` byte for byte (in single precision, as libopus computes it); `silk_Encode` and `opus_encode_float` byte for byte on the suite's inputs (SILK's float stages in double precision, so hostile input can part at a near-tie); libopus's portable C build throughout |
| MP4 | AAC and ALAC; any audio track; gapless (sample durations, edit lists, iTunSMPB) | none | ffmpeg, to the sample |

Streaming decoders (WAV, AIFF, FLAC, and `decode_reader` over them) match the one-shot decoders sample
for sample however the input is split, and have no length ceiling. One-shot decodes stop at
`shravan_set_max_samples` (2^26 interleaved samples by default) with `ERR_TOO_LARGE`. Real encoder
output with reference decodes is part of the suite (`src/interop_fixtures.cyr`), and
`scripts/interop.sh` sweeps every codec against ffmpeg, flac, mpg123 and libopus in CI.

### Paths no reference has reached

Open until a reference file or decoder exists for each.
- **MPEG-2.5 8 kHz mixed blocks.** ffmpeg and minimp3 disagree on the band layout and no conformance
  vector covers it, so shravan's layout is unchanged and unverified.
- **AAC `pulse_data`** is refused (`ERR_UNSUPPORTED_FMT`): no available encoder writes it.
- **ALAC from Apple's encoder** and **FLAC with a variable block size**: no tool here produces them.
  ffmpeg's ALAC and `flac`'s fixed-size output are verified.
- **ID3v2 grouped frames** are read per the spec; neither ffmpeg nor mutagen reads them at all.
- **AAC coupling channel elements** are refused (`ERR_UNSUPPORTED_FMT`): no available encoder writes them.
- **AAC 22.2 and channel configurations 11, 13 and 14**: ffmpeg's encoder writes none of them. Their
  element placement and channel order are checked against ffmpeg's source (`ff_aac_get_che`,
  `sniff_channel_order`), not against a file.

## Next — v2.12.x: hybrid encode and the mode decision · large

What stands between shravan and **3.0.0** is the rest of the Opus encoder.
- `feat(opus): hybrid encode and the mode decision`:
  - SILK's low band and CELT's high band over one range coder: the hybrid's SILK rate split, CELT
    from band 17 with `celt_encoder_set_silk_info` and `celt_encoder_encode_ec`, and the stereo width
    CELT takes from `silk_encoder_stereo_width`.
  - The mode decision by rate, signal and bandwidth (SILK, hybrid, CELT, with hysteresis).
  - Switches between modes: redundancy frames, `silk_Encode`'s prefill, CELT's pre-roll.
  - The applications that need them: `OPUS_APPLICATION_VOIP` (with its variable high-pass,
    `hp_cutoff`, reading the smoother `silk_hp_variable_cutoff` keeps), `_AUDIO`,
    `_RESTRICTED_LOWDELAY` and `_RESTRICTED_CELT`. `opus_encoder_new` refuses them until then.
- The tonality analysis (`run_analysis`), which libopus runs from complexity 7 for those
  applications. Its voice estimate and bandwidth detection feed the mode and bandwidth decisions, and
  its `AnalysisInfo` feeds the CELT encoder (`celt_encoder_set_analysis` takes it already).

Verify it the way the SILK and CELT encoders were verified:
- `scripts/gen-silk-vectors.py` and `scripts/gen-celt-vectors.py` record the instrumented libopus
  (`scripts/silkref/`, its dump hooks).
- The suite holds shravan to libopus's packets, with each stage's output stored as a hash.
- The suite's string data is at 94% of the compiler's fixed 2 MB, so new vectors must be compact.

Note on rounding: the float→fixed boundaries use `lrintf` (round half to **even**), which f64 code must
reproduce deliberately. The only SILK packets that differ from libopus's are near-ties that its single
precision rounds the other way. CELT's port computes in single precision itself, so it has no such
ties.

## v3.0.0 — production-quality, interoperable, hardened suite · MAJOR

The major bump: every codec value-verified end to end in both directions, fully interoperable, safe on
hostile input, no known correctness bugs.

**Gate:**
- Opus encodes *and* decodes every configuration, interop-verified with libopus both ways. Still open:
  hybrid, the mode decision and the applications above.
- Zero known correctness bugs.
- Met, and kept as regression checks in CI:
  - the FLAC LPC encoder;
  - every decoder fuzzed to 0 crashes (`./fuzz/run.sh 90000`, with its cold starts and poison mode);
  - the reference comparison (the `interop` job);
  - benchmarks in the CSV history.

**Backlog:** items that need the major bump.
- Remove the `detect_format` wrapper. `shravan_detect_format` replaced it in 2.9.2 and no consumer calls
  it, but it is public API, and removing it is what clears the last duplicate-function warning.
  `scripts/check-collisions.sh` allows it by name until then.

## Post-3.0

- `feat: DSD64/128/256 + DoP (1-bit sigma-delta path)`: new format, additive.
- `feat(aac): HE-AAC (SBR, PS)`. MP4 reports these as unsupported rather than playing the core at half
  rate.

## Working discipline (unchanged)

Every change lands as a small verified bite:
- The build is clean and all assertions pass.
- New code comes with new tests and benchmarks.
- Decode stays bit-exact through any shared-code refactor.

A round-trip through shravan's own encoder proves nothing. Judge decoders by maximum sample error
against a reference, per frame, on real encoder output. Judge encoders by what that reference does
with their output. Never claim a stage done until it is value-verified against something other than
shravan. Prove every fix by reverting it and watching a test fail.
