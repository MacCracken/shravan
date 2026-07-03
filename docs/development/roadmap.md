# Development Roadmap

**Current: v2.6.5.** Shipped history lives in `CHANGELOG.md`; this file is forward-looking only.

## Where we are

- **Decode is real and complete for the mainstream Opus surface.** shravan decodes actual
  libopus `.opus` — CELT + SILK + hybrid, mono **and** stereo, 10 ms **and** 20 ms, all
  bandwidths — bit-exact vs libopus (correlation ~1.0 vs ffmpeg). Plus WAV, AIFF, FLAC (enc+dec),
  Ogg, MP3 (MPEG-1/2/2.5 Layer I/II/III), AAC-LC, MP4/M4A demux, ALAC (decoder present).
- **Encode is real for CELT (the hard half started).** shravan encodes a real RFC-6716 **CELT**
  frame (mono, 20 ms, non-transient) that **libopus decodes sample-identical** to shravan's own
  decoder (correlation 1.000000). The whole chain — forward MDCT → band energy → coarse/fine
  energy → allocation → PVQ search+encode → range coder — is the verified inverse of the decoder.
- 11460 assertions, 0 failing. Cyrius toolchain pinned at 6.3.27.

The remaining distance is (1) finishing the Opus encoder, (2) closing the small decode gaps, and
(3) bringing the non-Opus codecs and the whole suite to production/interop quality. That is the
path to **3.0.0**, below.

## Path to 3.0.0

Two patches finish the CELT encoder; three minors then complete Opus encode, mature the rest of
the suite, and harden everything; **3.0.0** is the major bump when the suite is complete, fully
interoperable both directions, and fuzz-clean.

### v2.6.6 — CELT encoder quality · medium
Make the CELT encoder produce *good* frames, not just valid ones (2.6.5 uses fixed defaults).
- ✅ spread decision, ✅ 2-pass coarse-energy intra/inter size race, ✅ transient *encode*
  (short blocks), ✅ **transient *detection*** (`celt_transient_analysis`, `celt_encoder.c:267`
  float path, `inv_table` byte-exact; `celt_encode` auto-detects `isTransient` via the −1
  sentinel — steady tone → long, onset → short; adversarially verified faithful to libopus).
- ✅ **tf_analysis** (`celt_tf_analysis`, `celt_encoder.c:663` float path): per-band `tf_res`
  via L1-metric Haar-level search + dual Viterbi (`tf_select` + backtrace), `importance=13`
  uniform (dynalloc-off fallback). Replaces the all-zeros `tf_res` in `celt_encode_frame_ec`;
  round-trips through `celt_tf_encode`/`celt_tf_decode` (encoder remap == decoder remap, same
  bits) and *improved* every spectrum correlation (frame 0.968→0.990, transient 0.898→0.943).
- ✅ **dynalloc boosts** (`celt_dynalloc_analysis`, `celt_encoder.c:1049` float path): a masking
  follower (forward/backward + median-of-5 filter, noise-floor bounded) → per-band `offsets` (bit
  units = boost_count·quanta, so the R9 boost loop reproduces them exactly) + the real `importance[]`
  that now feeds tf_analysis. Mono; stereo keeps the flat fallback. Flat spectrum → no boosts /
  importance 13; a spectral peak → boost + higher importance (verified exact: peak offset 384,
  importance 208); pcm round-trip 0.997→0.999. **v2.6.6 is feature-complete — ready to cut.**

### v2.6.7 — CELT stereo encode · medium
- `feat(opus): stereo_split / intensity_stereo, quant_band_stereo N==2 sign encode, dual-stereo
  decision` → CELT encode covers mono **and** stereo, all bandwidths, verified vs libopus.

### v2.7.0 — Opus decode completeness · small–medium (close the last decode gaps)
The mainstream surface decodes; finish the long tail so *every* Opus config decodes.
- `feat(silk): MB (12 kHz, config 4–7) decode; 40 ms / 60 ms frame sizes (multi-subframe loop)`.
- `feat(opus): CELT 2.5 ms / 5 ms (LM=0/1) decode — parameterize the remaining LM=2/3 assumptions`.
- Redundant-frame (CELT↔SILK transition) audio handling in the hybrid path.

### v2.8.0 — Opus SILK + hybrid ENCODE · large (full Opus encode)
The big lift: the encoder counterpart of 2.6.1–2.6.3. Expect several patches (2.8.1…) —
pitch/LTP analysis and NLSF VQ are each substantial.
- `feat(silk): encode — LPC/LTP analysis, pitch estimation, NLSF quantization (VQ + stabilize),
  gain/excitation quant, the SILK bitstream`. Verify a shravan SILK frame decodes in libopus.
- `feat(opus): hybrid encode (SILK low band + CELT high band over one range coder) + the encoder
  mode/bandwidth decision and TOC assembly` → shravan encodes **every** Opus config libopus decodes.

### v2.9.0 — the rest of the suite, matured · medium (per-codec patches)
Bring the non-Opus codecs to real/complete and clear the open correctness bugs.
- `feat(alac): make the decoder live — wire detect_format/codec_open/MP4→alac_decode routing;
  value-verify on a real ALAC frame` (decoder exists but is unreachable today).
- `feat(flac): real LPC encoder (autocorrelation + Levinson-Durbin + quantized coefs, partitioned
  Rice, adaptive stereo, CONSTANT subframe, SEEKTABLE) + decoder MD5 verify` (Fixed-only today).
- `feat(aac): re-enable TNS (prediction-gain-gated), short-window, stereo/CPE, real HCB6, ATH/SMR
  psychoacoustics + optional R/D scale-factor loop`.
- `feat(mp4): esds/AudioSpecificConfig parse, AudioSampleEntry v1/v2, multi-track + edit-list
  (gapless trim), co64/largesize` (drop the hardcoded LC config).
- `fix: mp3 MPEG-2.5 8 kHz low-bitrate short blocks; flac seek anchor + Rice unary bound; wav
  WAVE_FORMAT_EXTENSIBLE 24-in-32; core detect_format/sankoch symbol collision (build warning)`.

### v3.0.0 — production-quality, interoperable, hardened suite · MAJOR
The major bump: every codec value-verified end-to-end both directions, fully interoperable, and
safe on hostile input. No known correctness bugs.
- `test/fuzz: AAC-decode, MP4-demux, Opus-decode fuzz targets (≥90K calls / 0 crashes each);
  harden anything they surface`.
- `feat(core): hi-res 88.2–384 kHz round-trip + PCM_F64 WAV; PCM SSE2/unrolled hot loops`.
- `perf: FFT-based MDCT/IMDCT preserving the verified TDAC convention (O(N²) today) — benchmarked`.
- **3.0.0 gate**: Opus encodes *and* decodes every config, interop-verified with libopus both
  ways; FLAC LPC encoder real; ALAC live; all decoders fuzzed 0-crash; benchmarks tracked in the
  CSV history; zero known correctness bugs.

### Post-3.0
- `feat: DSD64/128/256 + DoP (1-bit sigma-delta path)` — new format, additive.

## Working discipline (unchanged)

Every change lands as a small verified bite: build clean, all assertions pass, new tests +
benchmarks for new code, decode stays bit-exact through any shared-code refactor. Opus work is
proven by round-trip against the verified decoder and, where it matters, bit-exact against a
from-source libopus reference (`scratchpad/silkref`). Never claim a stage done until it is
value-verified (correlation, not just "tests pass").
