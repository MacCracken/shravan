# Development Roadmap

> **v2.5.8** — 843 tests + fuzz (90K/0), Cyrius 6.3.27. This is the current shipped
> version. The forward plan below was **reorganized 2026-07-02**: the sprawling
> 2.5.9–2.5.23 point-release list was collapsed into **four audio-milestone releases**
> (2.6.0 → 3.0.0), each packed with a bite-sized commit plan. The organizing principle:
> **make the codecs actually produce audio, and stop the test suite from hiding that
> they don't**, before adding breadth.

> **Reorg assumptions (flag to revisit):** (1) release shape = 4 milestones, honesty/
> safety first; (2) Opus target = **bespoke-internal decode first** (wire the existing
> bit-exact encoder/decoder into a real PCM round-trip, explicitly labeled non-RFC-6716),
> with **RFC-6716 conformance deferred to the 3.x breadth tier**. If either assumption is
> wrong, 2.7.0/2.8.0 scope changes.

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
- **AAC *decode*** (real dequant → IMDCT → overlap-add) and **MP4/M4A** demux → AAC-decode
  → PCM (verified: 1024 non-zero samples from a real AU).

**🟥 Looks done, isn't (scaffolding / broken — the "actual core" that remains):**
- **AAC *encode* → silence.** The quantizer step is ~100× too coarse **and** the encode/
  decode quantizers don't invert → every coefficient rounds to 0. A 0.6-amplitude sine
  decodes to ~2e-7. *(The old status wrongly listed AAC as end-to-end.)*
- **Opus/CELT — no audio either direction.** `opus_decode_from_packets` returns an
  **empty** vector; the CELT frame decoders are called **only from tests**; there is no
  magnitude synthesis / IMDCT / overlap-add. ~30% of all assertions are Opus, and *none*
  check audio. The encoder emits a **bespoke, non-RFC-6716** stream (LZMA-style coder).
- **ALAC — dead code.** A full decoder exists but is **unwired**: `detect_format` has no
  ALAC branch and MP4 routes only to AAC, so `codec_open`'s `FMT_ALAC` dispatch is
  unreachable and the decoder is unverified on real frames.
- **MP3 decode — empty stub** (metadata only; no Huffman/requant/IMDCT/filterbank).

**🐛 Confirmed real bugs the "843 passing" hides** *(scheduled into 2.6.0 unless noted)*:
- **[AAC] DoS**: `_aac_decode_spectral` **infinite-loops** on section codebook 12–15
  (unvalidated `cb`) — hangs on malformed/unexpected input.
- **[AAC] scale-factor DPCM desync**: encoder clamps transmitted deltas to ±60 but
  tracks the *unclamped* predictor (mono `aac.cyr:1096`, CPE `:1319`); decoder tracks the
  clamped value (`:2466/2883/2935`) → any delta >±60 corrupts that band and all later ones.
- **[AAC] CPE stereo bitstream malformed** → wrong stereo (a stray 11-bit side ICS +
  codebook/escape disagreement; the "hang" theory was *refuted* — guards terminate it).
- **[AAC] decoder double-increments band** for codebooks 1–4/9/10 → skips the next band.
- **[MP3]** Layer I/II parsed with the Layer III bitrate table → wrong frame size/duration.
- **[Opus]** decode advertises `total_samples>0` but returns 0 samples → silent playback,
  no error, for any consumer trusting the metadata.
- **[FLAC]** `decode_range` seek anchor uses the current frame's block size, not the
  stream's; Rice unary bound (32768) can reject valid deep-bps streams *(both low)*.
- **[core]** WAVE_FORMAT_EXTENSIBLE uses `valid_bits` as the byte stride (mis-decodes
  24-in-32); `detect_format` collides with sankoch's (build warning + consumer hazard)
  *(both low)*.

---

## Forward plan — four audio-milestone releases

Acceptance rule for every release below: **new/changed audio paths are proven by
value-level tests (SNR or correlation vs. input), never by sample count.** Each commit
must build clean (`cyrius build src/main.cyr build/shravan`) and keep the suite green.

### v2.6.0 — Honesty & Safety · small–medium · no new DSP subsystems

**Goal:** make the suite tell the truth and close every correctness/safety hole that does
*not* need a new DSP subsystem. This unblocks all later work: once tests assert real audio
(and honestly pin the not-yet-working paths), the AAC/Opus/MP3 fixes can't silently
regress or silently "pass."

