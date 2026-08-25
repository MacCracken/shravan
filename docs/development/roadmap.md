# Development Roadmap

**Current: v2.7.2.** Shipped history lives in `CHANGELOG.md`; this file is forward-looking only.

## Where we are

- **Decode is real and complete for the mainstream Opus surface.** shravan decodes actual
  libopus `.opus` — CELT + SILK + hybrid, mono **and** stereo, 10 ms **and** 20 ms, all
  bandwidths — bit-exact vs libopus (correlation ~1.0 vs ffmpeg). Plus WAV, AIFF, FLAC (enc+dec),
  Ogg, MP3 (MPEG-1/2/2.5 Layer I/II/III), AAC-LC, MP4/M4A demux, ALAC (decoder present).
- **CELT encode is real and complete — mono AND stereo, at quality (v2.6.0–2.6.7).** shravan encodes
  a real RFC-6716 **CELT** frame that **libopus decodes sample-identical** to shravan's own decoder
  (correlation 1.000000), with the full decision surface: adaptive spread, 2-pass coarse-energy race,
  transient detection + short-block encode, per-band tf_analysis, masking-driven dynalloc boosts, and
  **stereo** (joint mid/side + intensity, `stereo_analysis` dual decision). Every stage ported from
  libopus's float build and adversarially verified faithful.
- 11,622 assertions, 0 failing. Cyrius toolchain pinned at 6.5.35.

The remaining distance is (1) the rest of the Opus **encoder** (SILK + hybrid), (2) closing the small
decode gaps, and (3) bringing the non-Opus codecs and the whole suite to production/interop quality.
That is the path to **3.0.0**, below.

## Path to 3.0.0

The CELT encoder is done (mono + stereo). Three minors then finish Opus encode (SILK + hybrid),
mature the rest of the suite, and harden everything; **3.0.0** is the major bump when the suite is
complete, fully interoperable both directions, and fuzz-clean.

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
- `fix: mp3 MPEG-2.5 8 kHz low-bitrate short blocks; core detect_format/sankoch symbol collision
  (build warning)`. (**flac seek anchor** and **wav WAVE_FORMAT_EXTENSIBLE 24-in-32** were fixed in
  2.6.9 — SEC-031 and the container/valid-bits split. The **Rice unary bound** was reviewed and
  deliberately left at 32768: it is the SEC-008 security bound, and no failing real-world file
  was produced to justify loosening it.)
- `feat(tag): de-unsynchronisation is unimplemented (0x80 tags are reported unsupported since
  2.6.9 rather than mis-sliced)`. (**ID3v2.2** frame layout and 3-char frame IDs landed in
  2.6.10.)
- `feat(aac): PNS noise generator is not the reference one` — 2.7.1 fills noise bands to the
  correct coded energy (verified within 0.4% against ffmpeg) but with its own deterministic PRNG,
  so PNS bands never match a reference decoder sample-for-sample. That is inherent to noise
  substitution, not a defect, and only the band energy is defined by the format.
- `feat(aac): encoder does not use the stereo tools` — the decoder handles M/S (per band and
  all-bands), intensity stereo and PNS, but shravan's own encoder still emits neither intensity
  nor PNS, and only all-bands M/S.
- `fix(alac): alac_unmix_stereo uses a logical shift on a signed product` — confirmed by the
  2.6.9 audit.

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
