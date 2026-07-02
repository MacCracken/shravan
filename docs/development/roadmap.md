# Development Roadmap

> **v2.5.9** — 859 tests, Cyrius 6.3.27. **Opus now decodes to real PCM audio**
> (encode→decode waveform correlation 0.99+, mono+stereo) — the headline deferred item.
> The forward plan below was **reorganized 2026-07-02**: the sprawling
> 2.5.9–2.5.23 point-release list was collapsed into **four focused 2.5.x releases**
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
- **AAC *decode*** (real dequant → IMDCT → overlap-add) and **MP4/M4A** demux → AAC-decode
  → PCM (verified: 1024 non-zero samples from a real AU).
- **Opus/CELT encode+decode → PCM** *(2.5.9)* — `opus_encode` → `opus_decode_from_packets`
  round-trips real audio (waveform correlation: mono 0.997, stereo 0.996/0.999, transient
  0.998), mono + stereo. Caveat: a **bespoke, non-RFC-6716** stream (won't interoperate
  with libopus — conformance is a pinned 2.5.x item); transient frames decode via the
  long MDCT (short-window pre-echo coding deferred).

**🟥 Looks done, isn't (scaffolding / broken — the "actual core" that remains):**
- **AAC *encode* → silence.** The quantizer step is ~100× too coarse **and** the encode/
  decode quantizers don't invert → every coefficient rounds to 0. A 0.6-amplitude sine
  decodes to ~2e-7. *(The old status wrongly listed AAC as end-to-end.)* Also: `fft_mdct`
  is not the inverse of `fft_imdct` (found in 2.5.9), a second reason encode→decode can't
  reconstruct — the AAC encoder needs the fixed transform pair too.
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

## Forward plan — the deferred 2.5.x core work, done

Every deferred tail from 2.5.0–2.5.8 (the "Deferred:" footnotes) plus the bugs the audit
found are packed here into **four focused 2.5.x releases**, ordered dependency-first. This
is the work the scaffolding was for — none of it is pushed to a future major.

Acceptance rule for every release: **new/changed audio paths are proven by value-level
tests (SNR or correlation vs. input), never by sample count** — that is how the old suite
hid the silence. Each commit must build clean (`cyrius build src/main.cyr build/shravan`)
and keep the suite green.

### v2.5.9 — Opus/CELT decode → PCM · shipped 2026-07-02 · THE headline deferred item

**Done:** `opus_encode` → `opus_decode_from_packets` round-trips real audio — waveform
correlation mono 0.997, stereo 0.996/0.999, transient 0.998 (verified by value, not
sample count). The 2.5.0–2.5.8 pieces are now composed into PCM and the real decode path
is wired. Stream stays shravan-internal (RFC-6716 conformance is its own item below).

- [x] Magnitude synthesis (`_opus_denorm_frame`): band energy × unit PVQ shape → MDCT coeffs.
- [x] **Found + fixed the real blocker:** `fft_mdct`/`fft_imdct` are not a matched pair
      (encode→decode cancelled to silence — also a cause of the silent AAC encoder); added a
      verified direct pair `_opus_mdct`/`_opus_imdct` (OLA reconstruction 0.999).
- [x] IMDCT + synthesis sine window + 50%-overlap-add (Princen-Bradley TDAC); encoder now
      frames with 50% overlap to match.
- [x] Wired `opus_decode_from_packets` (mono + stereo via `_opus_stereo_decouple`).
- [x] Band-energy DPCM widened 128→256 symbols (the ±63 clamp was losing absolute levels).
- [x] Rate-control complexity from the real MDCT spectrum (2.5.8 deferral).
- [x] Encoder overflow no longer truncates/corrupts the packet.
- [~] Transient frames decode via the long MDCT (correct audio); true overlapping
      short-window coding deferred (see 2.5.10-tail / CELT transient completion).
- [~] Two-pass VBR still future.

**Follow-on hardening (do in 2.5.12):** fix `fft.cyr`'s `fft_mdct`/`fft_imdct` to be a
matched pair (unblocks the AAC encoder + lets Opus drop the O(N²) direct transform), and
add a real MDCT↔IMDCT reconstruction test (today's only asserts "output nonzero").

### v2.5.10 — AAC produces audio + all AAC bugs/completions · medium–large

**Goal:** `aac_encode` → `aac_decode` faithfully round-trips (SNR-verified), **mono and
stereo**, and every AAC correctness/safety bug the audit found is closed. Folds in the
deferred TNS + psychoacoustic completions.

**Commit plan:**
1. `fix(aac): quantizer step/scale-factor so encode and decode invert (consistent 0.75-power placement) + single-band roundtrip test` *(the silent-encoder root cause)*
2. `feat(aac): rate/distortion scale-factor loop to hit a bit budget (replaces the fixed heuristic) + bitrate-tracks-target test`
3. `fix(aac): encoder MDCT framing — 50% overlap, stop zeroing the 2nd half so TDAC cancels aliasing + windowed-SNR test`
4. `fix(aac): scale-factor DPCM predictor tracks transmitted (clamped) value (mono + CPE) + large-delta roundtrip test` *(confirmed desync bug)*
5. `fix(aac): CPE stereo bitstream — drop the stray side ICS, make side section codebooks match the coding + stereo SNR test`
6. `fix(aac): guard reserved section codebooks 12–15 (DoS infinite-loop) + remove decoder band double-increment for cb 1–4/9/10 + reject-not-hang test`
7. `feat(aac): real HCB6 tables; TNS short-window + stereo/CPE; psychoacoustic ATH floor + tonality-adjusted SMR` *(completes the 2.5.2/2.5.3 deferrals)*
8. `test(aac): content-based SNR roundtrip — sine/sweep/noise, mono + stereo`
9. `docs/CHANGELOG/VERSION: 2.5.10 — AAC produces audio`

### v2.5.11 — MP3 decode → PCM · large

**Goal:** MP3 Layer III (+ Layer I/II bitrate correctness) decodes to real PCM — replaces
the metadata-only stub.

**Commit plan:**
1. `fix(mp3): Layer I/II bitrate tables (select by layer) + frame-size test` *(existing bug)*
2. `feat(mp3): granule/side-info parse + scalefactor decode + cross-frame bit reservoir`
3. `feat(mp3): Huffman tables + big_values/count1 decode of the 576 frequency lines`
4. `feat(mp3): requantization (power law + pretab) + short-block reorder`
5. `feat(mp3): alias reduction + hybrid IMDCT (long 36 / short 12) + overlap-add`
6. `feat(mp3): 32-band polyphase synthesis filterbank`
7. `feat(mp3): MS/intensity stereo + wire into mp3_decode + real-file correlation test`
8. `docs/CHANGELOG/VERSION: 2.5.11 — MP3 decode`

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

**New subsystems (larger; may each become their own 2.5.x point release):**
- `feat(opus): RFC-6716 conformance — real ec_enc/ec_dec, Laplace coarse/fine energy, spec band layout + allocation (trim/boost/anti-collapse/fine-energy), correct TOC` — decode real `.opus`, libopus decodes ours *(supersedes the bespoke coder)*
- `feat(opus): SILK mode — LPC/LTP/LSF, excitation, shared range coder`
- `feat(opus): hybrid mode — SILK low-band + CELT high-band over one range coder (completes the Opus encoder)`
- `feat: DSD — DSD64/128/256 + DoP (1-bit sigma-delta path)`

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
