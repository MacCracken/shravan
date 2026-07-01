# Development Roadmap

> **v2.4.3** — 563 tests + fuzz (90K/0), `dist/shravan.cyr` bundle, Cyrius 6.3.19.
> Distlib bundle for consumers; codec modules relocated `lib/` → `src/` (lib/ =
> stdlib only). (v2.4.2: fuzz rebuilt + `cyrius vet`/fuzz-smoke in CI. v2.4.1:
> full serde type coverage. v2.4.0: `cyrius.cyml`, `lib sync`, `ganita`, `bayan`.)
> (v2.3.0: security audit 21/21 fixed, 90K fuzz 0 crashes; MDCT 5.45x, polyphase resampler.)

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

## v2.4.x — Own the stack (proposed — pending review)

### Opus encoder

- SILK mode for speech content
- Hybrid mode (SILK + CELT)
- VBR support
- Stereo coupling (dual-coded stereo instead of mono downmix)
- Full PVQ spectral shape coding (current is sign-only)
- Transient detection and short-window switching

### High-resolution audio

- 88.2/96/176.4/192/352.8/384 kHz sample rates
- 32-bit integer, 64-bit float PCM
- DSD support (DSD64/DSD128/DSD256, DoP)

---

## v2.5.x — Completeness (deferred from 2.3.x)

### v2.5.1 — TNS (Temporal Noise Shaping)

- IIR filter before IMDCT for AAC decoder
- TNS encoding support

### v2.5.2 — MP4/M4A container

- MP4 box parsing (moov/trak/mdia/stbl)
- AAC extraction from MP4 container
- Currently ADTS-only — this enables real-world .m4a files
- `sankoch` (compression) is already a dependency — wire it in for container payloads

### v2.5.3 — Performance

- PCM: inline asm SSE2 for i16/f64 hot loops
- FLAC encoder: LPC encoding (better compression than Fixed prediction)
- AAC: psychoacoustic model (masking thresholds)

---

## Resolved bugs

- ~~FLAC encoder segfault~~ — Fixed in cc3 3.3.11 (compiler codegen issue)
- ~~AAC decoder blocked on parser limit~~ — Fixed in cc3 3.3.17 (tok_names overflow)
- ~~tok_names regression in 3.4.x~~ — Fixed in cc3 3.4.3
