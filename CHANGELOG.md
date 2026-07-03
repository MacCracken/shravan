# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.6.1] - 2026-07-02

**SILK is real.** shravan now decodes actual libopus-encoded SILK-mode `.opus` files
(RFC 6716, voice: TOC config 1 = NB / config 9 = WB, 20 ms mono) end-to-end to 48 kHz
PCM — **correlation 0.999994 vs ffmpeg's libopus decoder** on a real file (the residual
is sub-sample delay/pre-skip phase, not decode error). Every internal stage is proven
**bit-exact against libopus** via a from-source reference decoder (`opusdec_dump`,
built with per-stage `SILK_DUMP` instrumentation) — 11343 assertions total. Built
foundation-up as an exact fixed-point port (int16/int32 widths, arithmetic shifts and
saturation emulated in Cyrius i64).

### Added — real RFC-6716 SILK decoder (`src/silk.cyr`)

- **LP-layer header + `silk_decode_indices`** — VAD/LBRR flags, signal type, gains
  (with `silk_gains_dequant` LastGainIndex state), NLSF stage-1/stage-2 indices,
  interpolation factor, and the voiced block (pitch lag, contour, LTP period + taps,
  LTP scale, seed). Bit-exact for NB (order 10) + WB (order 16) (`test_silk_indices_rfc`).
- **NLSF → LPC** — `silk_nlsf_decode` (predictive residual dequant + inverse-weighted
  CB1 + `silk_nlsf_stabilize`) → `pNLSF_Q15`, then `silk_nlsf2a` (LSF cosine-table
  `find_poly` P/Q convolution → `silk_lpc_fit` int16 fitting → the QA=24
  `silk_lpc_inv_pred_gain` stability/bandwidth-expansion loop) → `PredCoef_Q12`.
  Bit-exact vs the reference `PredCoef_Q12` dump, NB + WB.
- **Pitch + LTP** — `silk_decode_pitch` (lag + contour-codebook offset, clamped),
  `silk_decode_ltp` (per-subframe taps from the PER codebook, Q7→Q14), `silk_ltp_scale`.
  Bit-exact on voiced frames.
- **Excitation** — `silk_decode_pulses`: rate level, per-block sum-of-pulses with the
  LSB-shift escape, the 15-node `silk_shell_decoder` split tree over shell-code tables
  0–3, and `silk_decode_signs`. Every sample of 4 excitation vectors bit-exact.
- **Synthesis** — `silk_decode_core`: excitation reconstruction (`silk_RAND` LCG,
  quant-offset, sign), LTP long-term prediction, re-whitening via
  `silk_lpc_analysis_filter`, LPC short-term synthesis (order 10/16), gain scaling.
  Internal-rate output bit-exact for unvoiced (frame 0) and voiced (frame 1) frames.
- **Resampler** — `silk_resampler` (internal 8/16 kHz → 48 kHz): 2× all-pass HQ
  upsampler (`silk_resampler_up2_hq`) + 8-tap fractional FIR interpolation over the
  `frac_FIR_12` ROM, with the 1 ms input-delay buffer. Output bit-exact vs libopus.
- **Orchestration** — `silk_decoder_init` / `silk_decode_frame` / `silk_decode_pcm`:
  the full stateful per-frame pipeline (indices → pulses → NLSF interpolation for
  `NInterp<4` → pitch/LTP → core → resample) with cross-frame state (outBuf, sLPC,
  prev_gain, LastGainIndex, prevNLSF, sMid delay). End-to-end bit-exact bits → 48 kHz.

### Changed

- `opus_decode_from_packets` (`src/opus.cyr`) now routes SILK-only 20 ms mono packets
  (config 1 NB / 9 WB, code 0) through the real SILK decoder → 48 kHz, alongside the
  2.6.0 CELT config-31 path. Other configs (MB SILK, 10/40/60 ms, hybrid, stereo SILK)
  still emit timeline-aligned silence.

### Fixed

- `ogg_decode` now lazy-inits the CRC32 table, so consumers calling `decode_file()` on
  an Ogg/Opus file without the full init sequence no longer dereference a null table.
- `silk_isort_i16` (the NLSF-stabilizer insertion-sort fallback) mistranslated the C
  loop's stop condition — it clobbered the loop index on the `a[j] <= value` exit and
  wrote every element to index 0, corrupting the sort. Caught by an adversarial audit of
  the port against the libopus C reference; the fallback fires only when the 20-loop
  iterative stabilizer fails to converge on extreme NLSF vectors, so it is off the
  common path but reachable from arbitrary valid bitstreams. Now sorts correctly, with a
  direct unit test (`test_silk_isort_rfc`).

### Not yet (honest status)

- SILK **MB** (12 kHz internal, config 4–7), **10/40/60 ms** frame sizes, and **stereo**
  (MS) SILK are not decoded (silence). **Hybrid** (2.6.2) and the **encoder** (2.6.3)
  remain. Claim: "real CELT + SILK 20 ms mono `.opus` decode"; not "Opus complete".

## [2.6.0] - 2026-07-02

**Opus is real.** shravan now decodes actual libopus-encoded `.opus` files
(RFC 6716, CELT-only fullband — TOC config 31, mono + stereo) end-to-end to PCM,
**correlation 1.000000 / SNR ~131 dB vs ffmpeg's libopus decoder**, matching sample
values to ~1e-7. This is the interop milestone the 2.5.x arc kept deferring: the
bespoke, non-RFC LZMA-style stream is retired from the file-decode dispatch and
replaced by a real port of the libopus CELT decoder, built foundation-up with every
stage proven **bit-exact against libopus**. 1033 assertions.

### Added — real RFC-6716 CELT decoder (`src/opus.cyr`)

- **`ec_dec` range decoder** — exact port of libopus `entdec.c`/`entcode.c`
  (`ec_decode`/`ec_dec_update`/`ec_dec_bit_logp`/`ec_dec_icdf`/`ec_dec_uint`/
  `ec_dec_bits`/`ec_tell`/`ec_tell_frac`). Bit-exact vs a libopus-`ec_enc`-produced
  vector (`test_ec_dec_rfc_vectors`).
- **Energy** — `ec_laplace_decode` + coarse/fine/finalise (`quant_bands.c` float path).
  Laplace bit-exact vs libopus (`test_ec_laplace_rfc_vectors`); `e_prob_model`,
  `pred_coef`/`beta`, `eMeans` extracted verbatim.
- **Bit allocation + front-of-frame** — `tf_decode`, `clt_compute_allocation` /
  `interp_bits2pulses` / `bits2pulses` / `init_caps`, and the full silence → postfilter
  → transient → intra → coarse → tf → spread → dynalloc → trim → alloc → fine sequence.
  Bit-exact across 4 real frames incl. band-skipping (`test_celt_allocation_rfc`).
- **PVQ / CWRS / bands** — `cwrsi`/`decode_pulses` (Fischer U-table, replaces the bespoke
  index scheme), `alg_unquant`, `exp_rotation`, `quant_all_bands` / `quant_partition` /
  `quant_band_stereo`, `compute_theta`/`compute_qn`, `bitexact_cos`/`log2tan`,
  intensity+dual stereo, `denormalise_bands`. Verified via CWRS vectors
  (`test_celt_cwrs_rfc`) and full-recursion range-sync + collapse-mask match
  (`test_celt_bands_rfc`, mono+stereo, transient+steady).
- **Synthesis** — inverse MDCT (`clt_mdct_backward`, direct DFT, CELT 120-sample overlap
  window, per-channel overlap-add via `decode_mem`), `anti_collapse`, pitch `comb_filter`
  post-filter, and de-emphasis. IMDCT impulse-exact (`test_celt_imdct_unit`).
- **Wired**: `opus_decode_from_packets` drives the real decoder (config-31 detection,
  per-packet channels, pre-skip trim, granule cap). Full-file decode of
  `real_mono.opus`/`real_stereo.opus` verified 1.0 vs ffmpeg;
  `test_celt_pcm_rfc` + `test_celt_pcm_stereo_rfc` assert PCM correlation in-suite.

### Changed

- The bespoke Opus encoder/decoder path (2.5.9) is now **legacy** — retained for the
  encoder-conformance work (2.6.3) but no longer on the `.opus` file-decode path. The
  three bespoke encode→decode roundtrip tests (which asserted that superseded contract)
  were removed; the real decoder is verified against actual libopus streams instead.

### Scope (honest)

- Done: **CELT-only, fullband (config 31)** decode — what libopus emits for music, so
  real-world music `.opus` files decode today.
- Not yet: **SILK** (voice, 2.6.1), **hybrid** (2.6.2), and the **encoder** so libopus
  can decode shravan's output (2.6.3). See `docs/development/roadmap.md`.

