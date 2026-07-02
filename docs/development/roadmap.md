# Development Roadmap

> **v2.5.8** — 843 tests + fuzz (90K/0), Cyrius 6.3.27.
> CELT/Opus rate control + VBR: bit-reservoir controller + per-frame complexity metric
> drive CBR / constrained-VBR / VBR packet sizing; `opus_encode_vbr`. Completes the CELT
> bit-allocation *layer* — but Opus decode-to-PCM is still not wired (see Functional
> status below). (v2.5.7: MP4/M4A container demux. v2.5.6: CELT stereo full two-channel
> bitstream. v2.5.5: stereo coupling foundation. v2.5.4: transient detection + short-block
> MDCT. v2.5.3: AAC psychoacoustic model. v2.5.2: AAC TNS + serde repair. v2.5.1: first
> CELT decode. v2.5.0: full PVQ spectral shape. v2.4.4: Opus encoder framework.)
> (v2.3.0: security audit 21/21 fixed, 90K fuzz 0 crashes; MDCT 5.45x, polyphase resampler.)

## Functional status (honest — verified 2026-07-01)

"Tests pass" ≠ "codec produces audio." The passing Opus tests assert real, exact
*internal* properties (range coder invertible, PVQ bijective, M/S orthonormal, TNS
FIR∘IIR = identity, DPCM round-trips) — but not "a real stream decodes to audio."
What actually round-trips PCM end-to-end vs. what is still internal-only:

- **Real end-to-end audio:** WAV, AIFF (encode+decode); FLAC, ALAC (decode); AAC
  (encode+decode → IMDCT/overlap-add → PCM); MP4/M4A (demux → AAC → PCM).
- **NOT end-to-end yet — pinned below:**
  - **Opus/CELT.** The 2.5.0–2.5.8 work built the encoder + the entropy/PVQ/TNS/
    stereo/transient/rate sub-layers and proved each internally bit-exact, but
    `opus_decode_from_packets` still returns an **empty** sample vector: the CELT
    decoder (`_opus_decode_celt_frame`) is **only called from tests**, never wired
    into the decode path, and it reconstructs shape *direction* + energies, not PCM
    (no magnitude synthesis / IMDCT / overlap-add). The encoder emits a **bespoke,
    non-RFC-6716** bitstream, so it does not interoperate with real Opus tools either.
  - **MP3 decode.** A stub: `mp3_decode` returns metadata + an **empty** sample vector
    (no Huffman / requant / IMDCT / synthesis filterbank).

The v2.5.x items below finish these tails. They were previously buried as "Deferred:"
footnotes on shipped releases; they are now real, ordered, pinned releases.

## Completed (v2.0.0)

- [x] Full Rust -> Cyrius port (10,265 -> 11,780 lines)
- [x] All codec modules: WAV, AIFF, FLAC, ALAC, Ogg, MP3, Opus, AAC
- [x] AAC-LC decoder from scratch (bitreader, ICS parsing, spectral decode, inverse quant, IMDCT, overlap-add)
- [x] FLAC SEEKTABLE parsing + decode_range() for sample-accurate seeking
- [x] Opus decode path (OpusHead/OpusTags parsing, granule-based duration, ogg_decode dispatch)
- [x] codec_open dispatches all 8 formats
- [x] Stream chunking (WAV/AIFF chunk_frames functional)
- [x] Benchmarks: FLAC 5.2x of Rust+LLVM, WAV 42x, compile 16x faster

## Completed (v2.1.0)

- [x] AAC scale factor Huffman codebook (ISO 14496-3, 121 entries)
- [x] AAC M/S stereo decoding (CPE, common window, per-band M/S mask)
- [x] Metadata writing: tag_write_id3v2(), tag_write_vorbis() with roundtrip tests
- [x] True incremental FLAC streaming (frame-by-frame via flac_decode_range)

## Completed (v2.1.1)

- [x] AAC spectral codebooks 5, 7, 8 (ISO Huffman tables — signed pairs, unsigned pairs)
- [x] AAC short window support (EIGHT_SHORT_SEQUENCE, 8x MDCT-256, short SWB table)
- [x] General-purpose Huffman decoder (_aac_huff_decode)
- [x] Correct section length coding for short vs long windows

