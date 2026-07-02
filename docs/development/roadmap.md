# Development Roadmap

> **v2.5.12** — 932 tests, Cyrius 6.3.27. **MP3 decode is comprehensive**: MPEG-1/2/2.5
> **Layer III** + **Layer II** + **Layer I**, all verified sample-exact against minimp3
> (one narrow known edge: MPEG-2.5 8 kHz low-bitrate short blocks). Builds on **v2.5.11**
> (MPEG-1 Layer III, 0.99999 vs minimp3) and **Opus/AAC** (2.5.9/2.5.10, encode→decode 0.99+).
> **Honest scope (do not claim "almost done").** The 2.5.x arc got every codec *producing
> audio*. It did **not** make them complete or conformant — a lot was deferred along the
> way. The 2.6.x–2.8.x plan below is the real remaining work, and it is large. It is
> enumerated in full here (nothing hidden in "deferred" asides), **Opus-real first** —
> decoding an actual `.opus` file was the one thing 2.5.x kept punting, so it is now
> v2.6.0, ahead of everything else, built foundation-up with each stage proven bit-exact
> against libopus. The true distance to a v1.0-quality codec suite stays visible.

## Functional status (verified 2026-07-02)

**"Tests pass" ≠ "codec produces audio."** A ground-truth audit on 2026-07-02 found the
then-green 843-assertion suite **materially overstated completeness**: many "roundtrip"
tests asserted only sample *count* (`vec_len==1024`) or *metadata*, never sample *values*,
so they passed on silence. The 2.5.9–2.5.12 arc closed those gaps — the suite is now 932
assertions with **value-level** (correlation / reference-decoder) checks on every audio
path. The lists below are the *current* verified state, not the original audit.

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
  with libopus — conformance is **v2.7.0**); transient frames decode via the long MDCT
  (short-window pre-echo coding is **v2.6.6**).
- **MP3 decode → PCM** *(2.5.11 + 2.5.12)* — `mp3_decode` produces real samples for
  **all three layers and all MPEG versions**, verified sample-exact vs minimp3:
  MPEG-1/2/2.5 **Layer III** (mono/stereo/M-S/intensity; bit reservoir; block switching),
  **Layer II** (mono/stereo/low-rate), **Layer I**. Ported from pdmp3 + mpg123 + minimp3.
  Untrusted-input fuzzed (L3 20K/0, L1/L2 20K/0) + hardened. Caveat: MPEG-2.5 8 kHz
  low-bitrate short blocks decode imperfectly (~0.7; narrow, tracked).

**🟥 Looks done, isn't — the one remaining piece of dead scaffolding:**
- **ALAC — dead code.** A full decoder exists but is **unwired**: `detect_format` has no
  ALAC branch and MP4 routes only to AAC, so `codec_open`'s `FMT_ALAC` dispatch is
  unreachable and the decoder is unverified on real frames. **Scheduled first: v2.6.0.**

**🐛 Open bugs (low severity — all scheduled into v2.6.0):**
- **[FLAC]** `decode_range` seek anchor uses the current frame's block size, not the
  stream's; Rice unary bound (32768) can reject valid deep-bps streams.
- **[WAV/core]** WAVE_FORMAT_EXTENSIBLE uses `valid_bits` as the byte stride (mis-decodes
  24-in-32); `detect_format` collides with sankoch's (build warning + consumer hazard).
- **[MP3]** MPEG-2.5 8 kHz low-bitrate short blocks decode imperfectly (~0.7; narrow).

**✅ Bugs resolved since the audit** (were listed here as "hidden by the green suite"):
- ~~[AAC] DoS infinite-loop on section codebook 12–15~~ — **fixed 2.5.10** (`test_aac_reserved_cb_no_hang`).
- ~~[AAC] scale-factor DPCM desync~~ — **fixed 2.5.10** (predictor tracks the clamped value, `aac.cyr:1107/1329`).
- ~~[AAC] CPE stereo bitstream malformed~~, ~~[AAC] decoder double-increments band 1–4/9/10~~ — **fixed 2.5.10**.
- ~~[MP3] Layer I/II used the Layer III bitrate table~~ — **fixed 2.5.11**. ~~[Opus] advertised samples but returned 0~~ — **fixed 2.5.9**.

---

## Forward plan — v2.6.x → v2.8.x (the real remaining work, ~11 releases)

Every codec produces audio (2.5.x). What follows makes them **complete, correct, and
conformant**. Each release is a coherent, shippable bite; every deferred item that ever
appeared in a CHANGELOG or an earlier roadmap is scheduled below — **none left floating**.

