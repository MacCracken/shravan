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

## Zero External Dependencies

shravan has zero external dependencies. No C libraries, no crate ecosystem, no supply chain attack surface. The entire codebase is auditable in `src/main.cyr` and `lib/*.cyr`.

## Reporting Vulnerabilities

Report security issues to the repository maintainer via GitHub Security Advisories. Do not file public issues for security vulnerabilities.