## Completed (v2.2.0 — performance + stdlib modernization)

- [x] Compiler upgrade: cc3 3.4.3 → 4.10.3
- [x] Stdlib modernization: 12 vendored files removed, resolved via cyrius.toml [deps] stdlib
- [x] Sankoch compression dep (LZ4, DEFLATE, zlib, gzip) via [deps.sankoch]
- [x] Stdlib gains: inverse trig, inverse hyperbolic, lerp/hypot/sign/trunc/fract, gcd/lcm/fibonacci/binomial, f64_parse, word-at-a-time strlen, rep movsb/stosb, ASCII case helpers, extended str functions, bounds-checked fmt_sprintf
- [x] PCM hot path: div→mul reciprocals, vec pre-allocation, f32 denormal constant
- [x] FFT twiddle pre-computation: per-level tables, MDCT/IMDCT twiddle caching
- [x] AAC Huffman: sorted codebook decode replacing linear scan
- [x] WAV chunk parsing: u32 comparison replacing nested byte checks
- [x] Named struct size constants (FMTINFO_SIZE, DECODE_RESULT_SIZE, BITREADER_SIZE)
- [x] MDCT N/2-point FFT rewrite: 8.5ms → 1.6ms (5.45x faster)
- [x] Resample polyphase filter table: 256-phase pre-computed kernel cache

## Completed (v2.3.0 — security hardening)

- [x] Security audit: 21 findings (7 P0 + 5 P1 + 6 P2 + 3 P3), all fixed
- [x] P0: WAV/AIFF chunk overflow guards, SSND underflow, extensible OOB, FLAC block_size/metadata bounds, Ogg accumulation overflow cap
- [x] P1: FLAC unary bound 32768, ALAC INT64_MIN guard, MP3 frame_size guard, AAC frame cap 65536, SEEKTABLE cap 1024
- [x] P2: Vorbis zero-length guard, MDCT size validation, bitreader parameter validation, `_safe_alloc_mul` helper, tag COMM validation
- [x] Fuzz harness: `fuzz/fuzz_codecs.cyr` — FLAC, Ogg, MP3, ID3v2 (90K calls, 0 crashes)
- [x] 3 upstream issues filed for Cyrius 5.0.1 (alloc overflow, vec capacity overflow, allocation size cap)

---

## v2.3.x — Completeness

### v2.3.0 — Low-effort items (done)

- [x] Streaming: `decode_file()` (read file + auto-detect + decode) and `decode_reader()` / `decode_reader_feed()` / `decode_reader_flush()` (streaming with auto-detect)
- [x] AAC: 2-level Huffman lookup table (256-entry level-1, sorted scan fallback for long codes)
- [x] AAC: codebook 1-4 dispatch (skip bands — tables not yet loaded)
- [x] AAC: codebook 9-10 dispatch (stub — tables not yet loaded)

### v2.3.1 — AAC spectral codebooks (done)

- [x] HCB9/HCB10: 169 entries each (13x13 unsigned pairs), full decode with 2-level LUT
- [x] HCB1-4: 81 entries each (3^4 4-tuples), `_aac_decode_spectral_quad()` decoder
- [x] All 11 AAC spectral codebooks now operational (was 5-8 + 11 only)

### v2.3.2 — AAC encoder improvements (done)

- [x] Per-band Huffman codebook selection (1-11 based on max band magnitude)
- [x] M/S stereo encoding (CPE with mid/side transform, replaces mono downmix)
- [x] VBR mode (`aac_encode_vbr`, quality levels 1-5)
- [x] Per-codebook spectral encoding (quad/pair/escape dispatch)

_(v2.3.3 TNS, v2.3.4 MP4/M4A, v2.3.5 Performance were unshipped when 2.4.0 was
repurposed as the modernization release — moved to **v2.5.x** below.)_

---

## v2.4.0 — Language & toolchain modernization (shipped 2026-07-01)

