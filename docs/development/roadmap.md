# Development Roadmap

> **v2.5.11** — 904 tests, Cyrius 6.3.27. **MP3 (MPEG-1 Layer III) now decodes to real PCM
> audio** — verified sample-for-sample against minimp3 (waveform correlation 0.99999) and
> the committed suite correlates the decode against its source (0.999). This joins
> **Opus and AAC** (2.5.9/2.5.10), which encode→decode to real PCM (0.99+, mono+stereo).
> The forward plan below was **reorganized 2026-07-02**: the sprawling
> 2.5.9–2.5.23 point-release list was collapsed into focused 2.5.x releases
> that **do the deferred core work** — the actual audio the 2.5.0–2.5.8 scaffolding was
> built for. Nothing here is pushed to a future major; every deferred tail lives inside
> the 2.5.x arc and gets *done*, packed as bite-sized commits.

## Functional status (verified 2026-07-02)

**"Tests pass" ≠ "codec produces audio."** A ground-truth audit (read the control flow +
ran probes, not the comments) found the green 843-assertion suite **materially overstates
completeness**: many "roundtrip" tests assert only sample *count* (`vec_len==1024`) or
*metadata*, never sample *values* — so they pass on silence/empty output.

**✅ Genuinely produces end-to-end audio (value-verified):**
- **WAV** encode+decode (u8/i16/i24/i32/f32); **AIFF** encode+decode (i8/i16/i24/i32 BE,
  `sowt`).
- **FLAC** encode (Fixed-prediction only, no LPC) + decode (full: CONSTANT/VERBATIM/
  FIXED/LPC, Rice, all 4 stereo modes, CRC, SEEKTABLE-ranged).
