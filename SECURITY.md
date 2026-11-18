# Security Policy

## Scope

shravan is an audio codec library that decodes untrusted audio data. It processes file headers, bitstreams, and compressed audio from potentially hostile sources. Security is critical.

## Attack Surface

| Area | Risk | Mitigation |
|------|------|------------|
| Format detection | Magic byte spoofing | `detect_format` validates multiple header fields, not just magic |
| WAV chunk parsing | Malformed chunk sizes | Bounds checks on all chunk offsets, `dat_size` clamped to available data |
| FLAC frame parsing | Crafted bitstream overflow | CRC-8/CRC-16 verification on every frame, bitreader bounds checking |
| FLAC SEEKTABLE | Corrupted seek points | Placeholder entries (0xFFFFFFFFFFFFFFFF) filtered, offset bounds checked |
| AAC ADTS parsing | Invalid sync/frame length | Sync word + layer validation, frame_length bounds check |
| AAC Huffman decode | Infinite loop on crafted codes | Max 20-bit codeword limit in `_aac_huff_decode` |
| AAC spectral data | Out-of-bounds coefficient write | Band boundaries clamped to AAC_FRAME_SIZE |
| ALAC bitreader | Reads past buffer | `bitreader_read` checks `bpos >= dlen` before every byte read |
| Ogg CRC-32 | Corrupted page acceptance | CRC-32 computed and verified on every page |
| ID3v2 tag size | Allocation overflow | Syncsafe integer (max 256MB), frame sizes bounded by tag size |
| Bump allocator | Memory exhaustion | Grows by 1MB chunks via brk; no individual free |
| Integer overflow | Large sample counts | All arithmetic uses 64-bit integers |

## Dependency Surface

shravan links no C libraries and pulls from no third-party package ecosystem. Its entire
dependency surface is two first-party AGNOS sources, both vendored into `lib/` and pinned:

| Source | Pinned by | Verified by |
|--------|-----------|-------------|
| Cyrius stdlib (declared `[deps].stdlib` subset) | `cyrius.cyml [package].cyrius` toolchain pin | per-file SHA-256 in `cyrius.lock` |
| sankoch (compression) | `[deps.sankoch].tag` + resolved commit SHA in `cyrius.lock` | per-file SHA-256 in `cyrius.lock` |

That is a small surface, but it is not zero: a compromised upstream tag would reach a build that
re-resolves. `cyrius.lock` is the control — it records the resolved commit SHA and a SHA-256 for
every vendored file, and `cyrius deps --verify` fails the build on any mismatch. Run it after any
toolchain or dependency bump, and review the `lib/` diff as you would any other code.

All shravan-authored code is auditable in `src/*.cyr`; `lib/*.cyr` is vendored upstream.

## Fuzzing

Three harnesses, all gated in CI and run in full before each release:

| Harness | Reaches | Strategy |
|---------|---------|----------|
| `fuzz/fuzz_codecs.cyr` | FLAC metadata, Ogg pages, MP3 frame scan, ID3v2 | random bytes behind the format magic |
| `fuzz/fuzz_decode.cyr` | decode: `codec_open`, WAV, AIFF, ALAC, FLAC (+range), MP3, Ogg, MP4, AAC, the streaming decoders, ID3v2 (compressed, grouped, encrypted frames), Vorbis comments, Opus on libopus packets; encode: FLAC, AAC, SILK, `opus_encode`, the CELT encoder | random-behind-magic, valid files with N bytes corrupted, and for the encoders hostile f64 input (NaN, infinities, huge, denormal), every ctl in and out of its bounds, and buffers from none to past the largest frame |
| `fuzz/fuzz_opus.cyr` | the RFC 6716 Opus decoder | the suite's libopus packets damaged at random (bytes flipped, cut short, lost, replaced) behind a hostile OpusHead |

The corrupted-valid-file strategy is what reaches deep decode paths; random bytes rarely survive
header validation long enough to get there. Seeds include real ffmpeg, flac, lame and libopus
output, because shravan's own encoders never write FLAC LPC, ALAC, TNS, short blocks or an esds.

```sh
./fuzz/run.sh 90000            # the gate: >= 90,000 calls per harness, plus cold starts
./fuzz/run.sh 90000 --poison   # the same under the allocator's poison mode (redzones)
```

The gate's last run made 990,234 calls (810,000, 90,234 and 90,000) with 2,200 cold starts and no
crash, in both modes. A cold start is one call in a fresh process. It catches a table read before
its lazy initialiser runs, which a warm process hides.

`alloc()` is a bump allocator that never frees, so `fuzz_decode` and `fuzz_opus` are swept across
several seeded processes rather than run as one. A fixed seed keeps any failure reproducible, and a
failing batch prints the command that reproduces it.

Untrusted input is bounded rather than trusted: decoders cap total decoded samples, allocation
sizes are checked against a 256 MB policy ceiling, and every chunk body is required to be inside
the buffer before it is read. See the `SEC-0NN` markers in `src/` for the individual fixes and
the CHANGELOG entry that introduced each.

## Reporting Vulnerabilities

Report security issues to the repository maintainer via GitHub Security Advisories. Do not file public issues for security vulnerabilities.
