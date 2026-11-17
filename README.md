# shravan

**shravan** (Sanskrit: hearing / perception) -- Audio codecs for the AGNOS ecosystem.

WAV, FLAC, AIFF, ALAC, Ogg/Opus, MP3, AAC-LC, and MP4/M4A. Full **Opus decode** (CELT + SILK +
hybrid, mono/stereo, 10/20 ms, all bandwidths, bit-exact vs libopus), a real **RFC-6716 CELT
encoder** (libopus decodes shravan's frames sample-identically) and a **SILK encoder** (libopus's own
packets, byte for byte, voiced and unvoiced, mono and stereo). PCM sample format conversion,
sinc resampling, FFT/MDCT, dithering, streaming decoders, and audio metadata tags. Pure Cyrius:
the Cyrius stdlib plus two first-party libraries, sankoch (compression) and bayan-json.

Every decoder is checked against a reference (ffmpeg, flac, mpg123, libopus, the ISO MPEG
conformance vectors) on real encoder output, in the suite and in a CI sweep.

## Quick start

```sh
# Build + run the test suite (15,550 assertions)
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
| **flac** | FLAC decoder (all subframe types, SEEKTABLE seeking, `flac_verify` MD5 check); encoder at levels 0-8, the levels of `flac -0` to `-8` (LPC to order 12, partitioned Rice, stereo decorrelation, wasted bits, seek table), sizes within 0.1% of `flac`'s |
| **aiff** | AIFF/AIFF-C encoder/decoder (big-endian, 80-bit extended sample rate; AIFF-C sowt, fl32, fl64) |
| **alac** | Apple Lossless decoder (SCE/CPE/LFE, Rice coding, LPC prediction) |
| **ogg** | Ogg container parser/muxer (CRC-32, page/packet extraction) |
| **opus** | Opus decode — CELT + SILK + hybrid, mono/stereo and multistream (surround and ambisonics, RFC 7845 families 1, 2, 255), every frame size and bandwidth, FEC, packet-loss concealment, DTX, matching libopus; RFC-6716 CELT-frame encoder; header parsing (output gain applied) |
| **silk** | Opus SILK-mode decoder (NLSF/LTP/LPC synthesis + resampler), bit-exact vs libopus; SILK encoder (mono and stereo, 8/12/16 kHz from an 8–48 kHz input, 10–60 ms packets, VBR and CBR, complexity 0–1; pitch estimation and long-term prediction for voiced frames; LTP scaled by `silk_encoder_set_packet_loss`; in-band FEC and DTX; stereo as mid and side with the side predicted from the mid, mid-only frames at low rates, stereo input coded mono, moves between them packet by packet), libopus's `silk_Encode` packets byte for byte |
| **mp3** | MP3 decoder to PCM (MPEG-1/2/2.5 Layer I/II/III — reservoir, Huffman, requant, IMDCT, polyphase synthesis, intensity stereo, free format); ID3v2 tag skipping; gapless from the LAME tag |
| **aac** | AAC-LC decoder (any channel layout, to 22.2, in ffmpeg's channel order; all 13 sample rates, TNS, short windows, M/S, intensity, PNS), encoder (ADTS, all 13 rates: 3GPP psychoacoustics with tonality, block switching, TNS, M/S, intensity, PNS, rate loop with a bit reservoir) |
| **mp4** | MP4/M4A and QuickTime demux (box tree, sample table, esds and its channel layout, AAC and ALAC access units to their decoders); every audio track by index, the default as ffmpeg picks it; gapless (sample durations, edit lists, iTunSMPB) |
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

# Encode SILK: mono, a 48 kHz input coded wideband (internal 16 kHz)
var enc = silk_encoder_new_api(48000, 16000, 20, 20000, 1, 0);  # 20 ms, 20 kbit/s VBR, complexity 1
silk_encoder_set_packet_loss(enc, 10);               # optional: expect 10% loss (scales the LTP)
silk_encoder_set_fec(enc, 1);                        # optional: in-band FEC (LBRR frames)
silk_encoder_set_dtx(enc, 1);                        # optional: leave out packets of silence
var pkt = alloc(1276);
var n = silk_encode_opus(enc, pcm, pkt, 1276);       # pcm: silk_encoder_samples(enc) f64 samples

# Encode SILK in stereo: 48 kHz interleaved left/right, mid and side at 32 kbit/s in all
var st = silk_encoder_new_channels(48000, 16000, 2, 20, 32000, 1, 0);
var m = silk_encode_opus(st, lr, pkt, 1276);         # lr: 2 * silk_encoder_samples(st) f64 samples
silk_encoder_set_bitrate(st, 16000);                 # optional: a new rate from the next packet
silk_encoder_set_to_mono(st, 1);                     # optional, going to mono: one packet narrowed
m = silk_encode_opus(st, lr, pkt, 1276);             #   to nothing,
silk_encoder_set_to_mono(st, 0);
silk_encoder_set_channels(st, 1);                    #   then the channels' average, coded mono
```