- **AAC encode+decode → PCM** *(2.5.10)* — `aac_encode` → `aac_decode` round-trips real
  audio (waveform correlation mono 0.999, stereo 0.999/0.999); **MP4/M4A** demux →
  AAC-decode → PCM. Caveat: TNS disabled (amplified quant noise — proper noise-shaping is
  a follow-up); HCB6 reuses HCB5 (never selected by shravan's encoder).
- **Opus/CELT encode+decode → PCM** *(2.5.9)* — `opus_encode` → `opus_decode_from_packets`
  round-trips real audio (waveform correlation: mono 0.997, stereo 0.996/0.999, transient
  0.998), mono + stereo. Caveat: a **bespoke, non-RFC-6716** stream (won't interoperate
  with libopus — conformance is a pinned 2.5.x item); transient frames decode via the
  long MDCT (short-window pre-echo coding deferred).
- **MP3 (MPEG-1 Layer III) decode → PCM** *(2.5.11)* — `mp3_decode` produces real samples
  (verified vs minimp3 at 0.99999, sample-exact; committed test correlates 0.999 vs the
  source two-tone), mono + stereo/joint, with the bit reservoir and long/short block
  switching. Ported from pdmp3 + ISO 11172-3; untrusted-input fuzzed (20K/0) + hardened.
  Caveat: MPEG-2/2.5 (LSF) L3 and Layer I/II decode remain metadata-only (fall back).

**🟥 Looks done, isn't (scaffolding / broken — the "actual core" that remains):**
- **ALAC — dead code.** A full decoder exists but is **unwired**: `detect_format` has no
  ALAC branch and MP4 routes only to AAC, so `codec_open`'s `FMT_ALAC` dispatch is
  unreachable and the decoder is unverified on real frames.

**🐛 Confirmed real bugs the "843 passing" hides** *(scheduled into 2.6.0 unless noted)*:
- **[AAC] DoS**: `_aac_decode_spectral` **infinite-loops** on section codebook 12–15
  (unvalidated `cb`) — hangs on malformed/unexpected input.
- **[AAC] scale-factor DPCM desync**: encoder clamps transmitted deltas to ±60 but
  tracks the *unclamped* predictor (mono `aac.cyr:1096`, CPE `:1319`); decoder tracks the
  clamped value (`:2466/2883/2935`) → any delta >±60 corrupts that band and all later ones.
- **[AAC] CPE stereo bitstream malformed** → wrong stereo (a stray 11-bit side ICS +
  codebook/escape disagreement; the "hang" theory was *refuted* — guards terminate it).
- **[AAC] decoder double-increments band** for codebooks 1–4/9/10 → skips the next band.
- ~~**[MP3]** Layer I/II parsed with the Layer III bitrate table~~ — **fixed 2.5.11** (per-
  layer bitrate tables; `test_mp3_layer2_bitrate`).
- ~~**[Opus]** decode advertised `total_samples>0` but returned 0 samples~~ — **fixed 2.5.9**
  (`opus_decode_from_packets` now returns real PCM).
- **[FLAC]** `decode_range` seek anchor uses the current frame's block size, not the
  stream's; Rice unary bound (32768) can reject valid deep-bps streams *(both low)*.
- **[core]** WAVE_FORMAT_EXTENSIBLE uses `valid_bits` as the byte stride (mis-decodes
  24-in-32); `detect_format` collides with sankoch's (build warning + consumer hazard)
  *(both low)*.

---

## Forward plan — remaining 2.5.x work

**Opus (2.5.9), AAC (2.5.10), and MP3 (2.5.11) now decode to real PCM audio** — see
Completed history + Functional status. What remains: the deferred tails + hardening + new
subsystems (2.5.12), plus the MP3 tails (MPEG-2/2.5 LSF, Layer I/II decode).

Acceptance rule for every release: **new/changed audio paths are proven by value-level
tests (SNR or correlation vs. input), never by sample count** — that is how the old suite
hid the silence. Each commit must build clean (`cyrius build src/main.cyr build/shravan`)
and keep the suite green.

### v2.5.12 — remaining deferred tails + hardening + new subsystems · medium–large

**Goal:** finish every remaining deferral and the low-severity bugs, then the genuinely-new
breadth. Order these by priority; each is an independent bite.

**Deferred tails + audit bugs (do first):**
- `feat(mp4): esds/AudioSpecificConfig parse (drop hardcoded LC), AudioSampleEntry v1/v2, multi-track/edit-list (elst/stts gapless trim), co64/largesize >4 GiB` *(2.5.7 deferral)*
- `feat(alac): wire detect_format + codec_open dispatch + ALAC-in-MP4 routing; verify the decoder on a real frame` *(dead-code today)*
- `test/fuzz: add AAC, MP4, and Opus-decode fuzz targets (untrusted-input paths since 2.5.2)` *(2.5.7 deferral)*
- `fix(flac): decode_range seek anchor uses stream block size; Rice unary bound for valid deep-bps streams`
- `fix(wav): WAVE_FORMAT_EXTENSIBLE container-bytes vs valid-bits (24-in-32)`
- `chore: resolve detect_format/sankoch collision (namespace shravan's) — clears the build warning`
- `feat: hi-res 88.2–384 kHz roundtrip tests; PCM_F64 WAV encode/decode; PCM SSE2/unrolled hot loops + before/after benchmarks`
- `feat(flac): LPC encoder (autocorr + Levinson-Durbin + quantized coeffs), partitioned Rice, adaptive stereo choice, CONSTANT subframe, SEEKTABLE emit, decoder MD5 verify`

**Codec completions (deferred from 2.5.9 / 2.5.10 — refinements, codecs already produce audio):**
- `feat(aac): re-enable TNS with proper prediction-gain-gated noise shaping (it currently amplifies quant noise, so it is disabled) + short-window + stereo/CPE`
- `feat(aac): real HCB6 tables (reuses HCB5 today; shravan's encoder never selects cb6) + psychoacoustic ATH floor + tonality-adjusted SMR; optional rate/distortion scale-factor loop`
- `feat(opus): true overlapping short-window transient coding (transient frames decode via the long MDCT today) + two-pass VBR`
- `feat(mp3): MPEG-2/2.5 (LSF) Layer III decode (half-rate side info, one granule, LSF intensity-stereo scalefactor compression) + Layer I/II decode (both metadata-only today)`
- `perf: fast FFT-based MDCT/IMDCT preserving the verified convention (fft.cyr's pair is now O(N²) direct — correct but slow; the fast fft_mdct/fft_imdct were replaced because they did not invert each other). The MP3 IMDCT/synthesis cosines are likewise direct O(N²). Benchmark before/after.`

**New subsystems (larger; may each become their own 2.5.x point release):**
- `feat(opus): RFC-6716 conformance — real ec_enc/ec_dec, Laplace coarse/fine energy, spec band layout + allocation (trim/boost/anti-collapse/fine-energy), correct TOC` — decode real `.opus`, libopus decodes ours *(supersedes the bespoke coder)*
- `feat(opus): SILK mode — LPC/LTP/LSF, excitation, shared range coder`
- `feat(opus): hybrid mode — SILK low-band + CELT high-band over one range coder (completes the Opus encoder)`
- `feat: DSD — DSD64/128/256 + DoP (1-bit sigma-delta path)`

---

## Completed history

### v2.5.9–2.5.11 — the codecs actually produce audio (shipped 2026-07-02)

- **v2.5.11** — **MP3 (MPEG-1 Layer III) decode produces real PCM** (verified vs minimp3 at
  0.99999, sample-exact; committed test 0.999 vs the source two-tone), mono + stereo/joint.
  Ported from pdmp3 + ISO 11172-3: side info + cross-frame bit reservoir, scalefactors,
  Huffman (34-tree array from pdmp3 + big-values regions + count1 quads + linbits),
  requantization, short-block reorder, MS/intensity stereo, antialias, hybrid IMDCT
  (long 36 / short 12 + block-type switching), and the 512-tap polyphase synthesis
  filterbank. Untrusted-input fuzzed (20K/0) + hardened (big_values / region-index
  clamps). MPEG-2/2.5 (LSF) L3 and Layer I/II decode deferred to 2.5.12 (metadata-only).
- **v2.5.10** — **AAC encode→decode produces real PCM** (mono 0.999, stereo 0.999/0.999).
  Fixed the `fft_mdct`/`fft_imdct` mismatch (verified TDAC pair — the root cause silencing
  *both* encoders), the section-coding-by-has-data-not-codebook bug, the invertible
  quantizer + peak scale factors + 50%-overlap framing, the CPE stereo bitstream (stray
  side ICS + escape-codebook mismatch), the DPCM predictor sync, and the decoder DoS /
  band-double-increment bugs. TNS disabled (amplified quant noise); HCB6/psy-ATH deferred.
- **v2.5.9** — **Opus encode→decode produces real PCM** (mono 0.997, stereo 0.996/0.999,
  transient 0.998). Composed the 2.5.0–2.5.8 scaffolding into audio: magnitude synthesis,
  IMDCT + 50%-overlap TDAC, wired `opus_decode_from_packets` (mono+stereo), band-energy
  DPCM widened to 256 symbols, MDCT-spectrum rate complexity. Bespoke non-RFC-6716 stream;
  transient uses long MDCT; two-pass VBR + RFC conformance deferred.

### v2.5.0–2.5.8 — CELT/Opus + AAC sub-layer scaffolding (shipped, internal-only)

Nine point releases built the Opus/CELT entropy + allocation layers and several AAC
sub-features, each proven **internally bit-exact** but **not composed into PCM** — the
scaffolding that 2.5.9/2.5.10 composed into real audio (above).

- **2.5.8** rate control + VBR (bit-reservoir CBR/CVBR/VBR, complexity metric).
- **2.5.7** MP4/M4A container demux (box tree, sample table, ADTS-wrap → `aac_decode`).
- **2.5.6** CELT stereo full two-channel bitstream (replaces the mono downmix).
- **2.5.5** CELT stereo coupling primitives (M/S transform, per-band couple/decouple).
- **2.5.4** transient detection + short-block MDCT (encode-side; no decode inverse).
- **2.5.3** AAC psychoacoustic masking model (scale-factor coarsening).
- **2.5.2** toolchain 6.3.25, serde Str-deserialize repair, AAC TNS (long-window mono).
- **2.5.1** first CELT decode + matched invertible range coder (test-only path).
- **2.5.0** full PVQ spectral shape (CWRS index ↔ vector, pulse search).

### v2.0.0–v2.4.4 — port, performance, hardening, framework

- **v2.4.4** Opus encoder framework (config/state, mode+bandwidth select, TOC byte).
- **v2.4.3** `dist/shravan.cyr` distlib bundle + `src/main.cyr` split into library + harness.
- **v2.4.2** fuzz harness rebuilt for 6.3.19; `cyrius vet` + fuzz smoke in CI.
- **v2.4.1** serde full type coverage (enums, int structs, metadata, codec markers).
- **v2.4.0** Cyrius toolchain modernization (cc3 4.10.3 → 6.3.19), `cyrius.cyml`, serde wired.
- **v2.3.0–2.3.2** security audit (21/21 fixed, 90K fuzz 0 crashes); AAC spectral codebooks
  1–11; AAC per-band codebook selection, M/S encode, VBR; `decode_file`/`decode_reader`.
- **v2.2.0** performance + stdlib modernization (MDCT 5.45×, FFT twiddle cache, sorted
  Huffman, polyphase resampler, sankoch dep).
- **v2.1.0–2.1.1** AAC-LC decoder (Huffman codebooks, short windows), M/S stereo, metadata
  writing, FLAC incremental streaming + SEEKTABLE.
- **v2.0.0** full Rust → Cyrius port (10,265 → 11,780 lines); f64-internal samples; packed
  Result errors; all 16 modules; FLAC 5.2× of Rust+LLVM, WAV 42×, compile 16× faster.

---

## Resolved bugs

- ~~FLAC encoder segfault~~ — Fixed in cc3 3.3.11 (compiler codegen issue)
- ~~AAC decoder blocked on parser limit~~ — Fixed in cc3 3.3.17 (tok_names overflow)
- ~~tok_names regression in 3.4.x~~ — Fixed in cc3 3.4.3
