# shravan

**shravan** (Sanskrit: hearing / perception) -- Audio codecs for the AGNOS ecosystem.

WAV, FLAC, AIFF, ALAC, Ogg/Opus, MP3, and AAC-LC encoding/decoding. PCM sample format conversion, sinc resampling, FFT/MDCT, dithering, streaming decoders, and audio metadata tag reading/writing. Zero external dependencies. Pure Cyrius.

## Quick start

```sh
# Build
cyrius build src/main.cyr build/shravan

# Run tests (520 assertions)
./build/shravan

# Benchmarks
cyrius build src/bench.cyr build/bench
./build/bench
```

## Modules

| Module | Description |
|--------|-------------|
| **wav** | RIFF WAVE encoder/decoder (PCM 8/16/24/32-bit, IEEE float 32-bit) |
| **flac** | FLAC encoder/decoder (all subframe types, SEEKTABLE seeking, MD5) |
| **aiff** | AIFF/AIFF-C encoder/decoder (big-endian, 80-bit extended sample rate) |
| **alac** | Apple Lossless decoder (SCE/CPE/LFE, Rice coding, LPC prediction) |
| **ogg** | Ogg container parser/muxer (CRC-32, page/packet extraction) |
| **opus** | Opus CELT-mode encoder, header parsing, duration estimation |
| **mp3** | MP3 frame header parsing, ID3v2 tag skipping |
| **aac** | AAC-LC encoder/decoder (ADTS, Huffman codebooks, M/S stereo, short windows) |
| **pcm** | Sample format conversion (u8/i16/i24/i32/f32/f64), interleave/deinterleave |
| **fft** | Mixed-radix FFT (2,3,5), forward/inverse MDCT |
| **resample** | Windowed sinc interpolation (Draft/Good/Best quality) |
| **dither** | TPDF and noise-shaped dithering |
| **tag** | ID3v2 and Vorbis Comment reading and writing |
| **simd** | SIMD-style PCM conversion (unrolled scalar) |
| **stream** | Streaming decoders for WAV, FLAC, AIFF (chunked output) |
| **codec** | Auto-detect format and dispatch to decoder |

## Usage

Include `src/main.cyr` in your project. All codecs are available via the unified `codec_open()` interface:

```
include "path/to/shravan/src/main.cyr"

# Auto-detect and decode
var result = codec_open(data, len);
var fi = decode_result_info(result);
var samples = decode_result_samples(result);

# Encode WAV
var buf = alloc(65536);
var size = wav_encode(samples, 44100, 1, PCM_I16, buf);

# Read metadata
var meta = tag_read_id3v2(data, len);
var title = tag_meta_title(meta);
```

## Performance

Benchmarked against the previous Rust+LLVM implementation (v1.1.0):

| Benchmark | Cyrius | Rust (LLVM -O3) | Ratio |
|-----------|--------|-----------------|-------|
| FLAC decode 1s | 8.0 ms | 1.53 ms | 5.2x |
| FLAC encode 1s | 20.7 ms | 2.77 ms | 7.5x |
| WAV decode 1s | 2.0 ms | 47 us | 42x |
| Compile (full) | 420 ms | 6.9 s | **16x faster** |
| Binary size | 333 KB | 1.83 MB | **6x smaller** |
| Dependencies | 0 | 20 crates | **zero** |

## Consumers

- **tarang** -- media framework
- **jalwa** -- media player
- **dhvani** -- audio engine
- **shruti** -- DAW

## Requirements

- Cyrius >= 4.10.3
- Linux x86_64

## License

GPL-3.0-only