Acceptance rule for every release: **new/changed audio paths are proven by value-level
tests (SNR or correlation vs. input / a reference decoder), never by sample count** — that
is how the old suite hid the silence. Each commit builds clean
(`cyrius build src/main.cyr build/shravan`) and keeps the suite green. **Never skip
benchmarks** on a perf claim.

### v2.6.0 — **Opus is REAL: decode actual `.opus` files (RFC-6716 CELT)** · large · IN PROGRESS
**This is now #1, not last.** For the entire 2.5.x arc this was deferred every release
(it sat at v2.7.0, behind ALAC/fuzz/MP4/FLAC/AAC/hi-res). It is pulled to the front and
being built foundation-up, each stage proven **bit-exact against libopus** (system
libopus 1.6.1 + a C reference harness built from the fetched CELT source), and the whole
chain proven by PCM correlation vs ffmpeg's libopus decoder. Target: real libopus-encoded
fullband files (`config 31`, CELT-only, 20 ms, mono **and** stereo — confirmed what
libopus actually emits for music) decode to PCM. The bespoke LZMA-style coder is removed
from the file-decode path.

Progress (checked = landed + verified in the suite; unchecked = remaining, enumerated in
full — nothing hidden):
- [x] **Real `ec_dec` range decoder** (entdec.c/entcode.c port) — **bit-exact vs libopus**
  (`test_ec_dec_rfc_vectors`, 11 asserts against a libopus-`ec_enc`-produced vector).
- [x] **`ec_laplace_decode`** (laplace.c port) — **bit-exact vs libopus**
  (`test_ec_laplace_rfc_vectors`, 8 asserts). Coarse/fine/finalise energy decode ported
  from the float-build spec (`celt_unquant_coarse/fine/energy_finalise`).
- [x] **Bit allocation + front-of-frame decode** (rate.c `clt_compute_allocation` /
  `interp_bits2pulses` / `bits2pulses` / `init_caps`; `tf_decode`; and the full R1–R13
  prefix: silence, postfilter parse, transient, intra, coarse+fine energy, spread,
  dynalloc boosts, alloc_trim) — **bit-exact vs libopus** (`test_celt_allocation_rfc`,
  60 asserts across 4 real frames: intra+transient+boosts, inter/steady cross-frame,
  stereo/intensity, and a low-rate frame that actually skips bands). Large tables
  (`band_allocation`, `cache_index50/bits50/caps50`, `eBands`, `logN`) extracted
  verbatim from the runtime mode; signed-shift (`sar`) handles the negative-tilt sites.
- [ ] **Bands + PVQ + CWRS** (bands.c/vq.c/cwrs.c: `quant_all_bands` decode, `alg_unquant`
  → `cwrsi`, `exp_rotation` spreading, θ/split, intensity+dual stereo, `renormalise`,
  `denormalise_bands`, `anti_collapse`). Note: CELT's `cwrsi`/`icwrs` ordering must
  replace the bespoke PVQ index scheme for interop.
- [ ] **Inverse MDCT + FFT** (mdct.c/kiss_fft.c: `clt_mdct_backward`, the CELT low-overlap
  window (overlap=120, **not** 50%), pre/post rotation, per-channel overlap-add buffers).
- [ ] **Orchestration + state** (celt_decoder.c: `celt_decode_with_ec` order — silence,
  post-filter parse, transient/`tf_decode`, spread, dynalloc, trim, anti-collapse rsv,
  intensity/dual; persistent `oldBandE`/overlap/deemph state across frames; de-emphasis
  to PCM).
- [ ] **Wire + verify**: `detect_format` `.opus` → Ogg demux → real CELT decode; assert
  PCM correlation ≥ 0.99 vs ffmpeg for `real_mono.opus` + `real_stereo.opus`; retire the
  bespoke path from the file-decode dispatch.

### v2.6.1 — Opus encoder conformance (libopus decodes OURS) · large
- `feat(opus): real ec_enc + RFC band layout/allocation/TOC on the encode side` — goal:
  a shravan-encoded `.opus` decodes correctly in libopus/ffmpeg. Supersedes the bespoke
  encoder.

### v2.6.2 — Opus SILK mode · large (voice `.opus`)
- `feat(opus): SILK — LPC/LTP/LSF, excitation, shared range coder` (low-bitrate/voice
  files use SILK; needed for arbitrary `.opus`, not just fullband music).

### v2.6.3 — Opus hybrid mode · large (completes Opus)
- `feat(opus): hybrid — SILK low-band + CELT high-band over one range coder`.

