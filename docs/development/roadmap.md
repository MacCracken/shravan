# Development Roadmap

> **v2.6.4** — 11357 tests, Cyrius 6.3.x. **Opus 10 ms frames are real**: shravan decodes
> actual libopus **10 ms** `.opus` — SILK-only (config 0/8), hybrid (config 12/14, CELT
> LM=2), CELT-only (config 18/22/26/30) — mono + stereo, at **hybrid-10 ms correlation
> 0.9999 vs libopus**. Fixed a real bit-exact bug (**`special_hybrid_folding`** was missing
> from `quant_all_bands` — hybrid's second CELT band folded from a zeroed region; benefits
> all hybrid decodes). Split the legacy 2.5.x encoder out of `opus.cyr` (→ `opus_legacy.cyr`,
> not bundled) to hold the 256 KB distlib cap. shravan now decodes **10 ms + 20 ms** Opus,
> all bandwidths, mono + stereo. Prior:
> **v2.6.3** — **Opus stereo is real**: shravan decodes actual
> libopus **stereo** `.opus` — SILK mid/side stereo (config 1/9) + stereo hybrid (13/15),
> 20 ms — at **per-channel correlation 1.0000/0.9999 vs libopus**. **Every 20 ms Opus config
> now decodes** (mono + stereo). Prior:
> **v2.6.2** — **Opus hybrid is real**: shravan decodes actual
> libopus **hybrid** `.opus` (SILK low band + CELT high band over one shared range coder,
> SWB/FB 20 ms mono) at **correlation 0.9999 vs libopus**; the CELT refactor also lit up the
> CELT-only 20 ms configs (NB/WB/SWB). Prior:
> **v2.6.1** — **SILK is real**: shravan decodes actual
> libopus SILK-mode `.opus` files (voice, NB/WB 20 ms mono) to 48 kHz at correlation
> **0.999994 vs ffmpeg**, every internal stage bit-exact vs libopus. Prior:
> **v2.6.0** — **Opus is real**: shravan decodes actual
> libopus `.opus` files (CELT-only fullband) at correlation 1.0 / SNR ~131 dB vs ffmpeg.
> Prior: **v2.5.12** — MP3 decode comprehensive: MPEG-1/2/2.5
> **Layer III** + **Layer II** + **Layer I**, all verified sample-exact against minimp3
> (one narrow known edge: MPEG-2.5 8 kHz low-bitrate short blocks). Builds on **v2.5.11**
> (MPEG-1 Layer III, 0.99999 vs minimp3) and **Opus/AAC** (2.5.9/2.5.10, encode→decode 0.99+).
> **Honest scope (do not claim "almost done").** The 2.5.x arc got every codec *producing
> audio*. It did **not** make them complete or conformant — a lot was deferred along the
> way. The 2.6.x–2.8.x plan below is the real remaining work, and it is large. It is
> enumerated in full here (nothing hidden in "deferred" asides). **Opus-real shipped in
> v2.6.0** — real `.opus` (CELT-only fullband) decodes at correlation 1.0 vs ffmpeg; the
> remaining Opus lifts (SILK 2.6.1, hybrid 2.6.2, encoder 2.6.3) lead the forward plan,
> then the rest. The true distance to a v1.0-quality codec suite stays visible.

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
- **Opus CELT decode → PCM, RFC-6716** *(2.6.0)* — `opus_decode_from_packets` decodes real
  libopus-encoded `.opus` files (CELT-only fullband, config 31, mono + stereo) end-to-end at
  **correlation 1.000000 / SNR ~131 dB vs ffmpeg**, sample-accurate to ~1e-7. Every decode
  stage is bit-exact vs libopus (range coder, energy, allocation, PVQ/CWRS, MDCT, postfilter).
  The bespoke non-RFC encoder (2.5.9) is retained as legacy for the 2.6.3 encoder work but
  is off the file-decode path.
- **Opus SILK decode → PCM, RFC-6716** *(2.6.1)* — `opus_decode_from_packets` decodes real
  libopus-encoded SILK-mode `.opus` files (voice: config 1 NB / 9 WB, 20 ms mono) end-to-end
  to 48 kHz at **correlation 0.999994 vs ffmpeg** (residual is sub-sample delay/pre-skip
  phase). Every stage bit-exact vs a from-source libopus reference decoder: `decode_indices`,
  `NLSF_decode`, `NLSF2A`, pitch/LTP, `decode_pulses` (shell coder), `decode_core` (LTP+LPC
  synthesis), and the internal→48 kHz resampler. Not yet: SILK **MB** (12 kHz), **10/40/60 ms**
  frames, **stereo** (MS) SILK.