## [2.5.12] - 2026-07-02

**MP3 decode is now comprehensive** — MPEG-2/2.5 (LSF) Layer III, plus **Layer II
and Layer I**, join the MPEG-1 Layer III decoder from 2.5.11. Every common
configuration is verified **sample-exact against minimp3**. 932 assertions
(was 909, +23).

### Added — MPEG-2/2.5 (LSF) Layer III (`src/mp3.cyr`)

- **LSF side info** (one granule, 8-bit `main_data_begin`, no `scfsi`, 9-bit
  `scalefac_compress`) + **LSF scalefactor decode** (ported from mpg123's
  `III_get_scale_factors_2`: the 4 partition slens derive from the sfc range, the
  band counts from the `stab` table). Unified the scalefactor-band tables to one
  8-row set covering all 9 sample-rate configs (minimp3 `sr_idx` order).
- Verified sample-exact vs minimp3: **MPEG-2 22050 Hz mono + joint stereo (M/S)
  1.00000**, **MPEG-2.5 11025 Hz mono 1.00000**.
- **Stereo stage rewritten** to minimp3's model — M/S and intensity now run in
  bitstream order *before* reorder, with the correct `max_band` boundary and the
  MPEG-1 `g_pan` / LSF `ldexp` intensity panning. (The previous pdmp3-derived
  intensity was subtly wrong.)

### Added — Layer II and Layer I (`src/mp3.cyr`)

- Subband-PCM decode ported from minimp3 (CC0): subband **bit allocation** (per
  layer/bitrate/mode alloc tables), **scalefactors**, grouped/ungrouped sample
  **(de)quantization**, feeding the shared polyphase synthesis filterbank
  (refactored into `_mp3_synth_ch`, reused by all three layers). Layer I and II
  share the code via the group size (1 vs 3).
- Verified sample-exact vs minimp3: **Layer II mono, joint stereo, and the
  low-rate allocation table all 1.00000**; **Layer I 1.00000** on a constructed
  stream. `mp3_decode` dispatches Layer I/II/III automatically.

### Tests

- Committed value-level tests (embedded fixtures): Layer II mono + stereo
  (two-tone correlation, 0.999), Layer I (correlation vs an embedded minimp3
  reference frame, 0.999), MPEG-2/2.5 Layer III mono + stereo (0.999).

### Security

- The Layer I/II path parses untrusted input; fuzzed **20,000 malformed frames,
  0 crashes** (the subband structure is bounded within the frame).

### Known limitation

- **MPEG-2.5 at 8000 Hz, low bitrate, short blocks** decodes imperfectly
  (~0.7 correlation) — a narrow bug isolated to that combination (the sfb tables
  and reservoir assembly are both verified byte-exact; the reservoir and short
  blocks each work independently, and 8000 Hz decodes correctly at higher
  bitrate). All other sample rates and block types are sample-exact. Tracked.

## [2.5.11] - 2026-07-02

**MP3 (MPEG-1 Layer III) decodes to real PCM audio** — `mp3_decode` returned an
empty sample vector (metadata only) since 2.0.0; it now produces actual samples,
verified sample-for-sample against a reference decoder (minimp3 waveform
correlation **0.99999**, lag 0) and, in the committed suite, against the source
signal (**0.999** best-lag correlation). 904 assertions (was 875, +29).

### Added — full MPEG-1 Layer III decode chain (`src/mp3.cyr`)

Ported from pdmp3 (Krister Lagerström, public domain) + ISO/IEC 11172-3. The
whole pipeline, all f64-internal:

- **Side info + bit reservoir** — `main_data_begin` back-pointer assembles main
  data from up to two previous frames (real streams carry Huffman data across
  frame boundaries); side info parsed over shravan's `bitreader`.
- **Scalefactors** — long/short/mixed blocks, `scfsi` copy-from-granule-0.
- **Huffman** — the 34 code tables as a compact binary-tree array (2804 nodes,
  generated from pdmp3.c), big-values regions (per-region `table_select`),
  count1 quadruples, `linbits`, sign bits.
- **Requantization** — `x^(4/3)` power law, global gain, per-band scalefactors,
  `subblock_gain`, `preflag`/pretab; long and short blocks.
- **Reorder** (short-block frequency-line interleave), **stereo** (MS +
  intensity, long/short), **antialias** (8-tap butterflies from `cs`/`ca`).
- **Hybrid IMDCT** — long (36) / short (12) with the four block-type windows and
  block-type switching (start/stop/short/mixed), overlap-add across granules;
  frequency inversion.
- **Polyphase synthesis filterbank** — 32→64 matrixing + the 512-tap synthesis
  window (`g_synth_dtbl`) + the 1024-sample V FIFO → 576 PCM samples/granule.

`mp3_decode` now decodes MPEG-1 Layer III (mono + stereo/joint) to PCM and falls
back to the previous metadata-only path for Layer I/II and MPEG-2/2.5 (LSF), so
nothing regresses. Large tables are CSV string literals generated from pdmp3.c;
trig tables (IMDCT/synthesis cosines, windows, `cs`/`ca`, intensity ratios) are
derived at init from formulas.

### Tests

