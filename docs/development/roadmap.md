# Development Roadmap

> **v2.0.0** — Full Cyrius port. 18 modules, 499 tests, 9 benchmarks.
> AAC-LC decoder from scratch (no symphonia). Rust source removed (tag `1.1.0`).
> cc3 >= 3.4.3 required.

## Completed (v2.0.0)

- [x] Full Rust -> Cyrius port (10,265 -> 11,780 lines)
- [x] All codec modules: WAV, AIFF, FLAC, ALAC, Ogg, MP3, Opus, AAC
- [x] AAC-LC decoder from scratch (bitreader, section/SF parsing, spectral decode, inverse quant, IMDCT, overlap-add)
- [x] FLAC SEEKTABLE parsing + decode_range() for sample-accurate seeking
- [x] Opus decode path (OpusHead/OpusTags parsing, granule-based duration, ogg_decode dispatch)
- [x] codec_open dispatches all 8 formats (WAV, AIFF, FLAC, OGG/Opus, MP3, AAC, ALAC)
- [x] Stream chunking (WAV/AIFF chunk_frames functional)
- [x] 499 test assertions, 0 failures
- [x] Benchmarks: FLAC 5.2x of Rust+LLVM, WAV 42x, compile 16x faster

## v2.1.0 — Quality

### AAC decoder improvements

- Standard Huffman codebooks (11 spectral codebooks) for decoding external AAC files
- Short window support (8x128 for transients)
- M/S stereo decoding
- TNS (Temporal Noise Shaping)
- MP4/M4A container support (currently ADTS only)

### AAC encoder improvements

- Proper Huffman codebook selection (current uses escape pairs for all bands)
- Psychoacoustic model (masking thresholds)
- VBR mode

### Metadata writing

- ID3v2 tag writing (currently read-only)
- Vorbis Comment writing (currently read-only)

### Streaming improvements

- True incremental FLAC decode (frame-by-frame with sample offset tracking)
- decode_file / decode_reader convenience helpers

## v2.2.0 — Performance

### Opus encoder (current ~27ms/s -> target <10ms/s)

- Specialized radix-2/3/5 FFT butterflies with precomputed twiddle tables
- N/4-point MDCT via proper folding (current uses 2N-point FFT — 4x more work)
- Cache-friendly memory layout for FFT scratch buffers

### Resampler (current ~5ms/4096 samples -> target <1ms)

- Polyphase filter bank structure
- SIMD inner loop for polyphase convolution
- Pre-tabulated sinc coefficients per quality level

### FLAC encoder (current ~21ms/s)

- LPC encoding (currently Fixed prediction only — LPC gives 5-15% better compression)
- Adaptive block sizing based on signal characteristics

### PCM conversion (current 166us/4096 -> target <20us)

- Inline asm SSE2 for i16/f64 hot loops
- Hand-unrolled 4x processing

## v2.3.0 — Own the stack

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