**Commit plan (bite-sized, ordered — each green):**
1. `test: value-level PCM helpers (assert_pcm_close / snr_db) + apply to WAV/AIFF roundtrips`
2. `test: extend FLAC roundtrips to assert values across all subframe/stereo modes`
3. `test: honestly pin current no-audio behavior — assert AAC-encode→decode is silent,`
   `opus_decode_from_packets returns empty, mp3_decode returns empty (each with a`
   `TODO(2.7.0/2.8.0/3.0.0) pointer)` — reality lives in the suite; fixes flip these later
4. `fix(aac): guard reserved section codebooks 12–15 in _aac_decode_spectral (DoS) + reject-not-hang test`
5. `fix(aac): remove decoder band double-increment for cb 1–4/9/10 + alignment test`
6. `fix(aac): scale-factor DPCM predictor tracks transmitted (clamped) value (mono + CPE) + large-delta roundtrip test`
7. `fix(opus): make the decode metadata/data contract explicit (align total_samples with returned audio) + test`
8. `fix(mp3): add Layer I/II bitrate tables, select by layer + frame-size test for an L2 frame`
9. `fix(flac): decode_range seek anchor uses stream block size + trailing-short-frame test`
10. `fix(flac): raise/handle Rice unary bound for valid deep-bps streams`
11. `fix(wav): WAVE_FORMAT_EXTENSIBLE tracks container bytes vs. valid bits (24-in-32) + test`
12. `test(alac): verify the ALAC decoder on a real frame (Rice/LPC/unmix → PCM)`
13. `feat(alac): wire detect_format + codec_open dispatch (or explicitly gate FMT_ALAC as unreachable)`
14. `chore: resolve detect_format/sankoch collision (namespace shravan's audio detector) — clears build warning`
15. `docs: roadmap functional-status → verified truth; CHANGELOG 2.6.0; VERSION bump`

### v2.7.0 — AAC produces audio · medium–large

**Goal:** `aac_encode` → `aac_decode` reconstructs the input within an SNR threshold
(lossy but faithful), **mono and stereo**. Flip the 2.6.0 AAC "silence" pin to a real
round-trip.

**Commit plan:**
1. `fix(aac): quantizer step/scale-factor so encode and decode invert (consistent 0.75-power placement) + single-band roundtrip test`
2. `feat(aac): rate/distortion scale-factor loop to hit a bit budget (replaces the fixed heuristic) + bitrate-tracks-target test`
3. `fix(aac): encoder MDCT framing — 50% overlap, stop zeroing the 2nd half so TDAC cancels aliasing + windowed-SNR test`
4. `fix(aac): CPE stereo bitstream — drop the stray side ICS, make side section codebooks match the coding + stereo SNR test`
5. `feat(aac): real HCB6 spectral tables (remove the HCB5 placeholder)`
6. `test(aac): content-based SNR roundtrip — sine/sweep/noise, mono + stereo (flip the 2.6.0 pin)`
7. `docs: CHANGELOG 2.7.0; VERSION bump; update functional-status`

### v2.8.0 — Opus decodes to audio (bespoke-internal) · large

**Goal:** `opus_encode(tone/sweep)` → `opus_decode_from_packets` → PCM that **correlates
with the input** (mono + stereo). Uses the existing invertible LZMA-range coder + CWRS
PVQ + energy DPCM. **Explicitly labeled shravan-internal / non-RFC-6716** (will not read
`.opus` files from libopus/ffmpeg; conformance is a 3.x milestone). This is the headline
gap the 2.5.0–2.5.8 scaffolding was building toward.

**Commit plan:**
1. `feat(opus): magnitude synthesis — apply band-energy gain to the unit PVQ shape → MDCT coefficients + unit test`
2. `feat(opus): CELT IMDCT + sine-window inter-frame TDAC overlap-add (mono) via fft_imdct + single/two-frame test`
3. `feat(opus): wire opus_decode_from_packets — iterate audio packets → CELT frame decode → PCM, honor pre_skip (mono) + waveform-correlation test`
4. `feat(opus): stereo decode — _opus_stereo_decouple + per-channel synth + stereo correlation test`
5. `feat(opus): transient/short-block inverse (short IMDCT + overlap) so transient frames decode + test`
6. `fix(opus): encoder payload overflow — no silent truncation past target_bytes`
7. `docs(opus): label the bitstream shravan-internal/non-RFC-6716; note conformance = 3.x` + `test(opus): flip the 2.6.0 decode pin to real-audio assertions`
8. `docs: CHANGELOG 2.8.0; VERSION bump; update functional-status`

