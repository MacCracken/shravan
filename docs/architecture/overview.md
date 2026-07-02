# Architecture Overview

## Module Map

```
shravan/
  src/
    shravan.cyr     -- library: error, format, PCM, WAV, AIFF, ALAC, codec dispatch,
                       decode_file/reader (first [lib] bundle module)
    flac.cyr        -- FLAC encoder/decoder (all subframe types, SEEKTABLE, MD5)
    ogg.cyr         -- Ogg container parser/muxer (CRC-32, page extraction)
    mp3.cyr         -- MP3 frame header parsing, ID3v2 skipping
    tag.cyr         -- ID3v2 + Vorbis Comment reading and writing
    fft.cyr         -- Mixed-radix FFT (2,3,5), forward/inverse MDCT
    opus.cyr        -- Opus CELT-mode encoder, OpusHead/OpusTags parsing
    aac.cyr         -- AAC-LC encoder/decoder (ADTS, Huffman, M/S, short windows)
    mp4.cyr         -- MP4/M4A container demux (box tree, sample table, AAC -> aac_decode)
    resample.cyr    -- Windowed sinc resampler (Draft/Good/Best)
    dither.cyr      -- TPDF + noise-shaped dithering
    simd.cyr        -- SIMD-style PCM conversions (unrolled scalar)
    stream.cyr      -- Streaming decoders (WAV, FLAC, AIFF, chunked output)
    serde.cyr       -- JSON serialization of format/pcm/error/FormatInfo/... (bayan, #derive(Serialize))
    main.cyr        -- test harness entry (includes shravan.cyr + codecs, runs tests + init)
    bench.cyr       -- benchmarks (clock_gettime timing)
  dist/
    shravan.cyr     -- distlib bundle (all src [lib] modules concatenated; consumers include this)
  lib/              -- vendored Cyrius stdlib ONLY (alloc, vec, str, math, ganita, io,
                       syscalls, fmt, string, args, assert, tagged, thread, fnptr, bayan, …),
                       version-matched to the cyrius.cyml pin via `cyrius lib sync`
```

## Data Flow

```
Raw bytes --> detect_format() --> AudioFormat enum
Raw bytes --> codec_open()    --> decode_result (FormatInfo + samples vec)
               |
               +-- wav_decode()   --> parse RIFF/fmt/data --> PCM to f64
               +-- flac_decode()  --> parse STREAMINFO/frames --> subframes --> f64
               +-- aiff_decode()  --> parse FORM/COMM/SSND --> PCM to f64
               +-- alac_decode()  --> parse config/frames --> Rice/LPC --> f64
               +-- ogg_decode()   --> extract packets --> opus_decode_from_packets()
               +-- mp3_decode()   --> frame scan --> FormatInfo (no audio)
               +-- aac_decode()   --> ADTS frames --> ICS/spectral/IMDCT --> f64
               +-- codec_open()   --> also handles FMT_ALAC, FMT_OPUS

f64 samples --> wav_encode()   --> RIFF WAVE bytes
f64 samples --> flac_encode()  --> FLAC bitstream (Fixed prediction, CRC, MD5)
f64 samples --> aiff_encode()  --> AIFF bytes (big-endian)
f64 samples --> opus_encode()  --> Ogg/Opus bitstream (CELT mode)
f64 samples --> aac_encode()   --> ADTS/AAC-LC bitstream
f64 samples --> resample()     --> f64 samples at new rate
f64 samples --> dither_tpdf()  --> f64 samples with dither noise

tag_read_id3v2()  --> AudioMetadata struct
tag_write_id3v2() --> ID3v2 bytes
tag_read_vorbis() --> AudioMetadata struct
tag_write_vorbis()--> Vorbis Comment bytes
```

## Consumers

| Consumer | Usage |
|----------|-------|
| **tarang** | media framework -- full codec suite |
| **jalwa** | media player -- decode + playback |
| **dhvani** | audio engine -- PCM, resample, stream |
| **shruti** | DAW -- full codec suite |

## Design Decisions

- **f64 internally**: Cyrius native SSE2 double-precision. Higher precision than f32, simpler code (no f32 bit manipulation needed except for WAV IEEE float I/O).
- **Distlib bundle**: No separate compilation. Consumers `include "dist/shravan.cyr"` (the `cyrius distlib` bundle of `src/shravan.cyr` + codec modules) and get all codecs; they supply stdlib + bayan + sankoch from their own manifest and call `shravan_init_constants()` at startup.
- **Packed Result**: Negative values are errors (bit 63 set). No heap allocation for error paths. `is_err(r)` is a single comparison.
- **Caller-provided buffers**: Encode functions take an output buffer pointer. Decode functions return a vec (heap-allocated via bump allocator).
- **Bump allocator**: No individual free. Working memory grows monotonically. Suitable for batch processing; streaming use cases should be aware of memory growth.
