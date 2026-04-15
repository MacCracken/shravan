# Development Roadmap

> **v2.2.0** — 520 tests, 475KB binary, cc3 >= 4.10.3.
> Stdlib modernization + performance optimization pass.
> MDCT 5.45x faster (N/2-point FFT rewrite), PCM 1.3-1.6x (div→mul),
> polyphase resampler, sorted Huffman decode, sankoch compression dep.

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

## Completed (v2.2.0)

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

## v2.3.0 — Completeness + Performance

### AAC gaps (external file compatibility)

- Spectral codebooks 1-4 (4-tuple codebooks for low-energy bands)
- Spectral codebooks 9-10 (unsigned pairs, larger magnitude range)
- TNS (Temporal Noise Shaping) — IIR filter before IMDCT
- MP4/M4A container support (currently ADTS only)
- ~~Huffman decoder optimization~~ (done: sorted codebook decode in v2.2.0; 2-level lookup table future)

### AAC encoder improvements

- Proper Huffman codebook selection per band (current uses escape pairs for all)
- Psychoacoustic model (masking thresholds)
- VBR mode
- M/S stereo encoding (current downmixes to mono)

### Performance

- ~~MDCT: N/2-point FFT rewrite~~ (done: 5.45x in v2.2.0)
- ~~Opus FFT: precomputed twiddles~~ (done: per-level twiddle tables in v2.2.0)
- ~~Resampler: polyphase filter bank~~ (done: 256-phase table in v2.2.0)
- PCM: inline asm SSE2 for i16/f64 hot loops
- FLAC encoder: LPC encoding (better compression than Fixed prediction)

### Streaming

- decode_file / decode_reader convenience helpers

## v2.4.0 — Own the stack

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

## Resolved bugs

- ~~FLAC encoder segfault~~ — Fixed in cc3 3.3.11 (compiler codegen issue)
- ~~AAC decoder blocked on parser limit~~ — Fixed in cc3 3.3.17 (tok_names overflow)
- ~~tok_names regression in 3.4.x~~ — Fixed in cc3 3.4.3
