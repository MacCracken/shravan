# shravan

**shravan** (Sanskrit: hearing / perception) -- Audio codecs for the AGNOS ecosystem.

WAV, FLAC, AIFF, ALAC, Ogg/Opus, MP3, AAC-LC, and MP4/M4A. Full **Opus decode** (CELT + SILK +
hybrid, mono/stereo, 10/20 ms, all bandwidths, bit-exact vs libopus) and a real **RFC-6716 CELT
encoder** (libopus decodes shravan's frames sample-identically). PCM sample format conversion,
sinc resampling, FFT/MDCT, dithering, streaming decoders, and audio metadata tags. Pure Cyrius:
the Cyrius stdlib plus two first-party libraries, sankoch (compression) and bayan-json.

Every decoder is checked against a reference (ffmpeg, flac, mpg123, libopus, the ISO MPEG
conformance vectors) on real encoder output, in the suite and in a CI sweep.

## Quick start

```sh
# Build + run the test suite (12,960 assertions)
./scripts/test-all.sh

# Sweep against the reference decoders and encoders (ffmpeg, flac, mpg123, libopus)
./scripts/interop.sh

# Hostile input: >= 90K calls per fuzz harness, plus cold starts
./fuzz/run.sh 90000

# Benchmarks
cyrius build src/bench.cyr build/bench
./build/bench
```

## Modules

| Module | Description |
|--------|-------------|
| **wav** | RIFF WAVE encoder/decoder (PCM 8/16/24/32-bit, any depth to 32 in whole-byte containers, IEEE float 32/64-bit, EXTENSIBLE) |
| **flac** | FLAC decoder (all subframe types, SEEKTABLE seeking, `flac_verify` MD5 check), encoder (Fixed prediction) |
| **aiff** | AIFF/AIFF-C encoder/decoder (big-endian, 80-bit extended sample rate; AIFF-C sowt, fl32, fl64) |
| **alac** | Apple Lossless decoder (SCE/CPE/LFE, Rice coding, LPC prediction) |
| **ogg** | Ogg container parser/muxer (CRC-32, page/packet extraction) |
| **opus** | Opus decode — CELT + SILK + hybrid, mono/stereo, every frame size and bandwidth, FEC, packet-loss concealment, DTX, matching libopus; RFC-6716 CELT-frame encoder; header parsing (output gain applied) |
| **silk** | Opus SILK-mode decoder (NLSF/LTP/LPC synthesis + resampler), bit-exact vs libopus |
| **mp3** | MP3 decoder to PCM (MPEG-1/2/2.5 Layer I/II/III — reservoir, Huffman, requant, IMDCT, polyphase synthesis, intensity stereo, free format); ID3v2 tag skipping |
| **aac** | AAC-LC decoder (mono/stereo, all 13 sample rates, TNS, short windows, M/S, intensity, PNS), encoder (ADTS, long windows, M/S, intensity, PNS) |
| **mp4** | MP4/M4A and QuickTime demux (box tree, sample table, esds, AAC → aac_decode, ALAC → alac_decode_frames) |
| **serde** | JSON serialization of format/pcm/error + codec config/metadata types |
| **pcm** | Sample format conversion (u8/i16/i24/i32/f32/f64), interleave/deinterleave |
| **fft** | Mixed-radix FFT (2,3,5), cached radix-2 FFT, FFT-based forward/inverse MDCT |
| **resample** | Windowed sinc interpolation (Draft/Good/Best quality) |
| **dither** | TPDF and noise-shaped dithering |
| **tag** | ID3v2 (v2.2–v2.4, compressed frames) and Vorbis Comment reading and writing |
| **simd** | SIMD-style PCM conversion (unrolled scalar) |
| **stream** | Incremental streaming decoders for WAV, AIFF, FLAC: any split, bounded buffer, no length limit |
| **codec** | Auto-detect format and dispatch to decoder |

## Usage

Include the self-contained distlib bundle `dist/shravan.cyr` in your project (supply stdlib +
bayan + sankoch from your own manifest). All codecs are available via the unified `codec_open()`
interface:

```
include "path/to/shravan/dist/shravan.cyr"

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

One-shot decodes hold the whole result in memory, so they stop at a ceiling: 2^26 interleaved samples
by default (about 12.7 minutes of 44.1 kHz stereo), past which they return `ERR_TOO_LARGE` before
allocating. Raise or lower it with `shravan_set_max_samples(n)`, which returns the previous value, or
stream instead. The streaming decoders take bytes as they arrive, in pieces of any size, and have no
ceiling:

```
var dr = decode_reader(4096);              # WAV, AIFF or FLAC, detected from the first bytes
var events = decode_reader_feed(dr, chunk, chunk_len);
# each event: stream_evt_type(ev) is STREAM_EVT_HEADER (FormatInfo), STREAM_EVT_SAMPLES
# (a vec of interleaved f64), STREAM_EVT_ERROR (a packed error) or, from
# decode_reader_flush(dr), STREAM_EVT_END; stream_evt_data(ev) carries the payload
```

## Performance

`src/bench.cyr` on the library itself, v2.9.3, one core (the history is in `bench-history.csv`):

| Benchmark | Time | Real-time multiple |
|-----------|------|--------------------|
| WAV decode, 1 s mono 16-bit | 170 µs | ~5900× |
| FLAC decode, 1 s mono 16-bit | 3.25 ms | ~300× |
| FLAC encode, 1 s mono 16-bit | 10.8 ms | ~90× |
| AAC decode, 1 s stereo | 22 ms | ~45× |
| AAC encode, 1 s stereo | 50 ms | ~20× |
| Opus CELT decode, one 20 ms frame | 2.1 ms | ~10× |
| MDCT, 2048 points | 54 µs | |

On real files through `codec_open` (the interop CLI): 30 s of transient-rich AAC stereo decodes in
0.47 s, a minute of 16-bit stereo FLAC in 0.39 s, and the same FLAC streamed in 4 KiB pieces in 0.50 s.

The v2.0.0 port's comparison with the Rust+LLVM implementation it replaced (v1.1.0) is in
`benchmarks-rust-v-cyrius.md`.

## Consumers

- **tarang** -- media framework
- **jalwa** -- media player
- **dhvani** -- audio engine
- **shruti** -- DAW

## Requirements

- Cyrius 6.7.18 (pinned in `cyrius.cyml [package].cyrius`)
- Linux x86_64

## License

GPL-3.0-only