- [x] Cyrius toolchain 4.10.3 → 6.3.19; pin in `cyrius.cyml [package].cyrius`
- [x] Manifest `cyrius.toml` → `cyrius.cyml` (`${file:VERSION}`, `language`, toolchain pin)
- [x] Stdlib re-vendored via `cyrius lib sync` (version-matched 6.3.19 snapshot in `lib/`)
- [x] `ganita` added for transcendentals (moved out of `math` in 6.x); all deps retained (incl. `sankoch`)
- [x] Symbol collisions resolved (F64_HALF, is_err/err_code, is_ok→res_ok)
- [x] CI/release workflows modernized (pin-driven install, `cyrius lib sync` → `deps` → build)
- [x] `serde` JSON serialization wired in — `bayan` dep + `#derive(Serialize)` for
      `ShrFormatInfo`; AudioFormat/PcmFormat/ShravanErr to/from JSON now live + tested
- [x] 539 tests pass (520 + 19 serde)

## v2.4.1 — serde full type coverage (shipped 2026-07-01)

Restored the full Rust `#[derive(Serialize, Deserialize)]` surface as JSON.

- [x] Enums: MpegVersion, MpegLayer, ChannelMode, ResampleQuality (value-based)
- [x] Int structs via `#derive(Serialize)`: ShrAlacConfig, ShrMp3FrameInfo, ShrOpusHead
- [x] ShrAudioMetadata (7 Str tag fields) — `to_json` via derive; roundtrip verified
- [x] Codec markers WavCodec…AlacCodec (8)
- [x] Roundtrip tests per type (563 total)
- [!] `#derive(Serialize)` `Str`-**deserialize** is broken on cyrius 6.3.x — filed
      upstream (`cyrius …/2026-07-01-derive-serialize-str-field-deserialize-broken`);
      shipped a hand-written `audio_metadata_from_json` stopgap. **Remove once the
      derive is fixed upstream.**

## v2.4.2 — Stabilize (shipped 2026-07-01)

- [x] **Fuzz harness rebuilt for 6.3.19** — `fuzz/fuzz_codecs.cyr` self-contains
      the codec helpers it needs (`fmtinfo_*`, `decode_result_*`, `write_u32_le`,
      `read/write_u32_be`, unreachable `opus_decode_from_packets` stub); fixed the
      `ogg_parse_page`/`mp3_scan_frames` call arities. Builds clean, 90K calls / 0
      crashes (`./fuzz/run.sh`).
- [x] `cyrius vet` (include-dep audit) + a fuzz smoke pass wired into CI.

## v2.4.3 — Distlib bundle + source-layout cleanup (shipped 2026-07-01)

- [x] **`dist/shravan.cyr` distlib bundle** for consumers (tarang/jalwa/dhvani/
      shruti), matching the naad model (`cyrius distlib`). Self-contained;
      consumer smoke test verified (encode→decode→detect through the bundle).
- [x] Split `src/main.cyr` → `src/shravan.cyr` (library) + `src/main.cyr` (test harness).
- [x] Relocated codec modules `lib/` → `src/`; `lib/` is now vendored stdlib only
      (distlib stops mis-capturing them as leaves; `cyrius vet`/audit cleaner).

## v2.4.4 — Opus encoder framework (shipped 2026-07-01)

- [x] Encoder framework in `src/opus.cyr` (additive; 563 → 610 tests):
      `OpusEncoder` config/state, mode + bandwidth selection, RFC 6716 TOC byte
      (config 0–31), `opus_encode_frame` dispatch (CELT wired; SILK/HYBRID = 2.5.x seams).
- [x] Design doc: `docs/adr/0001-opus-encoder-framework.md`.
- [ ] CLEANUP (do in 2.5.0): `_opus_encode_celt_frame` hardcodes TOC config 30
      (CELT/FB 10ms) for 20ms frames — should be config 31 (`opus_toc_byte` computes
      it); swap the hardcode with a decode round-trip guard.

---

## v2.5.0 — Full PVQ spectral shape (CELT) · shipped 2026-07-01

- [x] `V(N,K)` pyramid count grid + `_pvq_bits`/`_pvq_choose_k` (K from bit budget).
- [x] CWRS index encode/decode — quantizer roundtrip `decode(encode(y))==y`
      (exhaustive N=2,K=2 + roundtrips across (3,2),(4,3),(2,10),(1,3); out-of-range rejected).
- [x] PVQ pulse search (`Rxy²/Ryy` greedy) + `_pvq_denormalize`; reconstructed
      shape cosine 0.9798 vs sign-only 0.800 — **PVQ beats sign-only**.