One-shot decodes hold the whole result in memory, so they stop at a ceiling: 2^26 interleaved samples
by default (about 12.7 minutes of 44.1 kHz stereo), past which they return `ERR_TOO_LARGE` before
allocating. Raise or lower it with `shravan_set_max_samples(n)`, which returns the previous value, or
stream instead.

MP4 and MP3 decode gapless, as ffmpeg plays them: an AAC file comes out at its source's length, without
the encoder's priming or the last frame's padding. MP4 uses the sample table, the edit list and iTunes'
`iTunSMPB`; MP3 uses the LAME tag's delay and padding. `shravan_set_gapless(0)` (it returns the previous
setting) decodes every frame whole instead, as before 2.10.2. An MP4 with several audio tracks decodes the
one ffmpeg would play; `mp4_track_count`, `mp4_track_info` and `mp4_decode_track` reach the others.

Multichannel audio comes out interleaved in ffmpeg's channel order, whatever the codec: 5.1 as FL FR FC
LFE BL BR, from AAC (any channel configuration or PCE, to 22.2), ALAC, and Opus mapping family 1. Opus
families 2 (ambisonics) and 255 keep the stream's mapping order, as RFC 7845 assigns them no layout.

The streaming decoders take bytes as they arrive, in pieces of any size, and have no ceiling:

```
var dr = decode_reader(4096);              # WAV, AIFF or FLAC, detected from the first bytes
var events = decode_reader_feed(dr, chunk, chunk_len);
# each event: stream_evt_type(ev) is STREAM_EVT_HEADER (FormatInfo), STREAM_EVT_SAMPLES
# (a vec of interleaved f64), STREAM_EVT_ERROR (a packed error) or, from
# decode_reader_flush(dr), STREAM_EVT_END; stream_evt_data(ev) carries the payload
```

## Performance

`src/bench.cyr` on the library itself, v2.10.4 (the SILK encoder v2.12.2), one core (the history is in
`bench-history.csv`):

| Benchmark | Time | Real-time multiple |
|-----------|------|--------------------|
| WAV decode, 1 s mono 16-bit | 172 µs | ~5800× |
| FLAC decode, 1 s mono 16-bit | 3.24 ms | ~310× |
| FLAC encode, 1 s mono 16-bit (level 5) | 5.70 ms | ~175× |
| AAC decode, 1 s stereo, 128 kbit/s | 16.2 ms | ~62× |
| AAC decode, 5.1 at 240 kbit/s (0.28 s) | 12.4 ms | ~22× |
| AAC encode, 1 s stereo, 128 kbit/s | 53 ms | ~19× |
| MP4 (AAC) decode, 1 s stereo, 128 kbit/s | 15.8 ms | ~63× |
| MP3 decode, 44.1 kHz stereo, 64 kbit/s | 63 ms a second | ~16× |
| Opus CELT decode, one 20 ms frame | 130 µs | ~150× |
| Opus CELT encode, one 20 ms frame | 220 µs | ~90× |
| SILK encode, 1 s wideband voiced, 20 kbit/s VBR | 42.6 ms | ~23× |
| SILK encode, 1 s wideband stereo, 32 kbit/s VBR | 76.3 ms | ~13× |
| SILK encode, 1 s 48 kHz voiced to wideband, FEC on | 72.8 ms | ~14× |
| SILK encode, 1 s wideband noise, 20 kbit/s VBR | 31.9 ms | ~31× |
| SILK encode, 1 s wideband noise, 12 kbit/s CBR | 71.2 ms | ~14× |
| Opus decode, 5.1 (four streams, 0.4 s) | 17.3 ms | ~23× |
| MDCT, 2048 points | 55 µs | |

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
