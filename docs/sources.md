# Source citations

Where shravan's non-obvious constants, tables and formulas come from, and how
each was verified. The rule is that a table is either derived from something
checkable or measured against a reference — never transcribed on trust.

## AAC spectral Huffman codebooks (`src/aac.cyr`)

**HCB1, HCB2, HCB3, HCB4 and HCB6 were measured, not transcribed.**

### Why

v2.6.9's audit found four of them structurally impossible. Checking the tables
straight out of the source:

| Codebook | Kraft-McMillan sum | Verdict |
|----------|--------------------|---------|
| HCB1 | 1 | valid lengths, but a duplicate codeword (indices 52 and 76 both `len 7 / 0x06e`) and 8 prefix collisions |
| HCB2 | 15/16 = 0.9375 | incomplete — 9 symbols unreachable |
| HCB3 | 4257/4096 = 1.0393 | **> 1: impossible for any prefix code** — 32 symbols unreachable |
| HCB4 | 3971/4096 = 0.9695 | incomplete — 28 symbols unreachable |
| HCB6 | — | no table at all; cb 6 was decoded with HCB5's codes |
| HCB5, 7, 8, 9, 10, scalefactor | 1 | valid, prefix-free, complete |

A codebook that is not a complete prefix code corrupts decoding two ways: some
symbols can never be emitted, and a codeword that is a prefix of another makes
the decoder consume the wrong number of bits, desynchronising the rest of the
frame. Real encoders use these codebooks heavily — a survey of an ffmpeg-encoded
sample found cb 1, 2, 3, 4 and 6 all in use, cb 6 across 326 coded bands.

### How they were obtained

By **measuring a reference decoder**, not by copying a table from anywhere:

1. Build a minimal ISO/IEC 14496-3 AAC-LC ADTS frame in which one scalefactor
   band uses the codebook under test and the spectral data is a chosen bit
   pattern (`scratchpad/aacframe.py` in the working notes).
2. Decode it with `ffmpeg` and recover the integer quantized coefficients from
   the PCM: with only the first bins non-zero the output block is a known linear
   combination of windowed cosine basis functions, so least squares recovers
   them exactly, and inverting the AAC dequantizer (`x = sign(q)·|q|^(4/3)·
   2^((sf−100)/4)`) gives back the integers.
3. Sweep every bit pattern. A Huffman code is a prefix code, so **every pattern
   beginning with symbol S's codeword decodes to S** — the common prefix of all
   patterns yielding S therefore *is* S's codeword, and its length is the
   codeword length.

Two details matter for correctness. The basis is only well conditioned over the
**whole** 2048-sample window (cond = 1.0); over a single IMDCT half it is
time-domain aliased and degenerate past ~4 coefficients (cond 2.4e5 at 8), so
both output halves are fitted jointly. And a frame the decoder *dropped* still
yields a block of silence, which is indistinguishable from the genuine all-zero
symbol — so a dropped frame is detected by the output block count instead.

### How they were verified

The method was validated against the codebooks shravan already had right,
before being trusted for the broken ones:

| Control | Result |
|---------|--------|
| HCB7 (64 symbols) | **64/64 exact** |
| HCB5 (81 symbols) | **81/81 exact** |

and every derived table is independently checked for the three properties that
define a usable Huffman codebook — Kraft-McMillan sum exactly 1, no duplicate
`(length, code)` pair, and no codeword a prefix of another. Corroborating
detail: derived HCB1's codeword-length histogram (`{1:1, 5:8, 7:24, 9:24, 10:8,
11:16}`) is identical to the one already in the source, which is consistent with
its *lengths* having been right all along and only its *codes* corrupted.

`test_aac_codebook_integrity` in `src/main.cyr` asserts all three properties for
every codebook at test time, so a mistyped table cannot ship again.

## Other tables

- **Opus / CELT / SILK** — ported from libopus's float build and verified
  bit-exact against libopus; see the CHANGELOG entries for 2.6.0–2.6.7.
- **AAC scalefactor codebook, HCB5, HCB7–HCB10** — pre-existing; verified here
  as complete prefix codes (Kraft = 1) and, for HCB5 and HCB7, confirmed exactly
  by the measurement procedure above.
- **Ogg CRC32** — polynomial 0x04C11DB7, verified against known vectors in
  `test_ogg_crc32_known_vectors`.