- [x] Wired into `_opus_encode_spectral_shape` (per-band split ≤ N=32, data-independent
      K budget so the future decoder computes identical K); TOC config 30→31 fix.
- [x] Full packet encode→decode stream roundtrip — shipped in **2.5.1**.

## v2.5.1 — CELT decode + full-stream PVQ roundtrip · shipped 2026-07-01

- [x] Replaced the custom range coder with a matched, provably-invertible LZMA-style
      pair (`opus_range_enc_*` + new `opus_range_dec_*`); 9-symbol mixed uint/bit
      roundtrip incl. edge cases (0/3, 65535/65536).
- [x] `_pvq_decode_band` — inverse of `_pvq_encode_band` (CWRS index → pulse vector);
      band-level symbol-exact roundtrip across narrow/wide N and a spread of K.
- [x] `_opus_decode_band_energies` (inverse DPCM) + closed-loop encoder fix so the
      predictor tracks the reconstructed value (encoder/decoder stay in sync).
- [x] `_opus_decode_spectral_shape` — lock-step band/sub-block traversal with the
      same data-independent K budget as the encoder; unit-L2 shape reconstruction.
- [x] `_opus_decode_celt_frame` — TOC parse (config 31) → energies + shape decode.
- [x] Full-stream roundtrip test: energies bit-exact, every K>0 sub-block's shape
      bit-exact vs the encoder's own PVQ output.
- Deferred: full magnitude synthesis (shape × band energy) + IMDCT→PCM with
  inter-frame TDAC overlap-add — a later 2.5.x refinement (this ships CELT
  *direction* decode, which is what PVQ carries).

## v2.5.2 — Toolchain 6.3.25 + serde repair + AAC TNS · shipped 2026-07-01

- [x] Toolchain pin 6.3.19 → 6.3.25 (`cyrius lib sync` re-vendor; lock refresh).
- [x] Retired the serde Str-deserialize workaround — cycc 6.3.25 fixes derived
      `_from_json` for `Str` fields (`ShrAudioMetadata` roundtrips via the derive).
- [x] AAC TNS (AAC-LC long-window, mono/SCE): analysis FIR before quantization +
      synthesis IIR before IMDCT, exact-inverse filter pair from shared quantized
      reflection coefs; `tns_data()` bitstream; full mono encode→decode roundtrip.
      Stereo/CPE path untouched. (Short-window + stereo TNS = later refinement.)

## v2.5.3 — AAC psychoacoustic model · shipped 2026-07-01

- [x] Asymmetric critical-band spreading function (`_aac_psy_init`): gentle upward
      (10 dB/SFB), steep downward (25 dB/SFB) — the upward spread of masking.
- [x] `_aac_psy_masking_offsets`: per-band coarseness offset `2·log2(spread/energy)`
      (0 isolated, positive when masked, capped at 24 scf units).
- [x] Encoder adds the offset to the baseline scale factor — masked bands quantize
      coarser; unmasked bands and the stereo/CPE path + decoder are untouched.
- Deferred: absolute-threshold-of-hearing floor + tonality-adjusted SMR (needs SPL
  calibration); closed-loop scale-factor DPCM (the ±60 diff-clamp is pre-existing).

## v2.5.4 — Transient detection + short-window switching (CELT) · shipped 2026-07-01

- [x] `_opus_detect_transient` — symmetric adjacent-sub-block energy test catches
      onsets and decays anywhere in the frame (incl. the first sub-block).
- [x] `_opus_short_mdct` — 8 windowed short MDCTs, coefficients interleaved
      (`out[f*M+b]`) into the 480-bin buffer so band-energy + PVQ code is unchanged.
- [x] `isTransient` coded as one range-coder bit at the frame head; decoder reads it
      back; full transient-frame roundtrip (flag + energies + shape all bit-exact).
- Deferred: proper CELT overlapping short-window shapes + per-band time-frequency
  resolution + cross-frame transient memory (need full IMDCT→PCM synthesis).

## v2.5.5 — Stereo coupling foundation (CELT) · shipped 2026-07-01

- [x] `_opus_stereo_ms_forward` / `_opus_stereo_ms_inverse` — energy-preserving
      mid/side transform (M=(L+R)/√2, S=(L-R)/√2), exactly invertible.