### v2.6.4 — Opus CELT completion · medium (refinements from 2.5.9)
- `feat(opus): true overlapping short-window transient coding + two-pass VBR`.

### v2.6.5 — ALAC live + the open correctness bugs · medium
Make the dead code live and close every low-severity bug.
- `feat(alac): wire detect_format ALAC branch + codec_open FMT_ALAC dispatch + ALAC-in-MP4 routing (mp4 → alac_decode); verify on a real ALAC frame with a value-level test` — the decoder exists but is unreachable today.
- `fix(mp3): MPEG-2.5 8 kHz low-bitrate short blocks (~0.7 today; sfb tables + reservoir are byte-verified, 8 kHz is exact at higher bitrate — a narrow short-block interaction)`.
- `fix(flac): decode_range seek anchor uses the stream block size (not the current frame's); widen the Rice unary bound so valid deep-bps streams aren't rejected`.
- `fix(wav): WAVE_FORMAT_EXTENSIBLE container-bytes vs valid-bits (24-in-32 mis-decode)`.
- `chore(core): resolve the detect_format/sankoch symbol collision (namespace shravan's) — clears the build warning + consumer hazard`.

### v2.6.6 — untrusted-input fuzz coverage · small–medium
Close the fuzz gaps opened since 2.5.2 (decoders that parse hostile input but have no fuzz target).
- `test/fuzz: AAC-decode, MP4-demux, and Opus-decode fuzz targets (vendor the codec chain into the standalone harness); harden anything they surface. Target ≥90K calls / 0 crashes each.`

### v2.6.7 — MP4/M4A container completeness · medium (real-world `.m4a`)
- `feat(mp4): esds/AudioSpecificConfig parse (drop the hardcoded LC config), AudioSampleEntry v1/v2, multi-track + edit-list (elst/stts gapless trim), co64/largesize (>4 GiB)`.

### v2.6.8 — FLAC encoder completion · medium–large
Today FLAC encodes Fixed-prediction only; make it a real encoder + verify the decoder.
- `feat(flac): LPC encoder (autocorrelation + Levinson-Durbin + quantized coefficients), partitioned Rice, adaptive stereo-mode choice, CONSTANT subframe, SEEKTABLE emit, decoder MD5 signature verify`.

### v2.6.9 — AAC quality completion · medium
Refinements deferred from 2.5.10 (AAC already produces audio).
- `feat(aac): re-enable TNS with proper prediction-gain-gated noise shaping (disabled today — it amplified quant noise) + short-window + stereo/CPE`.
- `feat(aac): real HCB6 tables (reuses HCB5 today; our encoder never selects cb6) + psychoacoustic ATH floor + tonality-adjusted SMR; optional rate/distortion scale-factor loop (replaces the peak heuristic)`.

### v2.7.0 — hi-res, PCM breadth, transform perf · medium
- `feat(core): hi-res 88.2–384 kHz roundtrip tests; PCM_F64 WAV encode/decode; PCM SSE2/unrolled hot loops (before/after benchmarks)`.
- `perf: fast FFT-based MDCT/IMDCT preserving the verified TDAC convention — fft.cyr's pair and the MP3 IMDCT/synthesis cosines are direct O(N²) today (correct but slow). Benchmark before/after.`

### v2.8.0 — DSD · large (new format)
- `feat: DSD64/128/256 + DoP (1-bit sigma-delta path)`.

### v1.0 criteria (after the above)
Every codec: value-verified end-to-end; untrusted-input fuzzed 0-crash; Opus interoperates
with libopus; FLAC encoder is real (LPC); ALAC live; no known correctness bugs; benchmarks
tracked in the CSV history.

---

## Completed history

### v2.5.9–2.5.12 — the codecs actually produce audio (shipped 2026-07-02)

- **v2.5.12** — **MP3 decode is comprehensive**: MPEG-2/2.5 (LSF) Layer III + Layer II +
  Layer I join MPEG-1 Layer III, all sample-exact vs minimp3. Ported the mpg123 LSF
  scalefactor decode + unified the sfb tables (8-row, all 9 sample-rate configs); rewrote
  the stereo stage to minimp3's model (M/S + intensity before reorder, `max_band`
  boundary — the pdmp3 intensity was wrong); ported minimp3's Layer I/II subband-PCM
  (bit allocation + scalefactors + dequant → shared polyphase synth). Fuzzed L1/L2 20K/0.
  Known edge: MPEG-2.5 8 kHz low-bitrate short blocks (~0.7) → v2.6.0.
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