- **Opus hybrid + CELT-only configs → PCM, RFC-6716** *(2.6.2)* — hybrid `.opus` (SILK low
  band + CELT high band, one shared range coder; config 13 SWB / 15 FB, 20 ms mono) decodes at
  **correlation 0.9999 vs a libopus golden**, per-frame ≥ 0.99993. The CELT decoder is now
  band-range/accumulate parameterized, so **CELT-only 20 ms configs 19/23/27** (NB/WB/SWB)
  decode too (config-23 bit-exact vs libopus).
- **Opus stereo → PCM, RFC-6716** *(2.6.3)* — SILK mid/side stereo (config 1/9) and stereo
  hybrid (config 13/15), 20 ms, decode at **per-channel correlation 1.0000 / 0.9999 vs a
  libopus golden** (`silk_stereo_decode_pred` bit-exact; `silk_stereo_ms_to_lr` +
  `silk_decode_stereo` + CELT stereo high band). **Every 20 ms config now decodes** (mono +
  stereo, all bandwidths). Not yet: **10 ms** frames (2.6.4), **encoder** (2.6.5).
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

### v2.6.1 — Opus SILK mode · ✅ SHIPPED (voice `.opus`)
SILK NB/WB 20 ms mono decode landed: `decode_indices` → `NLSF_decode` → `NLSF2A` →
pitch/LTP → `decode_pulses` (shell coder) → `decode_core` (LTP+LPC synthesis) →
internal→48 kHz resampler, every stage **bit-exact vs a from-source libopus reference**
(11343 assertions at 2.6.1), and a real libopus SILK `.opus` decodes at **correlation 0.999994
vs ffmpeg**. Wired into `opus_decode_from_packets` (config 1 NB / 9 WB, code 0, mono).
- **Remaining SILK surface** (folded forward, not silently dropped): **MB** (12 kHz
  internal, config 4–7), **10/40/60 ms** frame sizes (multi-SILK-frame packets), and
  **stereo** (MS prediction). These currently emit timeline-aligned silence.

### v2.6.2 — Opus hybrid mode · ✅ SHIPPED (SILK low-band + CELT high-band)
Hybrid decode landed for **config 13 (SWB) / 15 (FB), 20 ms mono**: one shared `ec` →
SILK low band (WB 16 kHz) from the front → CELT high band (`start=17`, end 19/21) from the
same `ec`, accumulated onto the SILK output. Verified at **corr 0.9999 vs a libopus golden**
(`test_opus_hybrid_rfc`), per-frame ≥ 0.99993 on a real file. The refactor generalized the
CELT decoder to `(start, end, accum)` + shared `ec` (config-31 stays bit-exact), which also
lit up **CELT-only 20 ms configs 19 (NB) / 23 (WB) / 27 (SWB)** — config-23 verified bit-exact
(`test_opus_celtwb_rfc`). Fixed a real `quant_all_bands` fold bug (`norm_offset` for `start>0`).
- **Remaining hybrid surface** (folded forward): **10 ms** (config 12/14, needs CELT LM=2),
  **stereo** hybrid (needs SILK MS stereo), and redundant-frame audio. CELT-only non-20 ms
  frame sizes remain too.