- [x] `_opus_stereo_couple` — per-band coupled-vs-dual decision (M/S when
      min(E_M,E_S) < min(E_L,E_R)); `_opus_stereo_decouple` inverts via `ms_flags`;
      couple∘decouple is the identity for any per-band choice.
- Deferred to 2.5.6: full two-channel bitstream + intensity stereo.

## v2.5.6 — Stereo coupling: full two-channel bitstream (CELT) · shipped 2026-07-01

- [x] `_opus_encode_celt_frame` stereo branch: deinterleave L/R → window + MDCT →
      `_opus_stereo_couple` → mid/side + `ms_flags`; codes isTransient + 21 `ms_flags`
      bits + mid/side energies + mid/side PVQ shapes (shape budget split evenly);
      stereo TOC bit. Replaces the mono downmix.
- [x] `_opus_decode_celt_stereo_frame` — mirror decode; caller `_opus_stereo_decouple`s
      to L/R. Shape encode/decode refactored to an explicit-bit-budget core.
- [x] Full stereo roundtrip (`ms_flags` + mid/side energies + shapes bit-exact) +
      encoder-path smoke test. Mono path unchanged.
- Deferred: stereo + transient (short blocks); intensity stereo; non-even bit split.

## v2.5.7 — MP4/M4A container · shipped 2026-07-01

- [x] Box-tree parser (`mp4_find`) + `moov→trak→mdia→minf→stbl` navigation (picks the
      `soun` handler track); `stsd`/`mp4a` track info + `stsz`/`stco`/`co64`/`stsc`
      sample-location table.
- [x] `mp4_decode` wraps each AU in an ADTS header → `aac_decode`; `FMT_MP4`
      detection (`ftyp`) + `codec_open`/`decode_file` dispatch.
- [x] Adversarially reviewed + hardened against malformed input (bounds-checked
      counts/offsets/sizes + FullBox headers + largesize; null-checked allocs);
      hostile files return an error, not a crash. Reject-not-crash tests added.
- Deferred: full `esds`/AudioSpecificConfig parse; multi-track/edit-list; fuzzing
  `mp4_decode` (needs the AAC chain in the standalone fuzz harness).

## v2.5.8 — Rate control + VBR (CELT/Opus) · shipped 2026-07-01

- [x] `_opus_rate_new` / `_opus_rate_target` — bit-reservoir controller: CBR (nominal
      every frame), constrained VBR (spend saved bits, average ≤ target bitrate),
      unconstrained VBR (size by complexity).
- [x] `_opus_frame_complexity` — high-frequency-energy proxy in [0,100] driving the
      per-frame allocation.
- [x] `opus_encode` refactored onto `_opus_encode_impl` (CBR, unchanged) + new
      `opus_encode_vbr` (constrained VBR).
- Deferred: complexity from the actual MDCT/masking (vs the time-domain proxy);
  stereo/transient-aware allocation; a two-pass VBR.

## v2.5.x — remaining (every deferred tail now pinned)

Functional correctness first — make the codecs actually produce audio before adding
breadth. Order is dependency-first. Effort tags: small / medium / large. Each item
below was previously a "Deferred:" footnote or an undisclosed stub; they are now real.

### v2.5.9 — Opus/CELT decode → PCM audio · needs 2.5.1–2.5.8 · large

**The headline gap.** Wire `_opus_decode_celt_frame` / `_opus_decode_celt_stereo_frame`
into `opus_decode_from_packets` (today it returns an empty sample vector), and implement
the missing synthesis: magnitude reconstruction (unit shape × band energy) → IMDCT →
inter-frame **TDAC overlap-add** → PCM. Prove it with a **real waveform round-trip**
(encode a tone/sweep, decode, assert the output correlates with the input) — not an
internal symbol check. Until this ships, Opus decode produces no sound. (Deferred from 2.5.1.)

### v2.5.10 — Opus bitstream conformance · needs 2.5.9 · large

Decide and execute: either make the stream **RFC 6716-conformant** (real `ec_enc`
range coder, spec band-energy coding + bit allocation, correct TOC) so it interoperates
with libopus, **or** explicitly document + label it as a shravan-internal format. Today
it is neither conformant nor decodable to audio outside shravan's own tests.

