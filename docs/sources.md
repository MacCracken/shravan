# Source citations

Where shravan's non-obvious constants, tables and formulas come from, and how
each was verified. The rule is that a table is either derived from something
checkable or measured against a reference — never transcribed on trust.

## AAC spectral Huffman codebooks (`src/aac.cyr`)

**HCB1, HCB2, HCB3, HCB4, HCB6 and HCB11 were measured, not transcribed.**

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
| HCB11 | — | no table at all; cb 11 used a bespoke raw 9-bit index with a fixed 8-bit escape (v2.7.0) |
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

Every derived table is then independently checked for the three properties that
define a usable Huffman codebook — Kraft-McMillan sum exactly 1, no duplicate
`(length, code)` pair, and no codeword a prefix of another. Corroborating
detail: derived HCB1's codeword-length histogram (`{1:1, 5:8, 7:24, 9:24, 10:8,
11:16}`) is identical to the one already in the source, which is consistent with
its *lengths* having been right all along and only its *codes* corrupted.

The decisive check is end-to-end: with all eleven codebooks in place, shravan
decodes ffmpeg-encoded AAC at correlation **1.00000** with matching sample
counts, and ffmpeg decodes shravan's AAC (v2.7.0).

`test_aac_codebook_integrity` in `src/main.cyr` asserts all three properties for
every codebook at test time, so a mistyped table cannot ship again.

## AAC perceptual noise substitution scaling (`src/aac.cyr`)

A PNS band carries only an energy, and the decoder synthesises noise to match
it. The scale relating the coded noise energy to the spectral coefficients was
**measured**, not taken on trust: PNS-only frames were built to spec, decoded
with ffmpeg, and their spectral coefficients recovered by least squares against
the windowed MDCT basis. The ratio to `2^((nrg-100)/4)` came out **exactly 0.5**
and constant across global_gain 150/160/170 and band counts 4/8/16, so the
target RMS per coefficient is

    2^(nrg/4) / 2

where `nrg` accumulates as `global_gain - 90`, then the first noise band's raw
9-bit value less 256, then Huffman deltas.

Averaged over 60 frames — enough to remove the noise realisation, which
otherwise scatters a single-block measurement by tens of percent — shravan's
output energy matches ffmpeg's within **0.4%** at every gain and band count
tested. Sample-for-sample agreement is impossible by construction: each decoder
generates its own noise. `test_aac_stereo_tools` pins the band energy.

## AAC intensity stereo — encoder position (v2.7.2)

The decoder side was verified in 2.7.1 against ffmpeg with hand-built CPE frames
sweeping `is_position` (`istest2.py`): the right band is reconstructed as the
left scaled by `0.5^(is_pos/4)`, signed in phase for `INTENSITY_HCB` (15) and
out of phase for `INTENSITY_HCB2` (14), with an M/S mask bit on the same band
inverting the sense again.

The encoder inverts exactly that relation. For a band with left/right energies
`eL`, `eR`, the gain the decoder must apply is `sqrt(eR/eL)`, so

    is_position = round(-4 * log2(sqrt(eR / eL)))

sent as a delta on its own predictor, which starts at 0 and is separate from
both the scalefactor and the noise-energy chains.

No constant was taken on faith here: `stereoT`, a stereo signal that triggers
intensity but not PNS, is encoded by shravan and decoded by ffmpeg and shravan
independently. The two decodes correlate at **1.00000** with an RMS ratio of
0.9997 — bit-level agreement, which is achievable for intensity (unlike PNS,
where the noise realisation differs by construction).

The band qualifies at |correlation| > 0.85 above ~6 kHz. That threshold is a
tuning choice, not a spec value; it was swept over 0.80/0.85/0.90 on three
signals, trading about 0.3% of bitrate against 0.0008 of ch1 correlation across
that range.

## Opus decode tables (v2.7.3)

libopus 1.5.2 source (`downloads.xiph.org/releases/opus/opus-1.5.2.tar.gz`) was
used as the authority, and every table was **generated from that source by
script, never transcribed**. Each generator was validated on data shravan
already had before its new output was trusted:

- **`e_prob_model[0]` and `[1]`** (CELT coarse-energy probability model, 2.5 ms
  and 5 ms) — `celt/quant_bands.c`. The parser was first run against rows `[2][0]`
  and `[2][1]`, which shravan already carried, and both matched byte-for-byte.
- **`pred_coef` / `beta_coef`** for LM=0 and LM=1 — `celt/quant_bands.c:63`.
- **`tf_select_table[4][8]`** — `celt/celt.c:263`. shravan held a single row; the
  generator confirmed it equalled the LM=3 row exactly, then emitted all four.
  The LM=2 row differs from LM=3 at index 4 (2 vs 3), so the single-row table was
  also wrong for the 10 ms configs already shipping.
- **`silk_uniform6_iCDF`** — `silk/tables_other.c:92`, the 12 kHz pitch-lag
  low-bits codebook.
- **`silk_pitch_delta_iCDF`**, **`silk_LBRR_flags_2/3_iCDF`** —
  `silk/tables_pitch_lag.c:41`, `silk/tables_other.c`.

Behavioural rules were read from the source rather than recalled, and two
recollections were wrong until checked:

- The redundant frame at a mode transition is **5 ms (LM=1)**, not 2.5 ms;
  `F2_5 = 120` is the cross-fade length, not the frame length
  (`src/opus_decoder.c:600`).
- The SILK LP header orders flags **per channel** — every VAD flag for a channel,
  then that channel's LBRR flag — not VAD/LBRR interleaved per frame
  (`silk/dec_API.c:231`).

## Verification rig

`scratchpad/opusref/` builds against the installed libopus and generates, for
each of the 32 TOC configs in mono and stereo, a packet container plus
**libopus's own decode of exactly those packets**. shravan is judged by maximum
absolute sample error against that PCM, not by correlation: correlation above
0.999 was found to pass a stream containing a frame off by 271 LSB.

### Comparing against libopus: use the float API (v2.7.4)

`opus_decode()` runs **`opus_pcm_soft_clip()`** over its output — a limiter that
sits outside the codec and carries state across calls. Anywhere the signal
approaches full scale it pulls the output down (by up to 20% on the vectors
here), so a *correct* decoder measured against it looks wrong. Eight of the
twelve gaps reported in 2.7.3 were this and nothing else.

`opus_decode_float()` skips the limiter (`opus_decoder.c`: the two calls differ
only in their `soft_clip` argument). The reference rig therefore decodes with
`opus_decode_float` and writes raw f64; shravan writes raw f64; error is reported
in LSB of a 16-bit sample. Under that comparison all 64 config x channel vectors
agree within **0.016 LSB**, and the nine SILK-only vectors — integer arithmetic
end to end — are bit-identical.

The lesson generalises: when a reference implementation's convenience wrapper
does more than the format specifies, compare against the layer that implements
the format, not the wrapper.

## SILK encoder tables and constants (v2.8.0)

Ported against libopus 1.5.2 built from source. Every table was generated from
that source by script and every constant read out of libopus's own headers by a
compiled program — none was transcribed or derived by hand.

- **`silk_max_pulses_table`, `silk_pulses_per_block_BITS_Q5`,
  `silk_rate_levels_BITS_Q5`** — `silk/tables_pulses_per_block.c`. Encoder-only;
  the decoder already carried the matching iCDFs.
- **`silk_NLSF_CB2_BITS_NB_MB_Q5` / `_WB_Q5`** — `silk/tables_NLSF_CB_NB_MB.c`,
  `tables_NLSF_CB_WB.c`. **Both are 72 bytes.** A first attempt read them through
  the `silk_NLSF_CB_struct` pointer assuming `order × 10` (100 and 160), walked
  off the end of the arrays, and produced tables of trailing garbage that crashed
  the trellis. The parser now takes the length from the declaration and is
  cross-checked against a table shravan already has (`silk_NLSF_CB2_iCDF_NB_MB`).
- **Gain quantisation** — `OFFSET = 2090`, `SCALE_Q16 = 2251`,
  `INV_SCALE_Q16 = 1907825`, `N_LEVELS_QGAIN = 64`, `MIN/MAX_DELTA_GAIN_QUANT =
  -4/36`, from `silk/gain_quant.c` and `silk/define.h` via a compiled probe.
- **NLSF codebooks** — `quantStepSize_Q16` 11796 (NB/MB) and 9830 (WB),
  `invQuantStepSize_Q6` 356 and 427, read from `silk_NLSF_CB_NB_MB` /
  `silk_NLSF_CB_WB` directly.

### Integer semantics that the source does not show

Two behaviours are invisible unless the declared C types are checked, and both
change the result:

- `silk_NLSF_del_dec_quant` accumulates its R/D in `opus_int32` and **wraps** —
  the distortion term `SMULBB(diff,diff) * w_Q5` routinely exceeds 2^31. In f64
  Cyrius the wrap has to be applied deliberately (`silk_sx32`).
- `W_adj_Q5[]` in `silk_NLSF_encode` is an `opus_int16` array, so
  `silk_DIV32_varQ`'s 32-bit result is **truncated to 16 bits** before the
  trellis sees it.

The float→fixed boundaries in `silk/float/wrappers_FLP.c` use `silk_float2int` =
`lrintf` = round half to **even**, not the usual round-half-away-from-zero; that
matters for the analysis front end still to be ported.

### Oracle

`scratchpad/silkoracle` links libopus's static library and calls
`silk_lin2log`, `silk_gains_quant`, `silk_A2NLSF`, `silk_NLSF_VQ_weights_laroia`
and `silk_NLSF_encode` directly on chosen inputs. Each ported function is
compared against the reference implementation over randomised sweeps rather than
against a reading of the C.

## Other tables

- **Opus / CELT / SILK** — ported from libopus's float build and verified
  bit-exact against libopus; see the CHANGELOG entries for 2.6.0–2.6.7.
- **AAC scalefactor codebook, HCB5, HCB7–HCB10** — pre-existing; verified here
  as complete prefix codes (Kraft = 1) and, for HCB5 and HCB7, confirmed exactly
  by the measurement procedure above.
- **Ogg CRC32** — polynomial 0x04C11DB7, verified against known vectors in
  `test_ogg_crc32_known_vectors`.