### v3.0.0 — MP3 decode + breadth · large (major)

**Goal:** MP3 Layer III decodes to PCM; the major bump also carries the breadth items that
change surface/compat (PCM_F64, RFC-Opus stream, etc.). MP3 decode is a full subsystem, so
it gets its own ordered commit series; breadth items follow as an explicitly-sequenced
backlog (any one may be split into its own 3.x minor).

**MP3 Layer III decode — commit plan:**
1. `feat(mp3): granule/side-info parse + scalefactor decode + cross-frame bit reservoir`
2. `feat(mp3): Huffman tables + big_values/count1 decode of the 576 frequency lines`
3. `feat(mp3): requantization (power law + pretab) + short-block reorder`
4. `feat(mp3): alias reduction + hybrid IMDCT (long 36 / short 12) + overlap-add`
5. `feat(mp3): 32-band polyphase synthesis filterbank`
6. `feat(mp3): MS/intensity stereo + wire into mp3_decode + real-file correlation test (flip the 2.6.0 pin)`
7. `docs: CHANGELOG 3.0.0; VERSION bump`

**Breadth backlog (post-MP3; each independent, sequence as capacity allows):**
- **Opus RFC-6716 conformance** — real `ec_enc`/`ec_dec`, Laplace coarse/fine energy,
  spec band layout + allocation tables (trim/boost/anti-collapse/fine-energy), correct
  TOC. Makes shravan decode real `.opus` and libopus decode ours. *Large; supersedes the
  bespoke coder.* (The big one if interop matters.)
- **Opus transient/stereo/rate-control completion** — overlapping short-window shapes +
  per-band time-frequency resolution + cross-frame transient memory; stereo+transient
  combined + intensity stereo + non-even mid/side split; VBR driven by real MDCT/masking
  complexity + stereo/transient-aware allocation + two-pass. *(needs 2.8.0.)*
- **FLAC LPC encoder** — autocorrelation + Levinson-Durbin + quantized coeffs (beats
  Fixed, matters for 24/32-bit); plus partitioned Rice search, adaptive stereo-mode
  choice, CONSTANT-subframe emission, SEEKTABLE emit, decoder MD5 verify. *(decode already
  works.)*
- **AAC TNS completion** — short-window + stereo/CPE TNS (today: long-window mono only).
- **AAC psychoacoustic completion** — absolute-threshold-of-hearing floor + tonality-
  adjusted SMR (SPL calibration).
- **Hi-res + wide PCM + SSE2** — 88.2–384 kHz roundtrip tests; **PCM_F64** WAV encode/
  decode (today `wav_encode` rejects it); PCM conversion SSE2/unrolled hot loops with
  before/after benchmarks.
- **SILK mode (speech)** — LPC/LTP/LSF, excitation, shared range coder. *(needs the Opus
  framework.)*
- **Hybrid mode (SILK + CELT)** — SILK low-band + CELT high-band over one range coder;
  completes the Opus encoder. *(needs SILK + 2.8.0.)*
- **MP4 completion** — `esds`/AudioSpecificConfig parse (today profile hardcoded LC),
  AudioSampleEntry v1/v2, multi-track/edit-list (`elst`/`stts`, gapless trim), co64/
  largesize >4 GiB, and vendor the AAC chain into the fuzz harness to fuzz `mp4_decode`.
- **ALAC-in-MP4** — recognize the `alac` sample entry, extract `ALACSpecificConfig`, route
  `mdat` frames to `alac_decode` *(if not already wired in 2.6.0)*.
- **DSD** — DSD64/128/256 + DoP (1-bit sigma-delta path).
- **Fuzz-coverage completion** — add AAC, MP4, and Opus-decode targets so the untrusted-
  input paths shipped since 2.5.2 are actually fuzzed.

---

## Completed history

### v2.5.0–2.5.8 — CELT/Opus + AAC sub-layer scaffolding (shipped, internal-only)

Nine point releases built the Opus/CELT entropy + allocation layers and several AAC
sub-features, each proven **internally bit-exact** but **not composed into PCM** (see
Functional status). Superseded by the 2.6.0–3.0.0 plan above.

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