### v2.5.11 — CELT transient completion · needs 2.5.9 · medium

Proper CELT **overlapping short-window** shapes, per-band **time-frequency resolution**,
and **cross-frame transient memory** (needs real IMDCT/overlap from 2.5.9). Today's path
swaps to 8 short MDCTs with per-block (non-overlapping) windows and no TF signalling.
(Deferred from 2.5.4.)

### v2.5.12 — CELT stereo completion · needs 2.5.9 · medium

**Stereo + transient combined** (today stereo forces a long MDCT — mutually exclusive),
**intensity stereo** for high bands, and a **non-even mid/side bit split**. (Deferred from 2.5.6.)

### v2.5.13 — Opus rate-control completion · needs 2.5.9/11/12 · medium

Drive VBR from the **actual MDCT/masking** complexity (not the time-domain first-difference
proxy), make allocation **stereo/transient-aware**, and add **two-pass VBR**. (Deferred from 2.5.8.)

### v2.5.14 — AAC TNS completion · medium

**Short-window** TNS and **stereo/CPE** TNS (today: AAC-LC long-window, mono/SCE only).
(Deferred from 2.5.2.)

### v2.5.15 — AAC psychoacoustic completion · medium

**Absolute-threshold-of-hearing** floor + **tonality-adjusted SMR** (needs SPL calibration),
and a **closed-loop scale-factor DPCM** fix — the AAC encoder transmits scale-factor deltas
clamped to ±60 but tracks the *unclamped* predictor, so encoder/decoder desync on large
deltas (a real correctness bug, not just a quality gap). (Deferred from 2.5.3.)

### v2.5.16 — MP3 decoder · large

Real MP3 **decode to PCM**: Huffman + requantization + alias reduction + IMDCT + synthesis
polyphase filterbank + stereo. Today `mp3_decode` is a metadata-only stub (empty samples).
(Newly pinned — was an undisclosed stub.)

### v2.5.17 — MP4 completion · small–medium

Full **`esds` / AudioSpecificConfig** parse (today: channels/rate lifted from the `mp4a`
entry, ADTS synthesized from that), **multi-track / edit-list** handling, and vendor the
AAC chain into the fuzz harness to **fuzz `mp4_decode`**. (Deferred from 2.5.7.)

### v2.5.18 — Fuzz-coverage completion · small

The fuzz harness covers FLAC / Ogg / MP3-scan / ID3 only. Add **AAC, MP4, and Opus
decode** targets so the untrusted-input paths shipped since 2.5.2 are actually fuzzed.
(Newly pinned — coverage gap.)

### v2.5.19 — Hi-res rates + wide PCM + SSE2 · independent · medium

88.2–384 kHz round-trip tests; **PCM_F64** (64-bit float) WAV encode/decode (today
`wav_encode` rejects `PCM_F64`); PCM conversion **SSE2/unrolled hot loops** with
before/after benchmarks (the perf item). Note: 32-bit int PCM already works.

### v2.5.20 — SILK mode (speech) · needs framework · large

LPC/LTP/LSF, excitation, shared range coder. Largest new subsystem.

### v2.5.21 — Hybrid mode (SILK + CELT) · needs 2.5.20 + 2.5.9 · large

SILK low-band + CELT high-band over one range coder. Completes the Opus encoder.

### v2.5.22 — FLAC LPC encoder · independent · medium

LPC prediction (beats Fixed); matters most for 24/32-bit hi-res lossless. (FLAC *decode*
already works; this is the encoder.)

### v2.5.23 — DSD (DSD64/128/256, DoP) · independent · medium

1-bit sigma-delta path.

### Backlog note

If the primary goal is a *working, interoperable Opus codec*, 2.5.9 + 2.5.10 are the
real "end of line" — everything from 2.5.11 on is refinement or breadth. The nine
"shipped" 2.5.0–2.5.8 releases are the entropy/allocation *scaffolding* for 2.5.9, not
a finished decoder.

---

## Resolved bugs

- ~~FLAC encoder segfault~~ — Fixed in cc3 3.3.11 (compiler codegen issue)
- ~~AAC decoder blocked on parser limit~~ — Fixed in cc3 3.3.17 (tok_names overflow)
- ~~tok_names regression in 3.4.x~~ — Fixed in cc3 3.4.3
