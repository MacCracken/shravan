# Contributing to shravan

Thank you for your interest in contributing to shravan.

## Development Workflow

1. Fork and clone the repository
2. Create a feature branch from `main`
3. Make your changes
4. Run the cleanliness check (see below)
5. Open a pull request

## Prerequisites

- Cyrius >= 4.10.3 (`cyrius --version`)
- Linux x86_64

## Cleanliness Check

Every change must pass:

```sh
# Build
cyrius build src/main.cyr build/shravan

# Test (must show "0 failed")
./build/shravan

# Build benchmarks
cyrius build src/bench.cyr build/bench
./build/bench
```

## Code Conventions

- All samples are f64 internally (Cyrius native, higher precision than f32)
- Error handling via packed Result (negative = error)
- Initialize f64 constants via `f64_from()` at startup (never hardcode bit patterns)
- `>>` is logical (unsigned) shift -- use conditional subtraction for sign extension
- Entry point is top-level statements, not `fn main()`
- Large buffers via `alloc()`, not stack arrays (code buffer limit)
- Prefix all public functions: `wav_`, `flac_`, `aac_`, `tag_`, etc.
- Do not use reserved keywords as variable names (`match`, `default`, `shared`, `in`)

## Adding a New Codec

1. Create `lib/mycodec.cyr` with encode/decode functions
2. Add `include "lib/mycodec.cyr"` in `src/main.cyr` before the codec module
3. Add format detection in `detect_format()` and dispatch in `codec_open()`
4. Add tests (encode/decode roundtrip, error handling)
5. Add benchmarks in `src/bench.cyr`
6. Update CHANGELOG.md and docs/development/roadmap.md

## Scope

shravan is an audio codec library. It handles:
- Audio format encode/decode (WAV, FLAC, AIFF, Ogg/Opus, AAC, ALAC, MP3)
- PCM sample format conversion
- Audio metadata (ID3v2, Vorbis Comment)
- Signal processing primitives (FFT, MDCT, resampling, dithering)

It does NOT handle:
- Audio I/O (playback, recording) -- that's dhvani
- Media containers (MP4, MKV) -- that's tarang
- Audio effects (reverb, EQ) -- that's dhvani

## License

By contributing, you agree that your contributions will be licensed under GPL-3.0-only.
