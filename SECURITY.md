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

Two harnesses, both gated in CI and run in full before each release:

| Harness | Reaches | Strategy |
|---------|---------|----------|
| `fuzz/fuzz_codecs.cyr` | FLAC metadata, Ogg pages, MP3 frame scan, ID3v2 | random bytes behind the format magic |
| `fuzz/fuzz_decode.cyr` | `codec_open`, WAV, AIFF, ALAC, FLAC (+range), MP3, Ogg, MP4, AAC, Vorbis comments | random-behind-magic **and** valid files built by shravan's own encoders with N bytes corrupted |

The corrupted-valid-file strategy is what reaches deep decode paths; random bytes rarely survive
header validation long enough to get there.

```sh
./fuzz/run.sh 90000     # current gate: 903,500 calls, 0 crashes
```

`alloc()` is a bump allocator that never frees, so `fuzz_decode` is swept across several seeded
processes rather than run as one — a fixed seed keeps any failure reproducible, and a failing
batch prints the seed that reproduces it.

Untrusted input is bounded rather than trusted: decoders cap total decoded samples, allocation
sizes are checked against a 256 MB policy ceiling, and every chunk body is required to be inside
the buffer before it is read. See the `SEC-0NN` markers in `src/` for the individual fixes and
the CHANGELOG entry that introduced each.

## Reporting Vulnerabilities

Report security issues to the repository maintainer via GitHub Security Advisories. Do not file public issues for security vulnerabilities.
