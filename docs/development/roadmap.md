# Development Roadmap

## v1.1.0

- High-resolution audio support (88.2/96/176.4/192/352.8/384 kHz sample rates, 32-bit integer, 64-bit float)
- Metadata writing (ID3v2, Vorbis Comment)
- Waveform analysis utilities
- Memory-mapped file support (mmap)
- LPC encoding in FLAC encoder
- Async runtime adapters (tokio, async-std)

## v1.3.0 — Performance

### Opus encoder (27ms/s → target <10ms/s)

- Specialized radix-2/3/5 FFT butterflies with precomputed twiddle tables (current generic combine is O(R×N) per stage)
- N/4-point MDCT via proper folding (current uses 2N-point FFT — 4x more work than necessary)
- Cache-friendly memory layout for FFT scratch buffers

### Resampler (3.3ms/4096 samples → target <1ms)

- Polyphase filter bank structure (avoid recomputing sinc taps per output sample)
- SIMD inner loop for polyphase convolution (extend existing `weighted_sum`)
- Pre-tabulated sinc coefficients per quality level

### FLAC encoder (1.35ms/s — already good, compression ratio improvements)

- LPC encoding (currently Fixed prediction only — LPC gives 5-15% better compression)
- Adaptive block sizing based on signal characteristics

## Future

### Codec gaps — All Done

tarang C FFI deps eliminated:
- ~~Opus encode (libopus)~~ — Done: CELT-mode, FFT-based MDCT
- ~~AAC decode (fdk-aac)~~ — Done: symphonia-codec-aac bridge
- ~~AAC encode (fdk-aac)~~ — Done: from-scratch AAC-LC, ADTS output
- ~~ALAC decode (symphonia)~~ — Done: from-scratch, no_std

### Own the stack — Opus encoder

- SILK mode for speech content
- Hybrid mode (SILK + CELT)
- VBR support
- Stereo coupling (dual-coded stereo instead of mono downmix)
- ~~FFT-based MDCT~~ Done (2N-point mixed-radix FFT, 430ms→27ms)
- Full PVQ spectral shape coding (current is sign-only)
- Transient detection and short-window switching

### Own the stack — AAC encoder

- Proper Huffman codebook selection (current uses escape pairs for all bands)
- Short window support for transients
- VBR mode
- Psychoacoustic model (masking thresholds)
- M/S stereo coding

### Own the stack — AAC decoder

- Replace symphonia-codec-aac with native implementation (remove std dependency)
- MP4/M4A container support (currently ADTS only)

### Other

- DSD support (DSD64/DSD128/DSD256, DoP)

## v1.0 Criteria — All Met

- [x] All FLAC subframe types decoded (Constant, Verbatim, Fixed 0-4, LPC 1-32)
- [x] WAV + FLAC encode/decode tested against reference implementations (ffmpeg)
- [x] Streaming API stable (StreamDecoder trait, WAV/FLAC/AIFF decoders)
- [x] Performance within 2x of C reference implementations
- [x] 85%+ test coverage (90%+ excluding platform-conditional dead code)
- [ ] Published on crates.io (user handles)