### v2.6.3 — Opus stereo · ✅ SHIPPED (SILK stereo + stereo hybrid)
SILK mid/side stereo landed: `silk_stereo_decode_pred` (bit-exact), `silk_stereo_ms_to_lr`
(3-tap predictor interpolation + MS→LR with `sMid`/`sSide` history), and `silk_decode_stereo`
(both channels' VAD → predictors → mid + side frames → MS→LR → resample each). Wired for
**stereo config 1/9** (SILK) and **stereo config 13/15** (hybrid: SILK stereo low band + CELT
stereo `C=2, start=17` high band over the shared ec). Verified **per-channel corr 1.0000**
(SILK stereo) / **0.9999** (stereo hybrid) vs a libopus golden. A VAD-flag refactor kept mono
+ hybrid **bit-exact**. **Every 20 ms Opus config now decodes** (mono + stereo, all bandwidths).

### v2.6.4 — Opus 10 ms frames · ✅ SHIPPED (SILK/hybrid/CELT 10 ms, mono + stereo)
20 ms was done; 2.6.4 adds the 10 ms frame size across every mode. CELT LM=2 (N=480, M=4,
short=120) was parameterized in the 2.6.3 refactor; 2.6.4 threads `nb_subfr=2` through SILK
and wires the dispatch: SILK-only config 0 (NB) / 8 (WB); hybrid config 12 (SWB, end=19) /
14 (FB, end=21) with SILK WB low band + CELT `LM=2, start=17` high band; CELT-only
18/22/26/30 — each mono **and** stereo, state cache keyed on `(fs, nb_subfr)`.
- **Real bit-exact fix — `special_hybrid_folding`** (`bands.c:1579`): hybrid's second CELT
  band has no lower band to fold from, so libopus duplicates the first band's fold data
  (`norm[n1..n2] ← norm[2·n1−n2..]`); shravan left it zero. Only bites when that band is
  *folded* (0 pulses), which is exactly the 10 ms hybrid case — traced stage-by-stage
  (energy/alloc/PVQ/`X` all matched, only the fold source differed), correlation
  **0.9662 → 0.9999**. Shared-path fix (helps all hybrid decodes).
- **Module split**: legacy 2.5.x bespoke encoder (~100 KB, 91 fns, off the decode path)
  moved to `src/opus_legacy.cyr` (included by `main.cyr`, **not** in `[lib]`), dropping
  `opus.cyr` 261 → 162 KB under the 256 KB distlib cap.
- In-suite guards: `test_opus_silk10_rfc` (1.0000), `test_opus_hybrid10_rfc` (0.9999),
  `test_opus_celt10_rfc` (1.0000).

### v2.6.5 — Opus encoder conformance (libopus decodes OURS) · large
The decode side is real (2.6.0–2.6.4); make the encode side real too.
- `feat(opus): real ec_enc + RFC-6716 CELT encode (band energy quant, allocation, PVQ
  search/encode via icwrs, TOC)` — goal: a shravan-encoded `.opus` decodes correctly in
  libopus/ffmpeg. Supersedes the bespoke 2.5.9 encoder. (SILK/hybrid encode follow.)

### v2.6.6 — Opus CELT completion · medium (refinements from 2.5.9)
- `feat(opus): true overlapping short-window transient coding + two-pass VBR`.

### v2.6.7 — ALAC live + the open correctness bugs · medium
Make the dead code live and close every low-severity bug.
- `feat(alac): wire detect_format ALAC branch + codec_open FMT_ALAC dispatch + ALAC-in-MP4 routing (mp4 → alac_decode); verify on a real ALAC frame with a value-level test` — the decoder exists but is unreachable today.
- `fix(mp3): MPEG-2.5 8 kHz low-bitrate short blocks (~0.7 today; sfb tables + reservoir are byte-verified, 8 kHz is exact at higher bitrate — a narrow short-block interaction)`.
- `fix(flac): decode_range seek anchor uses the stream block size (not the current frame's); widen the Rice unary bound so valid deep-bps streams aren't rejected`.
- `fix(wav): WAVE_FORMAT_EXTENSIBLE container-bytes vs valid-bits (24-in-32 mis-decode)`.
- `chore(core): resolve the detect_format/sankoch symbol collision (namespace shravan's) — clears the build warning + consumer hazard`.

### v2.6.8 — untrusted-input fuzz coverage · small–medium
Close the fuzz gaps opened since 2.5.2 (decoders that parse hostile input but have no fuzz target).
- `test/fuzz: AAC-decode, MP4-demux, and Opus-decode fuzz targets (vendor the codec chain into the standalone harness); harden anything they surface. Target ≥90K calls / 0 crashes each.`

### v2.6.9 — MP4/M4A container completeness · medium (real-world `.m4a`)
- `feat(mp4): esds/AudioSpecificConfig parse (drop the hardcoded LC config), AudioSampleEntry v1/v2, multi-track + edit-list (elst/stts gapless trim), co64/largesize (>4 GiB)`.

### v2.6.10 — FLAC encoder completion · medium–large
Today FLAC encodes Fixed-prediction only; make it a real encoder + verify the decoder.
- `feat(flac): LPC encoder (autocorrelation + Levinson-Durbin + quantized coefficients), partitioned Rice, adaptive stereo-mode choice, CONSTANT subframe, SEEKTABLE emit, decoder MD5 signature verify`.

### v2.6.11 — AAC quality completion · medium
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

### v2.6.0 — Opus is REAL: real `.opus` files decode (shipped 2026-07-02)

**The interop milestone.** shravan decodes actual libopus-encoded `.opus` files
(RFC 6716, CELT-only fullband, config 31, mono + stereo) end-to-end to PCM at
**correlation 1.000000 / SNR ~131 dB vs ffmpeg**, sample-accurate to ~1e-7. Retired the
bespoke non-RFC stream from the file-decode dispatch. Full libopus CELT decoder ported
foundation-up, every stage bit-exact vs libopus: `ec_dec` range coder, `ec_laplace` +
energy, `clt_compute_allocation` + `tf_decode`, `cwrsi`/`quant_all_bands` (PVQ/CWRS +
stereo), `clt_mdct_backward` + `anti_collapse` + pitch `comb_filter` + de-emphasis.
`opus_decode_from_packets` now drives it (pre-skip trim, granule cap). Proven in-suite by
`test_ec_dec_rfc_vectors`, `test_ec_laplace_rfc_vectors`, `test_celt_allocation_rfc`,
`test_celt_cwrs_rfc`, `test_celt_bands_rfc`, `test_celt_imdct_unit`, `test_celt_pcm_rfc`,
`test_celt_pcm_stereo_rfc`. Scope: CELT-only fullband (music); SILK/hybrid/encoder are
2.6.1–2.6.3. 1033 assertions.

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