- **Value-level correlation test** (the roadmap's acceptance rule): an embedded
  real MPEG-1-L3 mono fixture (44100 Hz, 32 kbps, 11 frames, base64 in-source +
  a tiny decoder) decodes to 12672 samples and correlates **0.999** with its
  two-tone source — the fixture exercises the bit reservoir and long/start/stop/
  short block switching. Plus a table-derivation self-test (Huffman node count,
  synthesis window, `cs`/`ca`, sfb indices, windows).

### Security

- The MPEG-1-L3 path parses untrusted input, so it was fuzzed (**50,000 malformed
  frames across two runs — plain and joint-stereo/intensity — 0 crashes**) and
  hardened. `big_values*2` (a 9-bit field, up to 1022) is clamped to the 576-line
  spectrum buffer and the Huffman region boundary indices (`r0+1`, `r0+r1+2`) are
  clamped to the 23-entry scalefactor-band table. An adversarial review against
  the reference then found three more boundary over-reads, all fixed: the top
  long-block region (band 21) and short-block region (band 12) now use
  scalefactor/pretab 0 (ISO-correct) instead of reading one past their tables,
  and intensity-stereo gates on `is_pos < 7` (0..6 valid) instead of `!= 7`, so a
  crafted 4-bit scalefactor (8..15) can't index the 6-entry `is_ratios` table out
  of bounds. A committed hostile-input test drives garbage/truncated/stereo
  frames through `mp3_decode` and asserts it returns safely.

### Deferred (MP3 tails — refinements, not blockers)

- MPEG-2/2.5 (LSF) Layer III (half-rate side info, one granule, intensity-stereo
  scalefactor compression) and Layer I/II *decode* (still metadata-only).
- A fast (FFT-based) IMDCT/synthesis (the direct cosine transforms are O(N²) —
  correct but a perf follow-up, matching the AAC/Opus note).

## [2.5.10] - 2026-07-02

**AAC encoder produces real audio** — `aac_encode` → `aac_decode` now round-trips
faithfully (mono **0.999**, stereo **0.999 / 0.999** waveform correlation), where before
it decoded to silence. Every AAC correctness/safety bug the audit found is closed. 870
assertions (was 859, +11).

### Fixed — the transform (root cause of BOTH silent encoders)

- **`fft_mdct`/`fft_imdct` were never a matched analysis/synthesis pair** (`fft_imdct`
  is a valid IMDCT — it decodes real AAC — but `fft_mdct`'s convention did not invert it,
  so encode→decode cancelled to silence). Replaced them with a verified TDAC-invertible
  direct pair (shared cosine basis, standard AAC/Vorbis convention, unity gain). The old
  `test_fft_mdct_roundtrip` only checked "output is nonzero"; it now asserts real
  windowed 50%-overlap reconstruction to `<1e-6`. Also fixed the IMDCT scale (`2/M`) so
  reconstruction is unity-gain (this made Opus decode unity-gain too). O(N²); a fast
  FFT-based pair preserving this convention is a perf follow-up.

### Fixed — AAC encoder/decoder

- **Encoder 50%-overlap framing** (window 2048, hop 1024; stop zeroing the 2nd MDCT
  half) so the decoder's overlap-add reconstructs (TDAC) — mono and stereo mid/side.
- **Invertible quantizer**: `q = round((|x|/step)^0.75)` (was `round(|x|^0.75/step)`,
  which did not invert the decoder's `q^(4/3)·step`), with **peak-based scale factors**
  so tonal peaks survive (the rms-based target ~10 collapsed single-bin tones to zero).
- **Section coding groups by codebook** (was grouping by has-data but writing the first
  band's codebook for the whole run, so a run of data bands with different codebooks was
  mislabeled and decoded as garbage — masked for a single-band tone, fatal for any
  multi-band signal). This one bug was capping mono at 0.975 and breaking stereo entirely.
- **CPE stereo bitstream**: dropped the stray 11-bit per-channel side ICS block the
  decoder never read (11-bit misalignment); the side sections now use the escape codebook
  matching the escape-coded side spectral (was copying the mid codebooks); mid/side
  overlap framing + invertible side quantizer.
- **Scale-factor DPCM predictor** tracks the transmitted (clamped) value, not the raw
  scale factor (mono + CPE) — the confirmed desync bug.
- **Decoder robustness**: reserved section codebooks 12–15 now advance the band (were an
  unguarded infinite loop = DoS on malformed input); removed the band double-increment
  for codebooks 1–4/9/10 (skipped the following band). Reject-not-hang test added.

### Tests

- Value-level round-trip tests (correlation vs. input, not sample counts): AAC mono,
  AAC stereo, and the transient/stereo/mono Opus paths. `aac_decode` of a real AAC AU
  (via MP4) still works — the new transform is standard-conformant.

### Deferred (AAC completions — refinements, not blockers)

- **TNS temporarily disabled**: the analysis-FIR/synthesis-IIR pair is exact only without
  quantization; engaged through the quantizer it amplified quant noise (0.98→0.68).
  Proper prediction-gain-gated noise shaping (+ short-window, stereo/CPE) is a follow-up.
- **Real HCB6 tables** (codebook 6 currently reuses HCB5; shravan's own encoder never
  selects it, so it only affects some third-party files) and the **psychoacoustic ATH
  floor + tonality-adjusted SMR** completion.
- Optional rate/distortion scale-factor loop (replaces the peak heuristic).

## [2.5.9] - 2026-07-02

**Opus/CELT decodes to real PCM audio** — the headline gap the 2.5.0–2.5.8 scaffolding
was built for. `opus_decode_from_packets` now returns actual samples (it returned an
empty vector before), verified by waveform correlation, not sample counts. 859
assertions (was 843, +16).

### Added

- **Magnitude synthesis** (`_opus_denorm_frame`, `src/opus.cyr`) — reconstructs MDCT
  coefficients from the decoded band energies (`2^(q/2)·band_size` sum-of-squares,
  split by width across PVQ sub-blocks) × the unit-L2 shapes. This is the missing step
  that turns the CELT "direction" into real spectral magnitude.
- **Verified TDAC MDCT/IMDCT pair** (`_opus_mdct`/`_opus_imdct`) — a self-matched
  direct cosine-basis pair (measured OLA reconstruction 0.999). Root cause found: the
  shared `fft_mdct`/`fft_imdct` in `fft.cyr` are **not a matched analysis/synthesis
  pair** — `fft_imdct` is a valid IMDCT (it decodes real AAC), but `fft_mdct`'s
  convention does not invert it, so encode→decode cancels to silence. This also
  explains a large part of the silent AAC encoder. (Fixing `fft.cyr`'s pair for the
  AAC encoder + optimizing the O(N²) direct transform is tracked for 2.5.x.)
- **Full decode path wired** — `opus_decode_from_packets` iterates the audio packets,
  decodes each CELT frame (mono via `_opus_celt_decode_mono_time`, stereo via
  `_opus_celt_decode_stereo_time` with `_opus_stereo_decouple`), IMDCTs, applies the
  synthesis sine window, and **50%-overlap-adds** (Princen-Bradley: sin²+cos²=1) into
  PCM. The encoder now frames with 50% overlap (hop = half a frame) to match.
- **Real end-to-end round-trip tests** — encode a tone/sweep, decode via the public
  `ogg_decode` path, assert best-lag normalized cross-correlation with the input:
  mono **0.997**, stereo **L 0.996 / R 0.999**, transient-containing **0.998**.

### Changed

- **Band-energy DPCM range widened 128→256 symbols** (±127 half-log2-power steps).
  The old ±63 clamp saturated the jump from a silent band to a loud tone band, so
  loud and quiet content collapsed to the same reconstructed energy (absolute levels
  were lost — steady tones only passed because correlation is scale-invariant). A
  step-amplitude signal now round-trips at 0.998 (was 0.28).
- **Rate-control complexity from the real MDCT spectrum** (`_opus_frame_complexity`) —
  fraction of spectral energy in the upper band, replacing the time-domain
  first-difference proxy (completes a 2.5.8 deferral; transients raise it naturally,
  so allocation is transient-aware). Two-pass VBR remains future work.
- **Encoder no longer truncates overflow packets** — an over-budget frame is emitted
  at its true length (Ogg carries it; each frame's range coder is independent) instead
  of cutting the trailing bytes and corrupting the whole packet.

### Deferred

- True CELT **overlapping short-window** transient coding (pre-echo reduction):
  transient frames are detected + flagged but decoded via the long MDCT for now (they
  produce correct audio, just without short-block time resolution).
- **RFC 6716 conformance** — the stream is still a bespoke shravan-internal format
  (LZMA-style range coder, uniform energy coding); it does not interoperate with
  libopus. Tracked as a dedicated 2.5.x release.

## [2.5.8] - 2026-07-01

CELT/Opus rate control + VBR — completes the CELT rate model. 843 assertions
(was 830, +13).

### Added

- **Bit-reservoir rate control + VBR** (`src/opus.cyr`):
  - `_opus_rate_new` / `_opus_rate_target` — a reservoir rate controller. CBR emits
    the nominal size every frame; **constrained VBR** lets a complex frame spend saved
    bits from the reservoir but keeps the average at/under the target bitrate (the
    reservoir stays ≥ 0, so total ≤ frames × nominal); **unconstrained VBR** sizes each
    frame purely by complexity.
  - `_opus_frame_complexity` — a cheap per-frame complexity in [0,100] (high-frequency
    first-difference energy / total energy): tones are simple, noise/transients complex.
  - `opus_encode` refactored onto `_opus_encode_impl` (CBR — behavior unchanged) and a
    new `opus_encode_vbr` (constrained VBR) that varies packet size with content.
- **Tests** (`+13` assertions): CBR target = nominal for all complexities; VBR
  monotonic in complexity (c=50 → nominal); CVBR per-frame bounded + average ≤ target
  over mixed frames; complexity(noise) > complexity(tone) + silence = 0; `opus_encode_vbr`
  yields a valid Ogg stream.

## [2.5.7] - 2026-07-01

MP4/M4A container demux — enables real `.m4a` playback. 830 assertions
(was 809, +21).

### Added

- **MP4/M4A container module** (`src/mp4.cyr`) — ISO Base Media File Format demux:
  - Box-tree parser (`mp4_find`) + navigation `moov → trak → mdia → minf → stbl`,
    picking the track whose `hdlr` handler is `soun`.
  - `mp4_audio_info` reads channels + sample rate from the `stsd` `mp4a` entry;
    `mp4_build_sample_table` computes each AAC access unit's file offset + size from
    `stsz` + `stco`/`co64` + `stsc` (sample-to-chunk).
  - `mp4_demux` returns the track info + per-sample table; `mp4_decode` wraps each AU
    in an ADTS header (`aac_build_adts_header`) and hands the stream to `aac_decode`.
  - New `FMT_MP4` format + `detect_format` recognition (`ftyp` box) + decode dispatch,
    so `codec_open`/`decode_file` handle `.m4a` transparently.
- **Tests**: a constructed minimal MP4 demuxes to the correct track info + sample
  table (`detect_format → FMT_MP4`), an MP4 wrapping a real AAC access unit decodes
  to 1024 PCM samples, and hostile inputs are rejected safely.

### Security

- The demuxer parses untrusted input, so it was adversarially reviewed and hardened:
  every attacker-controlled count/offset/size is bounds-checked against its box and
  the buffer (`stsz`/`stco`/`co64`/`stsc` counts, per-AU offset+size before `memcpy`),
  every FullBox header read (`hdlr`/`stsz`/`stco`/`stsc`) is guarded against an
  empty/truncated box, the largesize (`size==1`) read is guarded, and all
  allocations are null-checked. Malformed files return an error instead of reading
  out of bounds, over-allocating, or crashing.
  - Deferred: adding `mp4_decode` to the fuzz harness (needs the AAC chain vendored
    into the standalone harness).

## [2.5.6] - 2026-07-01

CELT stereo coupling: full two-channel bitstream (replaces the mono downmix).
809 assertions (was 799, +10).

### Added

- **Two-channel stereo CELT frame** (`src/opus.cyr`) — building on the 2.5.5
  coupling primitives, the encoder now codes stereo instead of downmixing:
  - `_opus_encode_celt_frame` gained a stereo branch: deinterleave L/R → window +
    MDCT each → `_opus_stereo_couple` → mid/side + per-band `ms_flags`; range-codes
    `isTransient` + 21 `ms_flags` bits + mid/side band energies + mid/side PVQ shapes,
    with the shape budget split evenly between the two channels. Stereo TOC bit set.
  - `_opus_decode_celt_stereo_frame` — mirror decode: reads `ms_flags` + both channels'
    energies and shapes; the caller applies `_opus_stereo_decouple` for L/R.
  - Shape encode/decode refactored to an explicit-bit-budget core
    (`_opus_encode_spectral_shape_bits` / `_opus_decode_spectral_shape_bits`) so the
    mid/side split is data-independent (both sides derive it from the packet size).
  - The mono path is unchanged.
- **Tests** (`+10` assertions): full stereo frame roundtrip (`ms_flags` + mid/side
  energies + mid/side shapes all bit-exact, couple∘decouple recovers L/R MDCT) and an
  encoder-path smoke test (stereo TOC bit + decodable packet).

### Deferred

- Stereo + transient (short blocks) combined; intensity stereo for high bands;
  stereo bit-budget beyond an even mid/side split.

## [2.5.5] - 2026-07-01

CELT stereo coupling foundation + toolchain bump to 6.3.27. 799 assertions
(was 796, +3).

### Added

- **CELT stereo coupling primitives** (`src/opus.cyr`) — the invertible core that
  replaces the mono downmix (full two-channel bitstream integration lands in 2.5.6):
  - `_opus_stereo_ms_forward` / `_opus_stereo_ms_inverse` — energy-preserving
    mid/side transform (M=(L+R)/√2, S=(L-R)/√2), exactly invertible.
  - `_opus_stereo_couple` — per-band coupled-vs-dual decision: chooses M/S when it
    concentrates energy (`min(E_M,E_S) < min(E_L,E_R)`), else dual L/R; records the
    choice in `ms_flags[band]`.
  - `_opus_stereo_decouple` — reconstructs L/R from the coupled channels + `ms_flags`;
    `couple∘decouple` is the identity for any per-band choice.
- **Tests** (`+3` assertions): M/S invertibility, per-band decision (correlated band
  → M/S), and an exact couple/decouple roundtrip over mixed correlated/uncorrelated bands.

### Changed

- **Toolchain pin bumped 6.3.25 → 6.3.27** (`cyrius.cyml`) — re-vendored stdlib via
  `cyrius lib sync`, lock refreshed. Clears the pin drift; all tests pass on 6.3.27.

## [2.5.4] - 2026-07-01

CELT transient detection + short-block MDCT. 796 assertions (was 785, +11).

### Added

- **CELT transient detection + short-window switching** (`src/opus.cyr`):
  - `_opus_detect_transient` — flags a frame whose sub-block energy sharply exceeds
    the running average of the preceding sub-blocks (or rises out of near-silence).
  - `_opus_short_mdct` — on a transient, replaces the single long MDCT with
    `CELT_SHORT_BLOCKS` (8) windowed short MDCTs whose coefficients interleave into
    the same 480-coefficient buffer (`out[f*M + b]`), localizing energy in time and
    cutting pre-echo. The band-energy + PVQ-shape path is unchanged (layout-agnostic).
  - `isTransient` coded as one range-coder bit at the head of the CELT frame; the
    decoder reads it back (new `transient_out` on `_opus_decode_celt_frame`).
- **Tests** (`+11` assertions): detector (steady tone → not transient, onset after
  silence → transient, block-0 click → transient — via a symmetric adjacent-pair
  test that catches onsets *and* decays anywhere in the frame) and a full
  transient-frame roundtrip (isTransient bit + energies bit-exact + every K>0
  sub-block shape bit-exact vs the encoder's PVQ).

## [2.5.3] - 2026-07-01

AAC psychoacoustic masking model driving scale-factor allocation. 785 assertions
(was 775, +10).

### Added

- **AAC psychoacoustic model** (`src/aac.cyr`) — simultaneous-masking model that
  coarsens the quantization of masked bands:
  - `_aac_psy_init` precomputes an asymmetric spreading function over scalefactor
    bands (which approximate critical bands): gentle toward higher frequency
    (`PSY_SLOPE_UP` 10 dB/SFB), steep toward lower (`PSY_SLOPE_DOWN` 25 dB/SFB).
  - `_aac_psy_masking_offsets` spreads each band's energy to its neighbours and
    derives a per-band coarseness offset `2·log2(spread/energy)` (0 for an isolated
    band, positive where neighbours mask it, capped at `PSY_MAX_OFFSET` 24).
  - The encoder adds the offset to the baseline scale factor, so masked bands are
    quantized coarser (fewer bits) while unmasked bands are unchanged. Conservative
    by construction — no change to the stereo/CPE path or the decoder.
- **Tests** (`+10` assertions): isolated band → no masking, neighbour masking,
  distance falloff, louder-masker → larger offset, upward/downward asymmetry, and a
  full mono encode→decode frame roundtrip through the masking-driven scale factors.

## [2.5.2] - 2026-07-01

Toolchain modernization (cyrius 6.3.25), the serde Str-deserialize repair, and
AAC TNS (Temporal Noise Shaping). 775 assertions (was 748, +27). The
psychoacoustic model moves to 2.5.3.

### Added

- **AAC TNS (Temporal Noise Shaping)** (`src/aac.cyr`) — AAC-LC long-window,
  mono/SCE path. Linear prediction across frequency flattens the temporal
  envelope so quantization noise tracks transients (reduces pre-echo):
  - Encoder: `_tns_autocorr` → `_tns_levinson` (reflection coefs + prediction-gain
    gate) → quantize (`_tns_quant_coef`) → analysis FIR (`_tns_filter`) over the top
    spectral bands before quantization; `_aac_write_ics_ext` emits the AAC-LC ICS
    extension (`pulse`/`tns`/`gain` presence + `tns_data()`).
  - Decoder: `_aac_parse_ics_ext` / `_tns_parse_data` read `tns_data()`; `_aac_synth`
    applies the synthesis IIR (`_tns_synth_apply`) to the dequantized spectrum before
    the IMDCT. Analysis and synthesis are built from the *same* quantized reflection
    coefficients, so the filter pair is an exact inverse — TNS shapes only quant noise.
  - Stereo/CPE path is untouched (TNS gated to the mono encoder; CPE decode passes a
    zeroed state), keeping that bitstream byte-identical.
- **Tests** (`+27` assertions): filter analysis/synthesis identity (both directions),
  coefficient-quantization roundtrip, `tns_data()` bitstream roundtrip, encoder-analyze
  → decoder-synth exact recovery (TNS engaged), and a full mono encode→decode frame
  roundtrip proving the stream stays in sync.

### Changed

- **Toolchain pin bumped 6.3.19 → 6.3.25** (`cyrius.cyml`) — re-vendored stdlib
  via `cyrius lib sync`; `cyrius.lock` refreshed. Clears the pin-vs-cycc drift
  warning. All 748 assertions pass, fuzz 90K/0, `cyrius vet` clean on 6.3.25.
- **Retired the serde Str-deserialize workaround** (`src/serde.cyr`) — cycc 6.3.25
  fixes `#derive(Serialize)`'s generated `_from_json` for `Str` fields, so
  `ShrAudioMetadata` now round-trips through the derived `ShrAudioMetadata_from_json`;
  the hand-written `audio_metadata_from_json` / `_meta_get_str` (2.4.1) are gone.
  Resolves cyrius issue `2026-07-01-derive-serialize-str-field-deserialize-broken`.

## [2.5.1] - 2026-07-01

shravan's first CELT decode: a matched, provably-invertible range coder, PVQ
shape decode, and a full-stream encode→decode roundtrip proving the entropy
stream inverts exactly (band energies bit-exact, spectral shapes bit-exact).
748 assertions (was 727, +21).

### Added

- **CELT decode path** (`src/opus.cyr`) — inverse of the 2.5.0 encode path:
  - `opus_range_dec_init` / `opus_range_dec_uint` / `opus_range_dec_bit` — a
    byte-oriented range decoder matched to a rewritten LZMA-style encoder.
  - `_pvq_decode_band` — reads a CWRS index over `[0, V(N,K))` and expands it to
    the pulse vector (inverse of `_pvq_encode_band`).
  - `_opus_decode_band_energies` — inverse DPCM of the band-energy sub-stream.
  - `_opus_decode_spectral_shape` — walks bands/sub-blocks in lock-step with the
    encoder (same data-independent K budget) and reconstructs each unit-L2 shape.
  - `_opus_decode_celt_frame` — TOC parse (CELT/FB config 31) → energies + shape.
- **Tests** (`+21` assertions): range-coder roundtrip (mixed uint/bit incl.
  edge cases 0/3 and 65535/65536), PVQ band symbol-exact roundtrip, band-energy
  DPCM roundtrip, and a full-stream CELT frame roundtrip asserting bit-exact
  energies and bit-exact per-sub-block shapes.

### Changed

- **Range coder rewrite** (`src/opus.cyr`) — replaced the custom forward coder
  (which was not bit-exact invertible) with a standard LZMA-style carry coder
  (one-byte cache + deferred `0xFF` run), keeping the public `opus_range_enc_*`
  names so existing call sites are unchanged. Opus encoder output bytes change.
- **Band-energy DPCM is now closed-loop** — the encoder's predictor tracks the
  reconstructed (clamped) value instead of the raw quantized energy, so the
  decoder stays in sync even when a delta saturates.

## [2.5.0] - 2026-07-01

Full PVQ (Pyramid Vector Quantization) spectral shape for the CELT encoder,
replacing the sign-only stub — the CELT quality root the rest of the Opus
encoder work (2.5.x) allocates bits against. 727 assertions (was 610, +117).

### Added

- **PVQ spectral shape** (`src/opus.cyr`) — each band's unit-L2 shape is
  quantized to an integer pulse vector `y` (`sum|y_i| = K`) coded by its CWRS
  index:
  - `V(N,K)` pyramid-count grid (saturating, range-coder-safe) + `_pvq_bits` /
    `_pvq_choose_k` (K from a bit budget).
  - `_pvq_index_encode` / `_pvq_index_decode` — bijective CWRS index ↔ vector,
    roundtrip-tested (exhaustive N=2,K=2 + (3,2)/(4,3)/(2,10)/(1,3); out-of-range
    decode rejected).
  - `_pvq_search` — greedy `Rxy²/Ryy` pulse search; `_pvq_denormalize` shape
    reconstruction. Reconstructed shape cosine **0.9798 vs sign-only's 0.800**.
  - Wired into `_opus_encode_spectral_shape`: per-band split to `N ≤ 32`,
    data-independent K budget (derived from packet size + band geometry so a
    future decoder computes identical K).
  - 4 test functions (+117 assertions → 727).

### Fixed

- **TOC config 30 → 31.** `_opus_encode_celt_frame` labeled its 20 ms CELT/FB
  frames as config 30 (which is 10 ms); corrected to config 31.

### Deferred

- **Full packet encode→decode stream roundtrip → 2.5.1.** Needs a CELT decoder
  (mirrored range decoder + `_opus_decode_spectral_shape`); shravan has no CELT
  audio decode today (`opus_decode_from_packets` is header-only). The PVQ
  quantizer itself is self-consistently roundtrip-validated at the CWRS/shape
  level (above).

## [2.4.4] - 2026-07-01

Opus encoder **framework** — the opening/foundational work for a full Opus
encoder. The feature set (SILK, hybrid, VBR, PVQ, stereo, transient) is
roadmapped as 2.5.x; this release lands only the scaffolding, additive to the
existing CELT-mode encoder. 610 assertions (was 563, +47).

### Added

- **Opus encoder framework** (`src/opus.cyr`, additive — the existing
  `opus_encode` Ogg path is untouched):
  - `OpusEncoder` config/state struct + `OpusMode`/`OpusBandwidth`/`OpusSignal`/
    `OpusApp` enums.
  - `opus_encoder_new` + setters (bitrate / vbr / bandwidth / mode).
  - Mode + bandwidth **selection logic** (`opus_select_mode`,
    `opus_select_bandwidth`, `opus_bandwidth_cap`) — bitrate thresholds + a
    sample-rate cap.
  - RFC 6716 §3.1 **TOC byte** (`opus_toc_config` / `opus_toc_byte`, config 0–31).
  - `opus_encode_frame` dispatch — routes CELT to the existing encoder;
    SILK/HYBRID return `ERR_UNSUPPORTED_FMT` (documented 2.5.x seams).
  - 5 test functions (+47 assertions).
- **Design doc** `docs/adr/0001-opus-encoder-framework.md` (architecture + 2.5.x plan).
- **2.5.x roadmap** — full Opus encoder sequenced (PVQ → transient → stereo →
  rate-control/VBR → SILK → hybrid), with the 2.3.x deferrals + hi-res/DSD interleaved.

### Known issues

- `_opus_encode_celt_frame` hardcodes TOC config 30 (CELT/FB 10ms) for its 20ms
  frames (should be config 31, which `opus_toc_byte` computes); logged as a 2.5.0
  cleanup — it needs a decode round-trip guard, so it's kept out of this additive
  release.

## [2.4.3] - 2026-07-01

Distlib bundle for consumers + a source-layout cleanup. No codec behavior
changes; the 563-assertion test build is unchanged.

### Added

- **`dist/shravan.cyr` — the consumable distlib bundle** (`cyrius distlib`).
  Concatenates the library (`src/shravan.cyr`) + all codec modules into one
  self-contained file (0 unresolved `include`s). Consumers `include` it and
  supply stdlib + `bayan` + `sankoch` from their own manifest, then call
  `shravan_init_constants()`. Verified end-to-end by a consumer smoke test
  (encode → decode → detect through the bundle alone). Declared via `[lib]`
  in `cyrius.cyml`; committed under `dist/`.

### Changed

- **Source layout: `src/main.cyr` split into library + test harness.**
  `src/shravan.cyr` now holds the library (error/format/pcm/wav/aiff/alac +
  codec dispatch + `decode_file`/`decode_reader`); `src/main.cyr` is the test
  harness that `include`s it + the codecs and runs the suite.
- **Codec modules relocated `lib/` → `src/`** (flac, ogg, mp3, tag, fft, opus,
  aac, resample, dither, simd, stream, serde). `lib/` is now **only** the
  vendored Cyrius stdlib — `cyrius distlib` no longer mistakes shravan's own
  modules for external stdlib leaves, and `cyrius vet`/audit is cleaner.
  Updated: `main.cyr`/`bench.cyr`/`fuzz` includes, CI security scan (`src/` +
  `fuzz/` sweep), `cyrius.lock` (31 entries — stdlib only), and docs.

## [2.4.2] - 2026-07-01

Stabilization — rebuild the fuzz harness for the 6.3.19 toolchain. No codec or
serde changes; the 563-assertion suite is unchanged.

### Fixed

- **Fuzz harness (`fuzz/fuzz_codecs.cyr`) builds + runs under Cyrius 6.3.19.**
  6.3.19's stricter linker refuses *reachable* undefined fns; the standalone
  harness referenced `src/main.cyr` helpers it didn't include. Added the missing
  self-contained helpers (`fmtinfo_format`/`sample_rate`/`channels`/`bit_depth`,
  `decode_result_info`/`samples`, `write_u32_le`, `read`/`write_u32_be`, and an
  unreachable `opus_decode_from_packets` stub) and corrected the `ogg_parse_page`
  (4-arg) / `mp3_scan_frames` (2-arg) call sites. Clean build, **90,000 calls /
  0 crashes** via `./fuzz/run.sh`.

### Changed

- **CI** gains `cyrius vet` (include-dependency audit — 0 untrusted / 0 missing)
  and a fuzz smoke pass (1000 iters, gated on "0 crashes").

## [2.4.1] - 2026-07-01

Full serde type coverage — restores the complete Rust `#[derive(Serialize,
Deserialize)]` surface as JSON serialization. **563 assertions pass** (539 + 24).

### Added

- **Enums** (value-based JSON, like AudioFormat): `MpegVersion`, `MpegLayer`,
  `ChannelMode` (mp3), `ResampleQuality` (resample) — `_to_json`/`_from_json`.
- **Int-field structs** via `#derive(Serialize)`: `ShrAlacConfig` (10 fields),
  `ShrMp3FrameInfo` (7), `ShrOpusHead` (6) — full roundtrip.
- **`ShrAudioMetadata`** (7 `Str` tag fields: title/artist/album/track_number/
  year/genre/comment). `to_json` via `#derive(Serialize)`; roundtrip verified.
- **Codec markers** `WavCodec`…`AlacCodec` (8) — serialize their identity
  (`{"codec":"…"}`).
- Roundtrip tests for every type (+24 assertions → 563).

### Fixed / worked around

- **`#derive(Serialize)` `Str`-field deserialize is broken on cyrius 6.3.x** —
  the generated `_from_json` yields garbage for `Str` fields (`to_json` is fine;
  int fields roundtrip fine). Filed upstream (cyrius issue
  `2026-07-01-derive-serialize-str-field-deserialize-broken`, with repro).
  shravan ships a hand-written `audio_metadata_from_json` (bayan
  `json_get`, 8-byte `Str`-handle fields) as the stopgap — remove once the
  derive is fixed.

## [2.4.0] - 2026-07-01

Language/toolchain modernization to Cyrius **6.3.19** (from cc3 4.10.3). No
codec behavior changes, no dependency removals; the previously-orphaned JSON
serialization module is now live. **539 assertions pass** (520 + 19 serde).

### Added

- **`serde` metadata serialization wired in.** `lib/serde.cyr` (a partial port
  of Rust's `#[derive(Serialize, Deserialize)]` usage, present-but-orphaned
  since 2.0.0) is now `include`d and tested. `ShrFormatInfo_to_json`/`from_json`
  are generated by Cyrius 6.x's `#derive(Serialize)` (unavailable under 4.10.3,
  which is why it never linked before); `AudioFormat`/`PcmFormat`/`ShravanErr`
  keep their hand-written `_to_json`/`_from_json`. 5 tests join the suite
  (+19 assertions → 539). Full Rust type coverage tracked for **2.4.1**.
- **`bayan` stdlib dep** — the modern home of the JSON API (`bayan_json_parse`/
  `bayan_json_get_int`), which serde uses. Replaces the old `json` stdlib leaf,
  removed upstream at 6.1.25 (carved into `bayan`).

### Changed

- **Toolchain: cc3 4.10.3 → Cyrius 6.3.19.** The pin now lives in
  `cyrius.cyml [package].cyrius` (single source of truth read by CI); the
  `.cyrius-toolchain` file is removed (retired upstream at 5.11.5).
- **Manifest: `cyrius.toml` → `cyrius.cyml`.** `version = "${file:VERSION}"`
  interpolates the `VERSION` file; adds `language = "cyrius"` and the toolchain
  pin. `[build]` entry/output are metadata; the codec `include`s are unchanged.
- **Stdlib vendoring via `cyrius lib sync`.** `lib/` now holds the
  version-matched 6.3.19 stdlib snapshot (28 files incl. platform variants),
  replacing the stale hand-vendored 4.10.3 copies that shadowed the toolchain.
  `cyrius.lock` records per-file hashes (committed).
- **`math` split: `ganita` added to `[deps].stdlib`.** The transcendentals
  shravan calls (`f64_pow/asin/acos/atan2/sinh/cosh/tanh/hypot`) moved out of
  `math` into `ganita` in the 6.x line; `f64_sin/cos/sqrt/exp/ln/…` are builtins.
  `math`, `tagged`, and the `sankoch` compression dependency are **retained**.
- **CI/release workflows modernized.** Toolchain installed via the canonical
  `install.sh` reading the `cyrius.cyml` pin (no hardcoded version); build flow
  is `cyrius lib sync` → `cyrius deps` → `cyrius build` → tests/bench; actions
  SHA-pinned; security scan scoped to shravan-authored modules.
- **`.gitignore`**: ignore `build/` only; keep `cyrius.lock`, `lib/`, and
  `bench-history.csv` tracked. Rust-era cruft dropped.

### Fixed

- **`file_open` arity** — 6.x signature is `file_open(path, flags, mode)`;
  `decode_file` now passes the mode argument.
- **`flac_stream_new` call arity** — dropped the silently-ignored `chunk_frames`
  argument (the FLAC streaming decoder is full-buffer by design).
- **Symbol collisions with the modern stdlib**: dropped shravan's `F64_HALF`
  (now from `math.cyr`); dropped shravan's `is_err`/`err_code` (byte-identical to
  the `syscalls` stdlib, pulled via `io`); renamed the packed-Result `is_ok` →
  `res_ok` to avoid clashing with `result.cyr`'s tagged-Result `is_ok`. The
  `sankoch`/audio `detect_format` last-wins warning is unchanged from baseline.
- **Stale `v2.0.0` startup/benchmark banners** → `v2.4.0`.

## [2.3.2] - 2026-04-15

### Added

- **AAC per-band codebook selection**: Encoder now selects the most efficient Huffman codebook per scale factor band based on max quantized magnitude. Codebook 1-2 for |q|<=1, 3-4 for |q|<=2, 5-6 for |q|<=4, 7-8 for |q|<=7, 9-10 for |q|<=12, 11 for escape. Quad codebooks used when band width is divisible by 4.
- **AAC M/S stereo encoding**: Stereo input now encoded as CPE (Channel Pair Element) with Mid/Side transform (M=(L+R)/2, S=(L-R)/2) and ms_mask_present=2 (all bands). Side channel gets own MDCT, quantization, section coding, scale factors, and spectral data. Previously downmixed to mono SCE.
- **AAC VBR mode**: `aac_encode_vbr(samples, rate, channels, quality, out)` with quality levels 1-5 (64-320 kbps). Maps quality to target bitrate and delegates to CBR encoder.
- **AAC spectral encoding per-codebook**: Encoder now writes spectral data using the selected codebook (1-10 + escape) instead of always using escape pairs (cb=11). Signed quads, unsigned quads with sign bits, signed pairs, unsigned pairs with sign bits all supported.

## [2.3.1] - 2026-04-15

### Added

- **AAC spectral codebooks 9-10**: ISO 14496-3 HCB9/HCB10 (169 entries each, 13x13 unsigned pairs, values 0-12). Full Huffman decode with 2-level LUT + sorted fallback. Wired into spectral decoder dispatch.
- **AAC 4-tuple decoder**: `_aac_decode_spectral_quad()` for codebooks 1-4. Decodes 4 spectral values per Huffman symbol. HCB1/2 (signed quads, values -1,0,1), HCB3/4 (unsigned quads with sign bits, values 0,1,2). Dispatch wired into spectral decoder for bands using codebooks 1-4.
- **AAC codebook 1-4 table data**: ISO 14496-3 HCB1-4 (81 entries each, 3^4 4-tuple codebooks). Tables loaded with 2-level LUT decode.

## [2.3.0] - 2026-04-15

### Added

- **`decode_file(path)`**: Read audio file from disk, auto-detect format, decode. Returns decode_result or error.
- **`decode_reader()` / `decode_reader_feed()` / `decode_reader_flush()`**: Streaming decoder with format auto-detection on first feed. Dispatches to WAV/FLAC/AIFF stream decoders.
- **AAC 2-level Huffman lookup**: 256-entry level-1 table for O(1) decode of short codes (<=8 bits), sorted-scan fallback for long codes. Applied to SCF and all spectral codebooks.
- **AAC codebook 1-4 dispatch**: Band skipping for 4-tuple codebooks (structural framework, tables deferred to v2.3.1).
- **AAC codebook 9-10 dispatch**: Unsigned pair decode (13x13 grid) wired into spectral decoder (tables deferred to v2.3.1).

### Security

- **Full security audit**: 21 findings (7 P0, 5 P1, 6 P2, 3 P3) — all fixed. Report: `docs/audit/2026-04-15-security-audit.md`.
- **P1 fixes**: FLAC unary decode bound (1M→32768), ALAC INT64_MIN guard, MP3 frame_size OOB guard, AAC frame count cap (65536), FLAC SEEKTABLE cap (1024).
- **P2 fixes**: Vorbis Comment zero-length/count cap, MDCT size validation (n>=4, n%4==0, n<=8192), bitreader parameter validation (0..64), `_safe_alloc_mul()` overflow-checked allocator (256MB cap), tag COMM validation.
- **Fuzzing harness**: `fuzz/fuzz_codecs.cyr` with `fuzz/run.sh` — FLAC metadata, Ogg page parse, MP3 frame scan, ID3v2 tag read, random bytes. 90,000 calls, 0 crashes.
- **Upstream filed**: 3 Cyrius stdlib issues for v5.0.1 (alloc overflow, vec capacity overflow, allocation size cap).
- 520 tests, 0 failures

## [2.2.0] - 2026-04-15

### Added

- **PCM reciprocal constants**: Pre-computed `F64_RCP_32768`, `F64_RCP_8388608`, `F64_RCP_128`, `F64_RCP_2147483648` for mul-instead-of-div in all PCM conversion hot paths. Pre-computed `F64_2_NEG_149` for f32 denormal conversion (was calling `f64_pow` per sample).
- **Vec pre-allocation**: `_vec_new_cap(n)` helper allocates vectors with known capacity, eliminating growth during PCM decode.
- **FFT twiddle pre-computation**: Per-recursion-level twiddle factor tables in `_fft_forward`. MDCT/IMDCT pre/post twiddle tables cached and reused across calls. Eliminates `f64_cos`/`f64_sin` from inner loops.
- **AAC sorted Huffman decode**: `_aac_huff_decode_fast` uses sorted-by-length codebook tables with skip-ahead, replacing linear scan of all entries per bit. Applied to SCF (121 entries) and spectral codebooks 5-8 (64-81 entries).
- **MDCT N/2-point FFT**: Rewrote `fft_mdct` and `fft_imdct` to use N/2-point (forward) and N/4-point (inverse) complex FFTs with folding and pre/post rotation, replacing the 2N-point zero-padded approach. 4x reduction in FFT size for AAC/Opus transforms.
- **Resample polyphase filter table**: Pre-computes 256-phase windowed sinc kernel table, replacing per-sample `f64_sin`/`f64_cos` calls (65 trig calls per sample for BEST quality → 0).
- **Struct size constants**: `FMTINFO_SIZE`, `DECODE_RESULT_SIZE`, `BITREADER_SIZE` replace magic numbers.

### Security (P0 — from 2026-04-15 audit)

- **SEC-001**: WAV chunk size overflow — added bounds guard before pos advancement.
- **SEC-002**: AIFF SSND offset underflow — validated `ssnd_offset <= ck_size - 8` and `pcm_start <= len`.
- **SEC-003**: AIFF chunk pos overflow — added overflow guard matching WAV fix.
- **SEC-004**: WAV extensible format OOB — added `pos + 34 <= len` guard.
- **SEC-005**: FLAC block_size cap — enforced `block_size <= 65535` in `flac_decode_block_size()`.
- **SEC-006**: FLAC metadata block OOB — validated `pos + block_size <= len` before advancing.
- **SEC-007**: Ogg packet accumulation overflow — added integer overflow check and 16MB hard cap.
- Full report: `docs/audit/2026-04-15-security-audit.md` (14 deferred P1-P3 items on v2.3.0 roadmap, 3 upstream issues filed for Cyrius 5.0.1).

### Changed

- **Compiler requirement**: cc3 >= 4.10.3 (from 3.4.3). Gains linalg, security fixes, improved stdlib.
- **Stdlib modernization**: Removed 12 vendored stdlib files from `lib/`. Stdlib modules now resolved via `[deps] stdlib` in `cyrius.toml` (matches modern Cyrius project pattern). `lib/` now contains only project-specific codec modules.
- **Sankoch compression dep**: Added sankoch 1.0.0 (LZ4, DEFLATE, zlib, gzip) via `[deps.sankoch]`. Provides `compress()`, `decompress()`, `detect_format()` for container format support.
- **Stdlib upgrades**: math.cyr gains inverse trig (asin, acos, atan2), inverse hyperbolic, lerp/hypot/sign/trunc/fract, gcd/lcm/fibonacci/binomial, f64_parse. string.cyr gains word-at-a-time strlen, rep movsb memcpy/memset, ASCII case helpers. str.cyr gains type annotations + 15 extended functions. fmt.cyr gains fmt_int_fd, efmt_int, bounds-checked fmt_sprintf.
- **WAV chunk parsing**: Replaced 12 nested single-byte comparisons with `read_u32_le` for RIFF, WAVE, fmt, data chunk detection.

### Performance

- **MDCT forward 2048: 8.534ms → 1.566ms (5.45x faster)** — N/2-point FFT rewrite
- WAV decode 1sec i16: 1.135ms → 766us (1.48x faster)
- PCM i16→f64 4096: 93.8us → 70.1us (1.34x faster)
- PCM i24→f64 4096: 94.8us → 67.3us (1.41x faster)
- PCM u8→f64 4096: 78.5us → 50.3us (1.56x faster)
- FFT forward 1024: 1.714ms → 1.528ms (1.12x faster)
- FLAC encode/decode: stable (not affected by FFT/PCM changes)
- Binary: 475KB (from 333KB in v2.1.1 — sankoch + polyphase tables)
- 520 tests, 0 failures

## [2.1.1] - 2026-04-11

### Added

- **AAC spectral Huffman codebooks 5-8**: HCB5 (signed pairs, 81 entries), HCB7/HCB8 (unsigned pairs, 64 entries each). Decoder dispatches by codebook number — codebooks 5-8 use ISO standard tables, codebook 11 uses existing escape pair decoder. Codebooks 1-4 and 9-10 skip bands (not yet implemented).
- **AAC short window support**: EIGHT_SHORT_SEQUENCE (window_seq=2) with 8x MDCT-256 transforms, sine windowing, sub-window overlap-add. Short SWB offset table (14 bands for 48kHz). Scale factor grouping parsed.
- **Huffman decoder**: general-purpose `_aac_huff_decode()` — bit-by-bit accumulation with linear codebook search. Used by scale factor and spectral codebook decoders.

### Changed

- **AAC section coding**: now uses correct escape values for short windows (3-bit length, max 7) vs long windows (5-bit length, max 31).
- Binary: 333KB (from 313KB in v2.1.0)

## [2.1.0] - 2026-04-11

### Added

- **AAC-LC decoder**: from-scratch implementation — bitreader, ICS/section/scale factor parsing, escape-pair spectral decode, inverse quantization (|q|^4/3), IMDCT via fft.cyr, sine window overlap-add. Encode/decode roundtrip verified.
- **AAC M/S stereo**: CPE (Channel Pair Element) parsing, common window flag, per-band and all-band M/S mask, stereo reconstruction (L=M+S, R=M-S). Stereo roundtrip tested.
- **AAC scale factor Huffman codebook**: ISO 14496-3 standard codebook (121 entries). Both encoder and decoder now use real Huffman codes instead of simplified 7-bit fixed encoding.
- **Metadata writing**: `tag_write_id3v2()` and `tag_write_vorbis()` — write/read roundtrip tested for title, artist, album, year, genre fields.
- **FLAC incremental streaming**: `flac_stream_feed` now decodes frame-by-frame via `flac_decode_range` with sample offset tracking, emitting SAMPLES events incrementally instead of buffering everything.
- **FLAC SEEKTABLE parsing**: `flac_parse_metadata` now parses SEEKTABLE blocks (type=3) into SeekPoint structs for fast seeking.
- **FLAC decode_range()**: sample-accurate seeking with SEEKTABLE support.
- **Opus decode path**: `opus_decode_from_packets` parses OpusHead/OpusTags, scans granule positions for duration. `ogg_decode` dispatches to Opus on OpusHead magic.
- **Opus parse_tags()**: reads Vorbis Comment metadata from OpusTags packets.
- **codec_open ALAC/Opus dispatch**: `codec_open` now dispatches all 8 formats including ALAC and Opus.
- **PCM i24 unpacked conversions**: `i24_to_f64()` and `f64_to_i24()` for unpacked i32-stored 24-bit samples.
- **Stream chunk_size**: WAV and AIFF streaming decoders now use `chunk_frames` parameter — emit multiple SAMPLES events of controlled size.
- 21 new test assertions (520 total, 0 failures)

### Changed

- **Compiler requirement**: cc3 >= 3.4.3 (tok_names overflow fix)
- **Rust source removed**: `rust-old/` deleted, preserved at git tag `1.1.0`
- **CI workflows**: bumped to Cyrius 3.4.3, added format check, security scan jobs
- **Benchmarks**: 9 benchmarks (WAV encode/decode, PCM i16/i24/u8, FFT 1024, MDCT 2048, FLAC encode/decode)

### Performance

- FLAC decode 1sec i16: 8.0ms (vs Rust 1.53ms — 5.2x)
- FLAC encode 1sec i16: 20.7ms (vs Rust 2.77ms — 7.5x)
- WAV decode 1sec i16: 2.0ms (vs Rust 47µs — 42x)
- Compile: 420ms full rebuild (vs Rust 6.9s — 16x faster)
- Binary: 313KB (vs Rust 1.83MB — 6x smaller)

## [2.0.0] - 2026-04-10

Full rewrite from Rust to Cyrius. Zero external dependencies.

### Changed

- **Language**: Rust → Cyrius (self-hosting systems language, zero LLVM/crate dependency)
- **Sample format**: f32 → f64 internally (native Cyrius SSE2, higher precision)
- **Error handling**: thiserror enum → packed Result (negative = error, bit 63 set)
- **Build system**: Cargo → `cyrius build` (211ms full compile, 253KB static ELF)
- **Architecture**: Library crate → include-based modules (`lib/*.cyr`)
- Rust implementation archived in `rust-old/` (removed in v2.1.0, preserved at tag `1.1.0`)

### Preserved

- All 16 modules ported: error, format, pcm, codec, wav, aiff, alac, flac, ogg, mp3, tag, fft, opus, aac, resample, dither, simd, stream
- Full encode/decode for WAV, FLAC, AIFF, Opus, AAC
- Decode-only for ALAC, MP3 (header parsing)
- Ogg container parsing/muxing, metadata tag reading (ID3v2, Vorbis Comment)
- Mixed-radix FFT, MDCT/IMDCT, sinc resampler, TPDF + noise-shaped dithering
- Streaming decoders for WAV, FLAC, AIFF
- 415 test assertions, all passing

### Performance

- WAV decode 1sec i16: 1.1ms (vs Rust 22.8µs — ~50x, expected without LLVM optimizer)
- PCM i16→f64 4096: 93µs (vs Rust 429ns — ~217x, scalar vs auto-vectorized)
- Compile: 211ms (vs Rust 5.1s — 24x faster)
- Binary: 253KB (vs Rust 1.8MB — 7x smaller)

## [1.1.0] - 2026-04-02

Four new codecs to eliminate tarang's C FFI audio codec dependencies.

### Added

- **AAC-LC decoder**: ADTS parsing + symphonia-codec-aac backend, mono through 7.1 channels. Feature: `aac` (requires `std`)
- **AAC-LC encoder**: From-scratch MDCT-based encoder producing ADTS bitstream. Per-band quantization, scale factor Huffman coding, escape-pair spectral coding. Mono/stereo, all standard AAC sample rates, CBR. Feature: `aac`
- **Opus CELT encoder**: From-scratch CELT-mode encoder producing valid Ogg/Opus files. Mono/stereo, 48 kHz, CBR 32-256 kbps, 20ms frames. Feature: `opus`
- **ALAC decoder**: From-scratch Apple Lossless decoder for raw frames from MP4. 16/20/24/32-bit, mono/stereo, LPC prediction, adaptive Rice-Golomb coding, stereo de-matrixing. `no_std` compatible. Feature: `alac`
- **Ogg muxer**: Page construction with CRC-32, lacing, BOS/EOS flags
- **Mixed-radix FFT** (`src/fft.rs`): Shared O(N log N) FFT (factors of 2, 3, 5) and MDCT, used by Opus and AAC encoders
- `AudioFormat::Aac`, `AudioFormat::Alac` variants with ADTS format detection
- `AacCodec`, `AlacCodec` structs implementing `AudioCodec` trait
- `AlacConfig` for parsing ALACSpecificConfig from MP4 extradata
- `#[must_use]` on all 22 public `Result`-returning functions
- Opus encode benchmark

### Fixed

- ADTS/MP3 format detection correctly distinguishes AAC (layer=0) from MP3 (layer!=0)
- Opus encoder TOC byte reflects mono-coded bitstream regardless of input channel count

## [1.0.1] - 2026-03-28

### Fixed
- `streaming` feature now compiles without requiring `flac` or `aiff` features — `FlacStreamDecoder` and `AiffStreamDecoder` are properly gated behind their respective feature flags

## [1.0.0] - 2026-03-28

### Added

- **Reference implementation tests**: Validate WAV, FLAC, and AIFF decode against ffmpeg-generated reference files. Cross-format consistency checks (WAV vs FLAC vs AIFF from same source).
- **WAVE_FORMAT_EXTENSIBLE support**: WAV decoder now handles format code 0xFFFE with SubFormat GUID extraction, enabling 24-bit and multi-channel WAV files from professional tools.
- Performance validated within 2x of C reference implementations (libFLAC, dr_wav)
- Test coverage: 85%+ line coverage (90%+ excluding platform-conditional dead code)

## [0.5.0] - 2026-03-28

### Changed

- **SIMD-accelerated resampling**: Inner kernel loop uses `simd::weighted_sum()` when `simd` feature is enabled, replacing manual f64 accumulation with vectorized f32 path
- **Multi-channel resampling optimization**: Deinterleave → per-channel resample → reinterleave for sequential memory access. Significant improvement for stereo and multi-channel audio
- **Async I/O**: `StreamDecoder::feed()` is async-compatible by design (non-blocking, caller-driven). No runtime dependency needed — callers use their async runtime to drive the streaming trait.

## [0.4.0] - 2026-03-28

### Added

- **Streaming decoders**: `StreamDecoder` trait with chunk-at-a-time `feed()`/`flush()` API, `StreamEvent` enum (`Header`, `Samples`, `End`)
- **WavStreamDecoder**: Streaming WAV decoder with configurable chunk size, state-machine header parsing, incremental PCM conversion
- **FlacStreamDecoder**: Streaming FLAC decoder using `decode_range()` with sample offset tracking to avoid duplicate emission
- **AiffStreamDecoder**: Streaming AIFF/AIFF-C decoder with big-endian PCM, `sowt` little-endian support
- **`decode_reader()`**: Read entire `std::io::Read` stream and auto-detect/decode (std-only)
- **`decode_file()`**: Read file from path and auto-detect/decode (std-only)
- Feature gate: `streaming` (requires `std`)

## [0.3.0] - 2026-03-28

### Added

- **Ogg container parser**: Page parsing, packet extraction, CRC-32 verification, continuation page handling, codec detection (delegates to Opus)
- **AIFF decoder/encoder**: FORM/AIFF and FORM/AIFC parsing, COMM chunk with 80-bit extended float sample rate, SSND chunk, big-endian PCM 8/16/24/32-bit decode and encode
- **MP3 frame sync**: Frame header parsing (MPEG 1/2/2.5, Layer I/II/III), bitrate and sample rate tables, frame size calculation, ID3v2 tag skipping, multi-frame scanning
- **Opus header parsing**: OpusHead identification header, OpusTags comment header (via Ogg container), duration from granule position
- Format detection for Ogg (`OggS`), AIFF (`FORM....AIFF`), AIFF-C (`FORM....AIFC`), MP3 (ID3v2 or MPEG sync word)
- Codec structs: `OggCodec`, `AiffCodec`, `Mp3Codec`, `OpusCodec` (all with Serialize/Deserialize)
- Feature gates: `ogg`, `aiff`, `mp3`, `opus` (opus depends on ogg)

### Fixed

- MP3 frame size calculation for Layer II with MPEG-2/2.5 (used wrong samples-per-frame divisor)
- Dither functions now clamp `target_bits` to 1..=32 instead of panicking on 0

## [0.2.1] - 2026-03-28

### Added

- **SIMD-accelerated PCM conversion**: SSE2 (x86_64) + NEON (aarch64) kernels for i16/f32 conversion and weighted dot product, behind `simd` feature gate
- **Dithering module**: TPDF and noise-shaped dithering for bit-depth reduction, behind `dither` feature gate
- **Extended PCM conversions**: `f64_to_f32`, `f32_to_f64`, `u8_to_f32`, `f32_to_u8`

## [0.2.0] - 2026-03-28

### Added

- **FLAC encoder**: Fixed prediction (orders 0-4) with automatic order selection, Rice entropy coding with optimal parameter selection, mid-side stereo channel decorrelation, MD5 signature computation
- **LPC subframe decoding**: Full support for LPC orders 1-32 with quantized coefficients and variable precision
- **CRC verification**: CRC-8 (frame header) and CRC-16 (full frame) validation on decode, correct CRC emission on encode
- **FLAC seeking**: `decode_range()` for sample-accurate seeking, SEEKTABLE metadata block parsing, range-based decoding with start/end sample positions
- **BitWriter**: MSB-first bitstream writer for FLAC frame construction
- FLAC encode/decode benchmarks (Criterion)

### Fixed

- `resample()` now rejects `source_rate=0` instead of panicking with capacity overflow
- WAV chunk parser uses saturating arithmetic to prevent overflow on malicious chunk sizes

## [0.1.0] - 2026-03-26

### Added

- **WAV codec**: RIFF WAVE encoder/decoder supporting PCM 8/16/24/32-bit integer and IEEE float 32-bit
- **FLAC decoder**: STREAMINFO parsing, frame sync, Constant/Verbatim/Fixed subframes, Rice entropy coding, channel decorrelation (independent, left-side, right-side, mid-side)
- **PCM conversions**: i16/i24/i32 to f32 and back, interleave/deinterleave, packed 24-bit support
- **Sinc resampler**: Windowed sinc interpolation with Blackman-Harris window, Draft/Good/Best quality levels
- **Tag reading**: ID3v2 (v2.2/v2.3/v2.4) and Vorbis Comment metadata parsing
- **Codec trait**: Unified `AudioCodec` trait and auto-detect `open()` function
- **Format detection**: Magic byte detection for WAV and FLAC
- Feature-gated modules: `wav`, `flac`, `pcm`, `resample`, `tag`, `logging`
- `no_std` support (with `alloc`)
- Serde serialization for all public types
- Comprehensive test suite with integration tests
- Criterion benchmarks for WAV decode, PCM conversion, and resampling
