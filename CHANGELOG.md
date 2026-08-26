# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.8.0] - 2026-08-26

**SILK encoder, part 1: the bitstream writer and the quantisation chain — every piece verified
bit-exact against libopus 1.5.2.** This is the first half of the v2.8.0 roadmap item; the analysis
front end (LPC, noise shaping, excitation) is not here and shravan cannot yet encode SILK audio.
See "What this does not do" below. 11,706 assertions (was 11,665; +41).

### Added — the SILK bitstream writer, the exact inverse of the decoder

`silk_encode_indices_ec`, `silk_encode_pulses`, `silk_shell_encoder` and `silk_encode_signs`, plus
the three encoder-only ROM tables (`silk_max_pulses_table`, `silk_pulses_per_block_BITS_Q5`,
`silk_rate_levels_BITS_Q5`) generated from the libopus source, not transcribed.

Verified by transcoding, which is the only test that can isolate a writer bug: take a **real
libopus SILK payload**, decode its indices and pulses with shravan's decoder, feed those straight
back into the new writers, and require the bytes to come back identical. **174 of 174 frames are
byte-identical** across all twelve SILK-only mono configs — NB/MB/WB at 10/20/40/60 ms, voiced and
unvoiced, independent and conditional coding. (The comparison stops two bytes short of the range
coder's flush tail, where the original stream legitimately continues with more symbols.)

### Added — the quantisation chain

| function | verified against libopus |
|---|---|
| `silk_lin2log`, `silk_ror32` | 209/209 values across the full int32 range |
| `silk_gains_quant` | 300/300 random gain sets × previous index × coding mode |
| `silk_a2nlsf`, `silk_bwexpander_32` | 122/122 LPC filters (2 from libopus's own encoder, 120 random stable) |
| `silk_nlsf_vq_weights_laroia` | 150/150 |
| `silk_nlsf_encode` (stage-1 VQ + 4-state delayed-decision trellis + requantise) | 250/250 across NB/MB/WB, four R/D weights, 1–8 survivors, all signal types |

All of these are **fixed point even in libopus's float build**, so they are bit-exactness
requirements rather than approximations — the decoder's inverses are already in the tree and any
drift would produce a stream libopus cannot read.

Two traps this surfaced, both found by comparing rather than reading: libopus accumulates the
trellis R/D in `opus_int32` and lets it **wrap** (the distortion term routinely exceeds 2^31), and
`W_adj_Q5[]` is an `opus_int16` array, so the division result is **truncated to 16 bits** before it
reaches the trellis. Neither is visible in the source without checking the declared types.

### Fixed — `silk_encode_signs` read past the end of the pulse buffer

The sign coder walks whole 16-sample blocks, so for 10 ms at 12 kHz (120 samples = 7.5 blocks) it
read `pulses[120..127]`. libopus is safe only because `silk_encode_pulses` zero-fills that tail;
shravan now does the same. With the bump allocator this was a silent read of stale heap, not a
crash — it would have produced wrong signs and desynchronised the decoder on exactly one config.

### Tooling

`scratchpad/silkoracle` calls libopus's own `silk_gains_quant` / `silk_A2NLSF` / `silk_NLSF_encode`
/ `silk_lin2log` / `silk_NLSF_VQ_weights_laroia` directly, so each ported function is checked
against the reference implementation rather than against a reading of it. libopus 1.5.2 is built
from source in the scratchpad with tracing hooks in its SILK encoder.

### What this does not do

shravan **cannot encode SILK audio yet**. The analysis front end is absent: `silk_burg_modified_FLP`
(LPC), `silk_noise_shape_analysis_FLP`, `silk_NSQ` (which produces the excitation),
`silk_process_gains_FLP`, and the `silk_encode_frame_FLP` orchestration. The voiced path
(`silk_pitch_analysis_core_FLP`, `silk_find_LTP_FLP`, `silk_quant_LTP_gains`) is also absent,
though the bitstream writer already handles voiced frames — that is what the 40/60 ms conditional
-coding round trips exercise.

The roadmap's gate for this item — "a shravan SILK frame decodes in libopus" — is **not met**, and
deliberately so: it can be reached trivially by emitting a legal all-zero frame, which would decode
to silence and prove nothing. The remaining work is itemised as v2.8.1 in the roadmap.

Benchmarks: interleaved A/B against 2.7.4 shows FFT +0.3%, FLAC decode −0.5%, Opus CELT decode
+0.5% — all noise; nothing on the decode path changed.

## [2.7.4] - 2026-08-26

**All 32 Opus configs now decode within 0.02 LSB of libopus, mono and stereo — 64 of 64 vectors.**
2.7.3 measured 52 of 64 and listed twelve gaps. Eight of those turned out to be a flaw in the
*measurement*, not the decoder; the rest were two real bugs, both now fixed. 11,665 assertions
(was 11,660; +5).

### Fixed — CELT stereo side-inversion was skipped on N == 2 bands

`inv` tells the decoder the side channel was negated before coding. libopus applies that negation
to **every** stereo band, gating only `stereo_merge` on `N != 2` (end of `quant_band_stereo` in
`bands.c`). shravan had the negation nested inside the `N > 2` branch, so any band of width 1 —
the eight lowest, where `N == 2` — kept the wrong sign on its side channel whenever `inv` was
signalled.

Found by diffing an instrumented libopus against an instrumented shravan, stage by stage: the
range decoding, bit allocation, band energies and `ec_tell` all matched exactly, and the decoded
spectrum matched for channel 0 while channel 1 came back sign-flipped on bands 2 and 3 — with
both decoders agreeing that `inv = 1` for precisely those bands.

`test_celt_stereo_inv_n2` pins it with a real libopus 5 ms stereo frame whose bands 2 and 3 signal
`inv`; the expected per-channel energies come from libopus 1.5.2 decoding the same bytes. With the
negation back in the wrong scope the test reports channel 1 at 4.626 instead of 5.219, so it fails
loudly.

### Added — the redundant frame is now decoded in the HYBRID path too

2.7.3 decoded redundant frames only for SILK-only packets; the hybrid path read the header (keeping
the range coder aligned) but discarded the frame. Both hybrid paths now decode and mix it, honouring
libopus's asymmetric ordering: a CELT→SILK redundant frame is decoded **before** the main CELT
frame, a SILK→CELT one **after**, with a decoder reset in between. That ordering is not cosmetic —
the redundant decode shares the CELT state the following frames depend on.

### Fixed — the reference rig was comparing against libopus's limiter, not its decoder

Eight of 2.7.3's twelve "gaps" were not shravan bugs. `opus_decode()` runs `opus_pcm_soft_clip()`
over its output — a limiter that lives outside the codec and has memory across calls. Wherever the
signal approached full scale it pulled libopus's output down by up to 20%, and a correct decoder
looked wrong. `opus_decode_float()` skips it, and that is what the reference must use.

This is what the supposed "CELT LM=1 stereo bug" actually was: libopus's own internal deemphasis
output at the diverging samples turned out to equal **shravan's** values exactly, with the
difference introduced only afterwards. Configs 25, 29, 30 and 31 needed no change at all. The rig
now compares raw f64 samples from `opus_decode_float` against shravan's f64 output, with error
reported in LSB of a 16-bit sample.

### Result

| | 2.7.3 | 2.7.4 |
|---|---|---|
| vectors within ±2 LSB | 52 / 64 | **64 / 64** |
| worst error across all vectors | — | **0.016 LSB** |
| median per-vector worst error | — | 0.007 LSB |
| bit-identical vectors | — | 9 / 64 |

The nine bit-identical vectors are the SILK-only configs, where the arithmetic is integer
throughout. The rest differ only by float rounding, three orders of magnitude below one LSB.

Benchmarks: interleaved A/B against 2.7.3 shows FFT +0.2%, FLAC decode −0.3%, Opus CELT decode
+0.6% — all noise.

## [2.7.3] - 2026-08-26

**Opus decode completeness — and a packet-framing bug that was silently breaking shipped configs.**
Verified against a from-source libopus rig covering all 32 TOC configs in mono and stereo: shravan
now matches libopus within ±2 LSB on **52 of those 64 vectors, up from 22**. 11,660 assertions
(was 11,622; +38).

### Fixed — shravan had no Opus packet-framing layer at all

The TOC byte's low two bits are the frame-count code (RFC 6716 §3.2). shravan read that code and
then acted on it only in one place — a `code == 0` guard on the SILK branches. Everything else fell
through: a code-1/2/3 SILK packet produced **silence**, and a code-3 CELT packet was handed to the
decoder whole, frame-count byte and padding included.

This is not a corner case. libopus pads CBR SILK streams using code 3, so **30% of packets (384 of
1280) across the reference streams are code 3**. The effect on configs the project already claimed
as verified:

| config | before | after |
|---|---|---|
| 0 — SILK NB 10 ms | 1 of 20 frames decoded, rest silence | bit-exact |
| 1 — SILK NB 20 ms | 1 of 20 | bit-exact |
| 8 — SILK WB 10 ms | 7 of 20 | bit-exact |
| 9 — SILK WB 20 ms | 6 of 20 | bit-exact |

`_opus_split_frames` now implements codes 0/1/2/3 with the one- and two-byte length encoding, the
255-continuation padding scheme, and libopus's validity checks (≤48 frames, ≤120 ms, every length
inside the packet). The decode loop iterates frames within packets, and the output buffer is sized
by a pre-scan of real frame durations — it was `packets × 960`, which a 60 ms frame (2880 samples)
or a code-3 packet of 48 frames would have overrun. `test_opus_packet_framing` covers all four
codes, both length encodings, padding, and four hostile-input rejections.

### Added — SILK medium-band (12 kHz), TOC configs 4–7

Every bandwidth decision in the SILK decoder was a binary `if (fs_kHz == 8) … else … WB`, so 12 kHz
silently took the wideband branch. libopus puts 8 and 12 kHz *together* on `silk_NLSF_CB_NB_MB` with
LPC order 10 (`decoder_set_fs.c:74`); five sites now test `fs_kHz < 16`. The one genuinely missing
table was the pitch-lag low-bits iCDF — 8/12/16 kHz use `silk_uniform4/6/8` and shravan had no
`uniform6`, so an MB voiced frame spent 3 bits where libopus spends log2(6). Also: 10 ms at 12 kHz
is 120 samples = 7.5 shell blocks, the one config where libopus rounds the block count up.

### Added — SILK 40 ms and 60 ms frames, TOC configs 2/3/6/7/10/11

A SILK-mode Opus frame longer than 20 ms carries 2 or 3 sequential SILK frames in one range-coder
stream, and the LP header groups every frame's VAD flag by channel ahead of that channel's LBRR
flag — so the flags cannot be read per-frame. Frames after the first use **conditional coding**:
the first gain is delta-coded, the pitch lag is predicted from the previous frame when that frame
was voiced, and the LTP scaling index is omitted entirely. All three are now implemented, with the
`ec_prevSignalType` / `ec_prevLagIndex` predictor state carried across frames.

### Added — CELT 2.5 ms and 5 ms frames (LM=0/1), TOC configs 16/17/20/21/24/25/28/29

The CELT core was already largely LM-generic; what was missing were the tables and two gates:

- `e_prob_model` rows for LM=0 and LM=1, and `pred_coef`/`beta_coef` for both. Generated from the
  libopus source and **validated by re-deriving the two rows shravan already had** before use.
- `tf_select_table` held only the **LM=3 row**. That was also wrong for the LM=2 configs that ship
  today — the LM=2 row differs at index 4 — so a transient 10 ms CELT or hybrid frame was decoded
  with the wrong tf resolution. All four rows are now present and indexed by LM.
- For LM=0 there is no `tf_select` bit and no transient bit; reading them desynchronised the range
  decoder for every 2.5 ms frame.

Before this, configs 16–29's odd rows decoded to silence, and **config 25 did not terminate at all**
— a denial of service on a valid packet.

### Added — redundant CELT frames at mode transitions (RFC 6716 §4.5.1)

At a SILK↔CELT switch the encoder embeds an extra 5 ms CELT frame. shravan read the flag in the
hybrid path but never decoded it, and in SILK-only mode never read it — there the flag is *implicit*,
set whenever 17 bits remain. The visible cost was not a click: the CELT decoder entered each CELT run
with stale state, so the frames after every switch were wrong and converged only over ~5 frames.
`_opus_decode_redundancy` now reads the header, resets and primes the CELT state, decodes the 5 ms
frame from its own range decoder over the trailing bytes, and cross-fades with the CELT window. Every
SILK 40/60 ms mono config went from 9–10 bad frames to **bit-exact**.

### Fixed — SILK stereo hardcoded a 20 ms frame length

`silk_decode_stereo` computed `frame_length = 20 * fs_kHz` while the mono path read it from decoder
state, so every 10 ms stereo packet built mid/side buffers twice the real length. Configs 0, 8, 12
and 14 in stereo went from max errors of 14618/12310/14951/16208 LSB to bit-exact.

### Added — Opus decode benchmark

`opus_celt_decode_20ms` covers the CELT decode path this release reworked: **2.25 ms per 20 ms
frame**, about 8.8× realtime. An interleaved A/B against 2.7.2 puts the framing pre-scan and the
LM-indexed tf table at −0.6% (noise); FFT and FLAC are unchanged.

### Known remaining gaps (measured, not estimated)

12 of the 64 vectors are still outside ±2 LSB, and all of them are understood:

- **CELT LM=1 stereo at SWB/FB** (configs 25, 29) — 3–4 isolated frames of 20 diverge; the rest are
  exact. Not transient frames, and `dual_stereo` is decoded and threaded correctly, so the trigger is
  not yet identified. This also propagates into the redundant frame, which is itself a 5 ms stereo
  CELT decode — which is why six SILK/hybrid **stereo** vectors still show one bad frame at a
  transition.
- **Configs 30 and 31** — 2 and 4 individual samples exceed the tolerance (max 271 LSB) in otherwise
  exact streams. Pre-existing; exposed by moving from correlation to a max-error metric.
- **Hybrid redundancy** reads its header and keeps the range coder aligned, but still does not decode
  the redundant frame; only the SILK-only path does.

## [2.7.2] - 2026-08-25

**The encoder now uses the stereo and noise tools it could already decode.** 2.7.1 taught the
decoder to read per-band M/S, PNS and intensity stereo; the encoder still emitted none of them —
M/S was a time-domain downmix applied to every band, and codebooks 13/14/15 were never written.
All three are now real encoder decisions made per scalefactor band on the spectrum. **ffmpeg
decodes the result**: on a stereo signal exercising intensity but not PNS, ffmpeg's and shravan's
decodes of shravan's own stream correlate at **1.00000**. 11,622 assertions (was 11,615; +7).

### Added — per-band mid/side

M/S was applied to the *time-domain* input, which is equivalent to M/S on every band and is why
the encoder could only ever signal `ms_mask_present = 2` ("all bands"). Both channels are now
transformed to the spectrum first and each band chooses independently, on the classic criterion —
use M/S when the quieter of (mid, side) is quieter than the quieter of (L, R), i.e. exactly when
it makes one of the two coded channels cheaper. The header emits `ms_mask_present = 1` and a
49-bit mask.

This exposed a second bug: the side channel had been quantized with the **left channel's**
scalefactors. That was survivable while every band was M/S (mid and side have similar shape), but
with a per-band choice ch1 alternates between "side" and "right", whose levels differ. The right
channel's correlation had dropped 0.9930 → 0.8149 before it was given its own scalefactors.

### Added — perceptual noise substitution in the encoder

A band whose spectrum is noise-like — peak-to-RMS ratio below 2.5, above ~6 kHz — is now coded
with `NOISE_HCB` (13): its energy is sent and **no spectrum at all**. `scale_factor_data()` grew
the two extra predictors ISO requires, the first noise band's raw 9-bit value and the noise
delta chain, alongside the ordinary scalefactor chain.

Waveform correlation is the wrong way to judge this — each decoder generates its own noise, so
the samples *cannot* match by construction. Measured on the property PNS actually defines, band
energy, ffmpeg and shravan agree with median correlation **1.0000** and RMS within 1.5%.

### Added — intensity stereo in the encoder

Where a high band of the right channel is the left scaled by a gain, the band is coded with
`INTENSITY_HCB` (15) or `INTENSITY_HCB2` (14) — the right channel sends an `is_position` on its
own predictor and no spectrum. Bands qualify above ~6 kHz with |correlation| > 0.85; the position
is `-4·log2(sqrt(eR/eL))`, inverting the decoder's `0.5^(is_pos/4)`.

A band is intensity-coded *or* M/S-coded, never both — for an intensity band the mask bit means
"invert the sign", not "mid/side" — and intensity is suppressed where the left band is zero or
itself PNS, which would otherwise have the decoder copy the left channel's *noise* into the right.

### Changed — bit cost and quality

Measured by encoding with shravan and decoding with **both** ffmpeg and shravan:

| signal | 2.7.1 | 2.7.2 | change | ch0 / ch1 correlation |
|---|---|---|---|---|
| stereo tones | 68,770 B | 63,974 B | **−7.0%** | 1.0000 / 0.9926 |
| music-like stereo | 160,119 B | 157,127 B | **−1.9%** | 0.9923 / 0.9828 |
| stereo clicks | 50,352 B | 42,543 B | **−15.5%** | 0.9873 / 0.9696 |

PNS and intensity compete for the same high bands and PNS wins where both apply, since it drops
the spectrum for *both* channels rather than one — which is why intensity's share of the saving
is the smaller one.

### Added — AAC benchmarks

AAC encode is the heaviest path in the library and had no benchmark at all. `aac_encode_1sec_stereo`
and `aac_decode_1sec_stereo` now cover it. An interleaved A/B against 2.7.1 puts the three new
per-band decision passes at **+0.6% encode time** (558 ms → 561 ms per second of stereo audio;
decode −0.6%, within noise) — the decisions are essentially free.

Both figures are slower than realtime (≈0.56 s to encode and ≈0.88 s to decode one second of
stereo). That is pre-existing and unchanged by this release, but it is now on the record.

### Fixed — the benchmark history was recording truncated numbers

`scripts/bench-history.sh` stripped the fractional digits before scaling to nanoseconds, so
`1.390 ms/iter` was written to `bench-history.csv` as `1000000` — a 28% error in the file the
project treats as its proof, and enough to manufacture or hide a regression on its own. It now
scales the full decimal.

## [2.7.1] - 2026-08-25

**Stereo AAC now interoperates too.** 2.7.0 made the single-channel path work; this closes the
channel-pair element, perceptual noise substitution and intensity stereo. On ffmpeg-encoded stereo
files shravan now decodes **every frame** (was 30 of 36 and 18 of 23) with per-channel correlation
**1.00000 / 0.99843** and **0.98269 / 1.00000** — the sub-unity figures are PNS bands, which
*cannot* match a reference decoder by construction. 11,615 assertions (was 11,601; +14).

### Fixed — the channel-pair element was broken in four separate ways

`channel_pair_element()` re-implemented `ics_info()` inline rather than sharing the SCE path, and
had drifted from it:

- **Each channel's `global_gain` was read AFTER its section data.** ISO Table 4.44 puts it first,
  at the head of `individual_channel_stream()` — the same defect 2.7.0 fixed for SCE, still
  present here.
- **The per-channel `pulse_data_present` / `tns_data_present` / `gain_control_data_present` flags
  were never read at all**, so `spectral_data()` began three bits early for both channels. This is
  what produced `ERR_END_OF_STREAM` on real stereo files.
- **Short windows were refused outright** (`ERR_UNSUPPORTED_FMT`).
- **M/S was applied to the TIME-DOMAIN output.** M/S is a spectral operation; applying it after
  the IMDCT only happens to be equivalent when *every* band is M/S coded, and is simply wrong for
  the per-band mask that real encoders use (`ms_mask_present == 1` in all 30 and 22 CPE frames of
  the sample files).

The element is now parsed with the same `_aac_parse_ics_info` / `_aac_parse_sections_sf` helpers
as the SCE path, so the two cannot drift again, and the encoder emits the matching ISO order.

### Added — perceptual noise substitution (PNS)

A band coded with `NOISE_HCB` (13) carries **no spectrum at all**, only an energy — it decoded as
silence. The decoder now fills such bands with noise at the coded energy.

The scaling was measured, not assumed: recovering the spectral coefficients of PNS-only frames
built to spec gave a ratio to `2^((nrg-100)/4)` of **exactly 0.5**, constant across global_gain
150/160/170 and band counts 4/8/16 — so the target RMS per coefficient is `2^(nrg/4) / 2`, where
`nrg` accumulates as `global_gain - 90`, then the first noise band's raw 9-bit value less 256,
then Huffman deltas. Averaged over 60 frames to remove the noise realisation, shravan's output
energy matches ffmpeg's within **0.4%** across every gain and band count tested.

PNS output can never match a reference decoder *sample-for-sample* — each decoder generates its
own noise — so the property checked is spectral energy: on the sample files the per-block
band-energy correlation is median **1.00000** (min 0.978) and the overall rms ratio **0.999**.

### Added — intensity stereo

A right-channel band coded with `INTENSITY_HCB` (15) or `INTENSITY_HCB2` (14) is not transmitted:
it is derived from the left, scaled by `0.5^(is_position/4)`, with 14 out of phase and an M/S flag
on the same band inverting the sense again. ffmpeg's encoder does not emit intensity stereo, so it
was validated against ffmpeg with hand-built CPE frames instead: **all 16 combinations** — both
codebooks x both M/S flags x four positions including a negative one — agree.

### Fixed — scale_factor_data() has three predictors

Ordinary scalefactors, intensity positions and PNS noise energies each carry their own running
predictor, and **the first noise band is a raw 9-bit value**, not a Huffman code. Treating every
band as an ordinary delta desynchronised any stream using either tool — which is every stereo file
ffmpeg produces, since it uses PNS heavily (351 and 319 bands in the two samples).

### Fixed — other

- **`_aac_parse_ics_info` wrote `max_sfb` past the end of the SCE's `ics_info` buffer** (96 bytes
  allocated, field at offset +96). Now 128.
- **Short-block synthesis discarded its overlap tail and ignored window groups** in the
  dequantiser, so windows outside group 0 were scaled with group 0's scalefactors.
- Dequantisation is split out of synthesis (`_aac_dequant_long` / `_aac_dequant_short`,
  `_aac_synth_spec` / `_aac_synth_short_spec`) so the stereo tools and PNS can run where the
  format defines them — between dequantisation and the IMDCT.

### Measured

| | 2.7.0 | 2.7.1 |
|---|---|---|
| stereoT.aac frames | 30 of 36 | **36 of 36** |
| clickST.aac frames | 18 of 23 | **23 of 23** |
| stereoT L / R correlation | — | **1.00000 / 0.99843** |
| clickST L / R correlation | — | **0.98269 / 1.00000** |
| PNS band energy vs ffmpeg | silence | within **0.4%** |
| intensity stereo cases agreeing | 0 of 16 | **16 of 16** |
| mono tone.aac / transient.aac | 1.00000 | 1.00000 (unchanged) |

### Added — tests

M/S applied to the right band only and leaving its neighbours alone; intensity across both
codebooks, both M/S senses and three positions; PNS band energy against `2^(nrg/4)/2` with the
band boundaries respected.

### Not fixed

shravan's own **encoder** still emits neither intensity stereo nor PNS, and only all-bands M/S —
the decoder handles all three, the encoder does not produce them. Tracked in the roadmap.

### Performance

Median **-0.5%**, range -1.0% … -0.1% across the nine benchmarks — noise; none exercise AAC.

## [2.7.0] - 2026-08-25

**AAC is interoperable.** shravan decodes ffmpeg-encoded AAC at correlation **1.00000** with
matching sample counts, and ffmpeg decodes shravan's AAC. Before this release it managed 7 of 23
and 8 of 45 frames of the same files before bailing, and what it did produce was 65536x too loud.
11,601 assertions (was 11,572; +29).

Minor bump, not patch: the AAC bitstream shravan writes has changed. Streams produced by 2.6.x
are **not** readable by 2.7.0, and vice versa — 2.6.x was not writing AAC, it was writing a
private format that happened to use ADTS framing.

### Fixed — the two deviations that made AAC unreadable

- **`individual_channel_stream()` field order.** ISO/IEC 14496-3 Table 4.44 puts `global_gain`
  **first**, then `ics_info()`. shravan's encoder and decoder both put `ics_info()` first, so a
  spec-conforming frame was mis-parsed from its very first field. Corrected on both sides.
- **Codebook 11 was a bespoke code.** The encoder wrote, and the decoder read, a raw 9-bit
  `x*17+y` index with a fixed 8-bit escape. ISO Table 4.A.11 is a 289-entry Huffman code followed
  by a **variable-length** escape (leading 1s terminated by a 0 give N; the value is
  `2^(N+4) + next N+4 bits`). The fixed escape also capped magnitudes at 271 where AAC reaches
  8191. HCB11 was derived by the same measurement procedure as the 2.6.10 codebooks and verified
  complete (289/289 symbols, Kraft = 1).

### Fixed — everything else that stood between those and a correct decode

Each was found by decoding real files and measuring, not by reading the spec top to bottom:

- **Absolute output level was 2^16 too high.** ISO defines the synthesis IMDCT with a `2/N`
  normalisation that `fft_imdct` does not apply. shravan's own encoder omitted the reciprocal, so
  its round-trip cancelled the error out and nothing caught it — but a real stream decoded ~65536x
  hot and clipped hard (measured rms ratio 65535.55 against ffmpeg). Applied on synthesis and
  inverted on analysis, so the round-trip is unchanged.
- **`window_shape` was read and discarded** — shravan always synthesised with a sine window.
  ffmpeg emits **KBD** (Kaiser-Bessel Derived) for essentially every frame: 23 of 23 in a sample
  tone file. Both shapes are now built (alpha 4 long, alpha 6 short, via a series expansion of the
  modified Bessel function I0) and selected per frame. The IMDCT's first half overlaps the
  *previous* frame, so it uses that frame's shape while the second half uses the current one.
- **`max_sfb` is 4 bits in an EIGHT_SHORT_SEQUENCE**, not 6, and there is no
  `predictor_data_present` bit there. Reading 6 + 1 unconditionally desynchronised every short
  frame by two bits before the section data even began.
- **Window groups.** `section_data()` and `scale_factor_data()` both run *per window group*, and
  the spectral data is ordered group → band → window-within-group. Only the single-group case
  worked; real short frames use 3 and 4 groups. `sect_cb` and `scale_factors` are now group-major
  throughout.
- **LONG_START / LONG_STOP window forms.** Sequences 1 and 3 use asymmetric windows (a flat run,
  a 128-sample short segment, and a zero run) and were being synthesised as ONLY_LONG, so the
  overlap-add did not reconstruct across transitions.
- **Short-block synthesis was placing its sub-windows 448 samples early**, ignoring the window
  shape, and discarding the second half of the frame (`memset(overlap, 0, ...)`) — which corrupted
  the *following* frame as well. Rewritten over a full 2048-sample frame with correct placement,
  per-group scalefactors and a real overlap tail.
- **`scale_factor_data()` has three predictors**, not one: ordinary scalefactors, intensity
  positions, and PNS noise energies — and the first noise band carries a **raw 9-bit** value
  rather than a Huffman code. Treating every band alike desynchronised any stream using PNS or
  intensity stereo.
- **Fill and data-stream elements were fatal.** A `raw_data_block` may carry several elements, and
  ffmpeg's very first frame is `ID_FIL`-only. shravan read exactly one element and failed on
  anything that was not SCE/CPE/END. `ID_FIL` and `ID_DSE` are now skipped.
- **The cb 11 decode path dereferenced a null LUT.** The sorted/LUT tables were built lazily
  inside `_aac_decode_spectral_pair`, which that path never calls, so a frame using only cb 0 and
  cb 11 segfaulted. The builder is now shared (`_aac_ensure_huff_tables`).

### Measured

| | before (2.6.10) | after |
|---|---|---|
| frames decoded, tone.aac | 7 of 23 | **23 of 23** |
| frames decoded, transient.aac | 8 of 45 | **45 of 45** |
| correlation vs ffmpeg, tone | — | **1.00000** (rms 1402 vs 1402) |
| correlation vs ffmpeg, transient | — | **1.00000** |
| ffmpeg decoding shravan's AAC | fails | **0.9903** vs the original signal |

The one block below 0.999 in either file is block 0 of transient.aac, whose rms is ~1 — encoder
priming silence, where correlation is meaningless.

### Added

- `_aac_init_hcb11` (289 entries), `_aac_read_escape` / `_aac_write_escape`, KBD and sine window
  tables, `_aac_bessel_i0`, `_br_byte_align`, `_aac_skip_fil` / `_aac_skip_dse`.
- Tests: ISO escape round-trip across the full magnitude range including the unterminated-escape
  rejection; both window shapes checked against the Princen-Bradley condition
  (`w[n]² + w[N-1-n]² = 1`) and against each other; HCB11 added to the codebook integrity check,
  which now covers all eleven codebooks; short-window `max_sfb` width and multi-group parsing.

### Not fixed

Short-window **CPE** (stereo transients) is still refused with `ERR_UNSUPPORTED_FMT` rather than
mis-decoded — its per-channel grouped synthesis is not implemented. **PNS** and **intensity
stereo** are parsed correctly so the bitstream stays in sync, but a PNS band decodes as silence
and an intensity band is not folded. Both are tracked in the roadmap.

### Performance

Median **-0.9%**, range -1.6% … -0.3% across the nine benchmarks — none of which exercise AAC, so
this is run-to-run noise rather than a real improvement.

## [2.6.10] - 2026-08-25

**The four AAC/ID3 defects the 2.6.9 audit confirmed but did not fix.** The AAC spectral Huffman
codebooks were not transcribed from anywhere — they were **measured** against a reference decoder
and then independently verified, because four of the five needed were structurally impossible and
there was no way to tell a plausible table from a correct one by inspection. 11,572 assertions
(was 11,527; +45).

### Fixed — AAC spectral Huffman codebooks

Four codebooks were not valid prefix codes at all, and a fifth had no table. Checked straight out
of the source:

| Codebook | Kraft-McMillan sum | State before |
|----------|--------------------|--------------|
| HCB1 | 1 | lengths valid, but symbols 52 and 76 shared codeword `len 7 / 0x06e`, plus 8 prefix collisions |
| HCB2 | 15/16 | incomplete — 9 symbols unreachable |
| HCB3 | 4257/4096 | **greater than 1: impossible for any prefix code** — 32 symbols unreachable |
| HCB4 | 3971/4096 | incomplete — 28 symbols unreachable |
| HCB6 | — | no table at all; cb 6 decoded with **HCB5's** codes |

A codebook that is not a complete prefix code fails two ways: unreachable symbols can never be
emitted, and a codeword that is a prefix of another makes the decoder consume the wrong number of
bits and desynchronise the rest of the frame. The concrete case, now a regression test: HCB1's
duplicate meant quad `(0,1,1,0)` and quad `(1,1,0,0)` shared one codeword, so decoding either
returned `(1,1,0,0)` and the other could never be produced. All five codebooks are replaced, and
cb 6 is decoded — and encoded — with its own table rather than HCB5's.

**How the tables were obtained.** By measuring, not copying. A minimal ISO/IEC 14496-3 AAC-LC
frame is built in which one scalefactor band uses the codebook under test and the spectral data is
a chosen bit pattern; ffmpeg decodes it; the integer quantized coefficients are recovered from the
PCM by least squares against the windowed cosine basis and inverting the AAC dequantizer. Sweeping
every bit pattern gives the table for free: a Huffman code is a prefix code, so every pattern
beginning with symbol S's codeword decodes to S, and **the common prefix of all patterns yielding
S is S's codeword**. Two details carry the method — the basis is orthonormal only over the *whole*
2048-sample window (over one IMDCT half it is time-domain aliased and degenerate past ~4
coefficients, condition number 2.4e5 at 8), so both output halves are fitted jointly; and a frame
the decoder *dropped* still emits a block of silence, indistinguishable from the genuine all-zero
symbol, so dropped frames are detected by output block count instead. Full write-up in
`docs/sources.md`.

**How they were verified.** The method was validated against the codebooks shravan already had
right, before being trusted for the broken ones — **HCB7 64/64 exact, HCB5 81/81 exact** — and
each derived table is independently checked for Kraft-McMillan == 1, no duplicate `(len, code)`,
and no codeword a prefix of another. Corroboration: derived HCB1's length histogram
(`{1:1, 5:8, 7:24, 9:24, 10:8, 11:16}`) is *identical* to the one already in the source,
consistent with its lengths having been right and only its codes corrupted.

`test_aac_codebook_integrity` now asserts all three properties for all eleven codebooks at test
time, so a mistyped table cannot ship again.

### Fixed — AAC EIGHT_SHORT_SEQUENCE

`_aac_parse_ics` read `window_sequence` and threw it away, so every frame — including short-block
frames — was synthesised with the long-window path, and `_aac_synth_short` had **no callers at
all**. Any stream containing a transient decoded to garbage from its first short frame onward,
with the overlap buffer carrying the damage forward. Now wired end to end for the single-group SCE
case:

- `_aac_parse_ics` reports the window sequence and the window-group count through an out-parameter,
  decoding `scale_factor_grouping` rather than ignoring it.
- `_aac_decode_spectral` takes the band table from the window type — the 14-entry
  `_aac_swb_short` over a 128-coefficient window, not the 49-entry long table over 1024 — and
  decodes each band once per window. The per-band decode was factored into
  `_aac_decode_band_range` so long and short share one implementation.
- `_aac_parse_ics_ext` receives the real `is_short` instead of a hardcoded `0`; `tns_data()` field
  widths differ between long and short windows, so a short frame's TNS was previously read with
  long-window widths and desynchronised everything after it.
- Short frames dispatch to `_aac_synth_short`.

Multi-group short frames and short-window CPE need the grouped spectral layout, which is not
implemented; both now return `ERR_UNSUPPORTED_FMT` rather than silently decoding as long windows.

### Fixed — ID3v2.2 tags

v2.2 frame headers are **6 bytes** (3-character ID + 3-byte big-endian size, no flag bytes), not
the 10 of v2.3/v2.4. Parsed with the v2.3 geometry, the size was read from bytes straddling the
tail of the real size field and the head of the next frame's ID, so the walk either stopped
immediately or wandered off alignment — and the 3-character IDs never matched the 4-character
comparisons anyway, so nothing was ever populated. Now: version-aware header geometry, a v2.2
field assigner (`TT2`/`TP1`/`TAL`/`TRK`/`TYE`/`TCO`), and `COM` routed to the comment extractor
alongside v2.3/v2.4's `COMM`. `major_version` outside 2..4 is rejected instead of being funnelled
into the v2.3 branch.

### Added

- `docs/sources.md` — where shravan's non-obvious tables come from and how each was verified.
- Regression tests, each checked to fail without its fix: codebook integrity (all 11), the HCB1
  distinct-quad mis-decode, window-sequence reporting including multi-group refusal, and ID3v2.2
  frame parsing including the version-range rejection.

### Not fixed — AAC is still not interoperable, and this is why

Fixing the codebooks was necessary but is **not** sufficient, and the reason surfaced while
building spec-conformant frames to derive them. shravan's AAC bitstream is self-consistent — its
own encoder and decoder agree — but it does not follow ISO/IEC 14496-3:

- **`individual_channel_stream()` field order.** ISO Table 4.44 puts `global_gain` **first**, then
  `ics_info()`. shravan writes and reads `ics_info()` first. A spec-conforming frame is therefore
  mis-parsed from its very first field.
- **Codebook 11 is a bespoke code.** The encoder writes, and the decoder reads, a raw 9-bit
  `x*17+y` index with a fixed 8-bit escape — not the ISO HCB11 Huffman code with its
  variable-length escape. Real encoders use cb 11 heavily (352 of 3243 coded bands in a sample
  file).

Measured consequence: shravan decodes 7 of 23 and 8 of 45 frames of ffmpeg-encoded files before
bailing. Both deviations are now tracked in the roadmap as the actual prerequisites for AAC
interop. Because encoder and decoder share the tables, shravan's own round-trip fidelity is
unchanged by this release (correlation 0.9697 before and after on a signal that exercises cb 1-4;
the encoded stream does change, 9738 → 9744 bytes) — the codebook fixes are what a *conformant*
decoder will need, not a fix for the round-trip.

### Performance

Unchanged: median **+0.6%**, range +0.2% … +0.8% across the nine benchmarks — within run-to-run
noise, and none of them exercise AAC or tag parsing.

## [2.6.9] - 2026-08-25

**P(-1) scaffold-hardening sweep: 14 security fixes (SEC-019…SEC-032), a second fuzz harness
covering the formats that had none, and CI widened from 1039 to all 11,527 assertions.**

Every finding below was reproduced with a proof-of-concept before it was fixed and re-measured
after, so the numbers are observed rather than estimated. Cost of the whole sweep: **+0.7 %
median** across the nine benchmarks — the new bounds are per-chunk and per-frame, not per-sample.
11,527 assertions (was 11,495; +32 in 10 new hostile-input regression tests, each verified to
fail without its fix).

### Security — memory corruption and crashes

- **SEC-025 — NULL write, verified SIGSEGV.** `alloc()` signals failure by returning **0**, not a
  negative error. `_vec_new_cap()` published that 0 as a vec's data pointer *alongside a non-zero
  capacity*, so the first `vec_push` wrote through NULL. A 300 MiB WAV drove `count` to
  157,286,400, `alloc(1,258,291,200)` returned 0, and the decode died with **exit 139**. It now
  range-checks `n`, checks both allocations, and returns a packed error — which the six converters
  and both `wav_decode`/`aiff_decode` now propagate (`vec_len()` on a packed error was itself the
  fatal dereference). Same file now returns `ERR_DECODE`, exit 0.
- **SEC-024 — 248-byte heap overflow in FLAC.** `flac_decode_lpc`/`flac_decode_fixed` allocated
  `block_size * 8` bytes and then wrote `order` warm-up samples into it with no relation checked
  between the two. Block-size code 6 yields sizes down to **1** and LPC `sf_type` 63 yields order
  **32**, so a crafted subframe wrote 256 bytes into an 8-byte allocation, with attacker-chosen
  contents. `num_res = block_size - order` also went negative into `alloc()`. Now requires
  `0 <= order < block_size`, as the format does.
- **SEC-026 — 112-byte heap overflow in AAC.** In the CPE path `shared_max_sfb` is a 6-bit field
  (0…63) driving the per-band M/S mask loop, but `ms_mask` holds only `_AAC_NUM_SWB` (49) entries:
  63 stores of 8 bytes into a 392-byte allocation. Reachable from an **18-byte** ADTS file. The SCE
  path already had this clamp; the CPE path now matches it.

### Security — out-of-bounds reads

- **SEC-022 — WAV and AIFF read chunk bodies past the buffer.** Both chunk walks guard only
  `pos + 8 <= len`, proving the 8-byte chunk *header* is present — then read the body. WAV's `fmt `
  reads to `pos + 23` (16 bytes past); AIFF's `COMM` reads to `pos + 25` (18 bytes past, and with
  no declared-size check at all); AIFF's `SSND` offset field reads 4 past. Demonstrated on WAV: a
  44-byte file whose `fmt ` header ends exactly at byte 44 **decoded successfully**, reporting
  `rate=12345, channels=2, depth=24` read entirely from beyond the buffer. All three now require
  the body to be present.
- **SEC-028 — undefined shift counts from AIFF's 80-bit sample rate.** `exponent` is a 15-bit
  field, so `unbiased` spans −16383…+16384 and both branches of `extended_to_f64` computed shift
  counts far outside the legal 0…63 — exponent `0x403F` produced `32 - 33` = **−1**, a negative
  shift. Now bounded, returning 0 (which callers already reject) outside 0…62. AIFF `COMM` also
  validates channels, sample size and rate as ranges rather than only against zero.

### Security — allocation amplification

`alloc()` is a bump allocator that never frees, so amplification is unbounded growth, not
transient. Each fix converts an input-proportional multiplier into a fixed ceiling.

| Fix | Vector | Before | After |
|-----|--------|--------|-------|
| **SEC-023** | FLAC: 13-byte frame declaring `block_size` 65535 with a CONSTANT subframe | 26 KB → **3.0 GB** (~122,000x, unbounded) | hard ceiling **774 MB** at any input size |
| **SEC-020** | resample: kernel half-width scales with the inverse rate ratio, and `source_rate` comes from the file header | 64 samples, 48000→1 → **1.5 GB** | **9.6 MB** |
| **SEC-027** | AAC: 8-byte ADTS frame yields 1024 samples (~1024x); the cap was on the counting pass, not the allocating one | 560 KB → **2.15 GB** | ceiling **523 MB** |
| **SEC-029** | ALAC: `num_samples` was capped per element, the *number* of elements was not (~1 byte → 16384 samples) | ~63,000x | 2^25-sample ceiling |
| **SEC-019** | Ogg: 255 zero lacing values in 282 bytes minted 255 empty packets, each an alloc + `str_new` + vec slot | ~43 bytes heap per input byte | **2.9x** (page overhead only) |
| **SEC-030** | MP4: `stsz` sizes are u32 and `count` is bounded only by file length, so the byte total could wrap past 2^63 onto a *small* positive and undersize the `alloc()` the memcpy loop then overruns | wrap reachable in principle | capped at 256 MB |

FLAC additionally honours STREAMINFO `total_samples` when non-zero — the stream's own statement of
its length, so stopping there is correct decoding rather than only defence.

### Fixed — decode correctness

- **WAVE_FORMAT_EXTENSIBLE 24-in-32** (a roadmap item). `wValidBitsPerSample` was assigned over
  `wBitsPerSample`, but the former is the *meaningful* depth and the latter is the **container**
  that strides the sample data. Every "24-bit in a 32-bit container" file — the common
  professional layout — was read on a 3-byte grid out of 4-byte data and desynchronised after the
  first sample. Now strides by the container and reports the valid depth: a 24-in-32 file decodes
  to exactly 0.5 / −0.5 / 0.25 / 0.0.
- **SEC-031 — FLAC seek anchor** (a roadmap item). The SEEKTABLE byte offset is a 64-bit value
  taken straight from the file and never validated, so `flac_br_set_pos()` could anchor the
  bitreader before the audio or past the end of the buffer. Seek points that do not land inside
  the audio region are now ignored.
- **ID3v2 COMM frames were silently dropped — all of them.** A COMM body is
  `encoding(1) | language(3) | description(NUL) | text`, but it was passed to the plain-text
  extractor at `frame_data + 3`; that helper reads its first byte as the encoding, which is the
  *third language byte* (the `g` of `"eng"`), so the encoding check rejected every spec-conforming
  comment. Replaced with a COMM-aware extractor that skips the language code and the
  NUL-terminated description.
- **ID3v2 header flags were never read.** An extended header (flag `0x40`) was parsed as if it
  were the first frame, so the frame walk started at the wrong offset and lost the entire tag.
  Now skipped, honouring the v2.3/v2.4 difference (v2.3 excludes the size field, v2.4 includes it).
  Unsynchronised tags (`0x80`) shift every frame boundary; shravan does not de-unsynchronise yet,
  so they are reported `ERR_UNSUPPORTED_FMT` rather than returning confidently wrong metadata.
- **SEC-032 — chained Ogg streams were spliced together.** Page serial numbers were never
  consulted, so a physical stream carrying several logical ones — exactly what
  `cat a.opus b.opus` produces, legal per RFC 7845 §3 — had its pages reassembled into a single
  packet sequence. Reassembly now latches the serial from the first page (not from a BOS flag,
  which a truncated capture may lack and an attacker controls) and stops at a foreign one.
- **SEC-021 — `resample_mono` returned an empty vec as success** for a zero or negative rate,
  having none of the guards its sibling `resample()` carries: 64 samples in, 0 out, no error.
  Both now reject rates and channel counts `<= 0` (the old `== 0` check let negatives through)
  and return `ERR_INVALID_RATE`.

### Added — fuzzing and CI

- **`fuzz/fuzz_decode.cyr`** — a second harness that includes the *real* `src/shravan.cyr` rather
  than stubbing it. The existing `fuzz_codecs` note that "WAV/AIFF/ALAC require main.cyr library
  (deferred to library factoring)" had been obsolete since the library was factored out, leaving
  **WAV, AIFF, ALAC, AAC, MP4, Ogg/Opus, `codec_open` and Vorbis comments with no coverage at
  all**. Two input strategies per target: random bytes behind the format magic, and a *valid*
  file built by shravan's own encoders with N bytes corrupted — the latter is what reaches decode
  paths random data never survives to touch. Seeded and reproducible; a failing batch prints the
  seed that reproduces it.
- **`fuzz/run.sh` sweeps seeds across batches.** `alloc()` never frees, so one process cannot run
  unboundedly (~34K calls ≈ 230 MB RSS); the roadmap's ≥90K gate is reached with bounded memory
  instead. Current gate: **903,500 calls, 0 crashes** (810,000 `fuzz_codecs` + 93,500
  `fuzz_decode`), run after every fix above.
- **`scripts/check-write-lengths.py`** — CI guard for the v2.6.8 defect class. Every
  `syscall(1, fd, "…", N)` must declare its true UTF-8 byte length; short counts truncate output
  and long counts read past the literal into `.rodata`, emitting stray NULs that turn logs into
  binary files. 268 literals checked; `--fix` rewrites them. This is not hypothetical — the bug
  was reintroduced while *writing* the new fuzz harness and the guard caught it.
- **CI now gates on all three harnesses.** It built and ran only `src/main.cyr`, covering 1039 of
  11,527 assertions and leaving the Opus CELT encoder and the SILK golden vectors untested.
  `scripts/test-all.sh` runs under `set -o pipefail` — without it `tee` supplies the exit status
  and a failing suite would pass the step silently (verified) — plus a check that exactly three
  harnesses reported, so a suite that silently runs zero tests cannot pass.

### Performance

Neutral: median **+0.7 %**, range −0.3 % … +1.8 % across the nine benchmarks (same machine, same
session, 3 runs each, medians compared). Run-to-run variance on this machine is under 1 %, so the
sweep sits at the edge of noise; the bounds checks are per-chunk and per-frame, never per-sample.

### Not fixed — known and tracked

The audit confirmed further defects that are features rather than repairs, and they are **not**
addressed here: AAC `EIGHT_SHORT_SEQUENCE` is parsed then discarded (short-window frames decode
with the long-window path), spectral codebook 6 has no table of its own, and codebooks 1–4 are
malformed prefix codes; ID3v2.2 tags are parsed with the v2.3 frame layout; `alac_unmix_stereo`
uses a logical shift on a signed product. These belong with the existing 2.9.0 per-codec work.
`flac_br_read_unary`'s 32768 bound (SEC-008) was reported as rejecting spec-legal streams; it is
deliberately left in place — loosening a security bound on a theoretical argument, with no failing
real-world file, is the wrong direction for a hardening release.

## [2.6.8] - 2026-08-25

Maintenance release: toolchain pin **6.3.27 → 6.5.35**, sankoch **1.0.0 → 2.7.10**, stdlib
re-vendored. **The bump itself required no codec-source changes** — every module compiled
untouched and all 11,495 assertions pass unchanged (1039 / 138 / 10318). The source edits here
are unrelated to the bump: **12 `write(2)` calls across `src/` and `fuzz/` passed a byte count
that did not match their string literal** — 4 truncating output, 8 reading past the end of the
literal and emitting NUL bytes into the test transcript.

### Changed

- **Toolchain pin bumped 6.3.27 → 6.5.35** (`cyrius.cyml [package].cyrius`) — clears the pin
  drift the wrapper had been warning about (installed cycc was already 6.5.35). Re-vendored the
  declared `[deps].stdlib` subset with `cyrius lib sync` (31 modules, up from 30). Every codec
  module compiled unchanged: no stdlib symbol moved out from under shravan the way
  `math` → `ganita` (6.0) and `json` → `bayan` (6.1.25) did.
- **`lib/` grew from 31 vendored files to 38.** One (`thread_macos.cyr`) is new to the declared
  `[deps].stdlib` subset itself — the 6.5 line splits a macOS `thread` backend out. The other six
  are transitive leaves the 6.5 stdlib pulls in and 6.3.27 did not: `atomic`, `mmap`, `result`,
  `sync`, `sync_macos`, `sync_windows` (`thread` → `sync`/`atomic`, `alloc` → `mmap`,
  `io`/`bayan` → `result`). `cyrius.lock` therefore hashes 38 entries where it hashed 31, and
  **all 38 must be committed** for `cyrius deps --verify` to pass on a clean checkout — it
  reports `31 verified, 7 failed` otherwise. (Ordinary builds are unaffected: CI runs
  `cyrius lib sync` + `cyrius deps`, which re-vendor the missing seven before compiling.)
- **sankoch 1.0.0 → 2.7.10** (`[deps.sankoch]`, commit `89771bb`) — a major-version jump on a dep
  shravan declares but does not currently call; `cyrius.lock` refreshed to 38 deps locked, 1
  commit-pinned. Because `lib/` and `src/` are concatenated into one compilation unit, the jump
  was checked for symbol collisions rather than assumed safe: sankoch grew **88 → 486** top-level
  functions (+398), and a programmatic diff of its symbol set against all 912 `src/` functions
  found **zero newly introduced collisions**. The compiler agrees — still exactly one `duplicate
  fn` warning, the pre-existing `detect_format`.
- `dist/shravan.cyr` regenerated at v2.6.8. Its entire diff is four lines: the version stamp
  plus the three byte-count corrections that happen to live in `[lib]` modules (`src/flac.cyr`,
  `src/serde.cyr` ×2) — so consumers had been shipping those out-of-bounds writes too. That the
  bundle changed in *no other way* is the cleanest evidence that the toolchain and dependency
  bumps altered no generated code. `cyrius distlib` now also emits `dist/shravan.deps`, the
  15-leaf stdlib sidecar consumers feed to `cyrius deps`. `cyrius.cyml`'s `[lib]` comment has
  pointed consumers at that file all along, but it has not been in the tree since 957cec1
  ("cleaning up lib for source", 2026-07-01) deleted the copy added hours earlier by a1b82e3 —
  so for the whole 2.6.x line the manifest referenced a sidecar the repo did not ship. Note the
  file's meaning also changed while it was absent: the 2026-07-01 copy listed shravan's own codec
  modules (`flac`, `ogg`, `mp3`, …), whereas `cyrius distlib` now emits the *stdlib leaf*
  requirements (`string`, `fmt`, `alloc`, … `bayan`) — which is what `cyrius deps` actually
  consumes.

### Fixed

- **Mismatched `write(2)` byte counts — 12 sites across `src/` and `fuzz/`.** Every literal-plus-
  length write in the tree was audited programmatically (230 call sites); 12 disagreed with the
  true UTF-8 length of their literal. Two distinct defects, same root cause:
  - **Short counts (4) — output truncated.** The three test-harness banners
    (`src/main.cyr` 50 → 51, `src/main_encoder.cyr` 52 → 54, `src/main_silk.cyr` 57 → 59) had been
    sized as if the em-dash (U+2014) were one byte rather than three, so each lost its tail — most
    visibly the SILK suite, which printed `…golden vectors (Cyriusrunning tests...`, having dropped
    the closing `)\n`. Also `fuzz/fuzz_codecs.cyr` 24 → 25.
  - **Over-long counts (8) — out-of-bounds read.** `write(2)` was handed a length one byte beyond
    the literal, so it read the literal's NUL terminator out of `.rodata` and wrote it to stdout.
    This was not merely cosmetic: the eight stray NULs made the test transcript a *binary* file to
    `grep`, `diff`, and CI log scrapers, which silently skip or mis-handle it.
    `src/flac.cyr` 27 → 26, `src/main.cyr` 44 → 43, `src/opus_rfc_tests.cyr` 13 → 12 (×4, the
    `corrNx10000=` labels), `src/serde.cyr` 18 → 17 and 19 → 18.

  The suite output is now NUL-free, well-formed UTF-8 end to end: 8 NUL bytes before, 0 after,
  with all 11,495 assertions still passing. (It is not pure ASCII, and is not meant to be — the
  banners and section headers legitimately carry em-dashes, 30 high-bit bytes across the three
  binaries. The defect was the stray NULs, not the multi-byte characters.) `src/bench.cyr` (26)
  was already correct.
- Stale figures in `CLAUDE.md` and `README.md`: the suite was documented as 11,469 assertions
  with a 112-assertion encoder harness; the measured totals are 11,495 and 138. `src/main.cyr`'s
  own header still called itself the "843-assertion suite"; that harness runs 1039.
- **`SECURITY.md` claimed "zero external dependencies … no supply chain attack surface."** The
  first half is defensible (no C libraries, no third-party package ecosystem); the second is not,
  and this release makes that concrete by pulling 398 additional functions of vendored sankoch
  from an upstream git tag. Replaced with an accurate *Dependency Surface* section: the two
  first-party sources (Cyrius stdlib, sankoch), what pins each, and `cyrius deps --verify` /
  `cyrius.lock` as the control that actually enforces it. The section also no longer points
  auditors at "`src/main.cyr` and `lib/*.cyr`" — shravan-authored code is all of `src/*.cyr`,
  and `lib/` is vendored upstream.

### Performance

Toolchain bump is **performance-neutral** — median **−0.5%**, range −3.3% … +1.6% across the
nine benchmarks (same machine, same session, 3 runs each, 6.3.27 tree rebuilt from `HEAD` for
the A/B). Nothing regressed beyond run-to-run noise and nothing meaningfully improved.

Note for anyone reading `bench-history.csv`: comparing the new row against the previous
(2026-07-02) row suggests a 10–25% improvement. That is an artifact, not a codegen win — it is
cross-session machine drift, amplified by `bench-history.sh` truncating `ms`/`us` readings to
their integer part before recording (a 3.76 ms result and a 3.99 ms result both land as
`3000000`). The controlled same-session A/B above is the number to trust.

### Notes

- The two `lib/bayan.cyr` "assigning non-pointer to typed pointer" compile warnings are gone on
  6.5.35.
- Two warnings remain, both benign. The `detect_format` duplicate between `src/shravan.cyr` and
  `lib/sankoch.cyr` is pre-existing — shravan's definition is included last and wins, and it is
  already tracked in the roadmap for 2.9.0.
- **New: a `lib/` shadow notice, and a trap in the remedy it suggests.** Because the vendored
  sankoch (2.7.10, latest release) is newer than the copy the 6.5.35 toolchain bundles (2.7.8),
  every `cyrius build` / `cyrius distlib` prints a shadow warning ending in *"run `cyrius lib
  sync --full` to re-sync"*. **Do not follow that advice.** It was verified to do two harmful
  things: it overwrites `lib/sankoch.cyr` with the older 2.7.8 (silently downgrading the pinned
  dep, after which `cyrius deps --verify` reports `FAIL: lib/sankoch.cyr (hash mismatch)`), and
  it dumps the *entire* 108-file stdlib snapshot into `lib/`, discarding the declared-subset
  convention. The notice is cosmetic; leave it. To silence it legitimately, either set
  `CYRIUS_NO_WARN_SHADOW_LIB=1` or pin `[deps.sankoch].tag = "2.7.8"` to match the bundle — at
  the cost of not tracking latest.

## [2.6.7] - 2026-07-03

**CELT encode now covers stereo — mono AND stereo, closing the 2.6.x line.** `celt_encode(C=2)`
produces real joint (mid/side) and dual stereo that shravan's own decoder (bit-exact vs libopus)
reconstructs both channels of: correlated L/R → joint **0.998 / 0.997**, independent L/R → dual
**0.996 / 0.996**, low-bitrate (80 B/frame) → **0.981 / 0.977**. Every stereo-encode stage was ported
from libopus's float build and adversarially verified faithful. 11,495 assertions.

### Added — CELT stereo encode (`src/opus.cyr`)

- **`stereo_split`** (L/R → mid/side rotation by 1/√2) and **`intensity_stereo`** (fold Y into X for
  intensity-coded bands), wired into `compute_theta`'s encode branch: `qn≠1` applies
  `itheta==0 ? intensity_stereo : stereo_split`; `qn==1` (intensity band) does the `inv` side-inversion
  decision (`itheta>8192 && !disable_inv`), the Y-negation, the intensity fold, and codes the `inv` bit.
- **`quant_band_stereo`** N==2 side-sign encode (`sign = (x2[0]·y2[1] − x2[1]·y2[0]) < 0`) and the
  `MIN_STEREO_ENERGY` (1e-10) preamble that copies the louder channel over a ~silent one.
- **`stereo_analysis`** — the **dual-stereo decision** (L1 "entropy" of L/R vs M/S over the low bands)
  → joint mid/side vs L/R-separate coding.
- **Bitrate-driven `intensity`** via `hysteresis_decision` over `intensity_thresholds` /
  `intensity_histeresis` — without it, `intensity=0` folded every band and destroyed the distinct L/R
  shapes; with it, both channels reconstruct at ~0.99.
- Band energies handed to `quant_all_bands` (`_celt_enc_bandE` → `band_ctx+88`) for the stereo transforms.

### Fixed

- **Range-coder desync on stereo intensity coding.** `celt_compute_allocation`'s encode path omitted the
  `*intensity = min(*intensity, codedBands)` clamp (`rate.c:400`) before `ec_enc_uint`; when
  `intensity > codedBands` the encoder coded an out-of-range symbol, desyncing the range coder so the
  decoder mis-read intensity and every subsequent band. Found by adversarial verification. (Reachable
  with a persistent intensity + VBR; the single-frame tests keep `intensity ≤ codedBands`.)
- **`intensity_histeresis` table** was transcribed from a different libopus version; corrected to
  byte-exact `{1×7, 2×7, 3, 3, 4, 5, 6, 8, 8}` (a programmatic table diff caught it).

### Not yet (honest status)

- **SILK** encode and **hybrid** encode remain (the encoder counterparts of 2.6.1–2.6.3) — tracked for
  2.8.0. CELT stereo encode is fullband today; other bandwidths ride the `end` parameter.

## [2.6.6] - 2026-07-03

**CELT encoder quality: the encoder now makes real, signal-driven decisions.** Where 2.6.5
shipped a valid-but-plain CELT frame (fixed `tf_res=0`, no dynalloc, default spread/trim,
single-pass energy), 2.6.6 fills in the full encoder-quality decision surface — every knob
ported from libopus's **float build** (where `SHR32`/`PSHR32`/`SROUND16` collapse to no-ops, so
the port replicates what the macros *evaluate to*) and adversarially verified faithful. Net
effect on the in-suite round-trips: frame spectrum **0.968→0.991**, transient **0.898→0.943**,
PCM **0.997→0.999**; libopus still decodes shravan's frames sample-identically. 11,484 assertions.

### Added — CELT encoder-quality knobs (`src/opus.cyr`)

- **Adaptive spread decision** (`celt_spreading_decision`, `bands.c`) — the signal-driven tapset
  replaces the fixed default (interop verified 1.0).
- **2-pass coarse-energy intra/inter race** (`quant_coarse_energy`) — encodes both the intra and
  inter energy predictors into scratch range-coder state and keeps the cheaper (e.g. inter 40 vs
  intra 55 bits) instead of always coding intra.
- **Transient encode** (short blocks) — `isTransient=1` frames MDCT into M short blocks with the
  anti-collapse reservation, `celt_tf_encode`, and the short-block `quant_all_bands` path.
- **Transient detection** (`celt_transient_analysis`, `celt_encoder.c:267`) — the encoder's own
  short-block decision from the pre-emphasized signal (HP filter → forward/backward masking →
  integer mask metric `>200`), `inv_table[128]` byte-exact vs libopus. `celt_encode` auto-detects
  `isTransient` via a `−1` sentinel (steady tone → long, sudden onset → short).
  `test_celt_transient_analysis_rfc`.
- **tf_analysis** (`celt_tf_analysis`, `celt_encoder.c:663`) — real per-band `tf_res` via an
  L1-metric Haar-level search + a dual Viterbi (`tf_select` + backtrace), smoothed by `lambda =
  max(80, 20480/bytes+2)`. Replaces the all-zeros `tf_res`; round-trips through `celt_tf_encode`/
  `celt_tf_decode` (encoder remap == decoder remap, identical bits). `test_celt_tf_analysis_rfc`.
- **dynalloc boosts** (`celt_dynalloc_analysis`, `celt_encoder.c:1049`) — a masking follower
  (forward/backward passes + median-of-5 filter, noise-floor bounded) → per-band bit `offsets`
  (output in accumulated 1/64-bit units = `boost_count·quanta`, so the R9 boost loop reproduces
  them exactly) + the real `importance[]` that now weights tf_analysis. Mono; stereo keeps the
  flat fallback. `test_celt_dynalloc_analysis_rfc` (flat spectrum → no boosts; a lone peak → boost
  384 + importance 208, matching a hand trace).

### Changed

- **Test suite split into three harness binaries** so no single build overflows the ~3.15 MB
  Cyrius code buffer: `src/main.cyr` → `build/shravan` (core codecs + Opus decode + API, 1039),
  `src/main_encoder.cyr` → `build/shravan-encoder` (CELT encoder, 127), `src/main_silk.cyr` →
  `build/shravan-silk` (SILK decode golden vectors, 10318). `scripts/test-all.sh` builds + runs
  all three; the shared assertion/correlation helpers moved to `src/opus_test_helpers.cyr`. Total
  assertions preserved.

### Verified

- Each stage was ported from a from-source libopus reference and checked by fan-out
  adversarial-verification workflows (transient / tf / dynalloc), each stage confirmed faithful to
  the float build. The one flagged finding (R9 dynalloc-encode `tb2` budget) was **refuted against
  source** — libopus's gate is `tell+(dll<<BITRES) < total_bits − total_boost`
  (`celt_encoder.c:2373`), which shravan's `tb2 -= quanta` folds into one running budget. Every
  decode / SILK test stayed green throughout.

### Not yet (honest status)

- **CELT encode is mono only.** Stereo encode (`stereo_split`/`intensity_stereo`,
  `quant_band_stereo` N==2 sign), **SILK** encode, and **hybrid** encode remain (2.6.7+).

## [2.6.5] - 2026-07-03

**Opus encode is real (CELT): libopus decodes shravan's output.** shravan now *encodes* a
real RFC-6716 **CELT** frame, and a **libopus** decoder decodes it **sample-identical** to
shravan's own verified decoder (**correlation 1.000000**, rms/scale exactly matched). The
whole encode chain — forward MDCT → band energies → coarse/fine energy encode → bit
allocation → PVQ shape search+encode → range coder — was built from scratch as the exact
inverse of the verified decode path and proven by encode→decode round-trips at every stage.
An end-to-end frame (spectrum → 160-byte packet → decode) reconstructs the input spectrum at
**correlation 0.968** (the residual is quantization, as expected). This is the encoder
counterpart of v2.6.0's "Opus is real" (CELT-only mono first). 11460 assertions.

### Added — CELT encoder (`src/opus.cyr`), each stage inverse of the verified decoder

- **Range encoder** `ec_enc_*` (`entenc.c` port): `ec_encode`/`_bin`/`_bit_logp`/`_icdf`/
  `_uint`/`_bits`/`_done` + the carry-propagation (`ec_enc_carry_out`) with uint32-wrap
  masking. Byte-identical to libopus for a fixed symbol sequence, and round-trips through
  `ec_dec` (`test_ec_enc_rfc`).
- **Forward MDCT** `clt_mdct_forward` (`mdct.c`): window/fold → pre-rotate (scale 1/N4) →
  the same direct DFT as the backward transform → post-rotate. Coeff-exact vs libopus
  (`test_celt_mdct_forward_rfc`).
- **Band energy + shape**: `compute_band_energies` (L2 norm) + `normalise_bands` (unit shape);
  energies match libopus, each normalized band has unit norm (`test_celt_band_energy_rfc`).
- **Energy encode**: `ec_laplace_encode` (byte-identical, `test_ec_laplace_encode_rfc`) +
  `quant_coarse_energy_impl`/`quant_coarse_energy` (single-pass) + `quant_fine_energy` +
  `quant_energy_finalise`. Full energy encode→decode round-trip is **bit-exact**
  (`test_celt_energy_encode_rfc`).
- **PVQ encode**: `op_pvq_search` (projection + greedy pulse search), `icwrs`,
  `celt_encode_pulses`, `exp_rotation_enc`, `alg_quant`. Resynth is bit-exact vs the decoder
  (`test_celt_pvq_encode_rfc`).
- **Band coding** made bidirectional: `stereo_itheta` + `compute_theta` encode branch
  (`test_celt_compute_theta_rfc`), the `quant_partition`/`quant_band`/`quant_band_n1` encode
  paths (`test_celt_quant_band_rfc`), `celt_tf_encode` (`test_celt_tf_encode_rfc`), and the
  `celt_compute_allocation` encode path (skip/intensity/dual-stereo). **Every decode test
  stayed bit-exact (11455→11460) throughout the shared-code refactor.**
- **Frame assembler** `celt_encode_frame_ec`: the full non-transient mono prefix (silence /
  postfilter / transient flags → coarse energy → tf → spread → dynalloc → alloc_trim →
  allocation → fine energy) + `quant_all_bands` encode + finalise. Milestone:
  `test_celt_encode_frame_rfc` (0.968 spectrum round-trip; libopus interop verified
  externally, correlation 1.0).
- A **fan-out adversarial-verification workflow** (7 agents) over the whole encoder found a
  real latent `quant_band_n1` N==1-band encode gap (fixed) that the round-trip tests missed.

### Not yet (honest status)

- **CELT encode only, mono, 20 ms, non-transient**, with simple encoder decisions
  (`tf_res=0`, no dynalloc boosts, `spread`/`trim` defaults, single-pass intra/inter). The
  bitstream is valid and libopus-decodable; the **encoder-quality** knobs (transient +
  `tf_analysis`, dynalloc, the 2-pass coarse-energy race, spread decision) are refinements.
- **Stereo** encode (`stereo_split`/`intensity_stereo`, `quant_band_stereo` N==2 sign),
  **SILK** encode, and **hybrid** encode remain — the encoder counterparts of 2.6.1–2.6.3.

## [2.6.4] - 2026-07-03

**Opus 10 ms frames are real.** shravan now decodes actual libopus-encoded **10 ms**
`.opus` across every mode: SILK-only (NB config 0 / WB config 8), hybrid (SWB config 12
/ FB config 14), and CELT-only 10 ms (config 18/22/26/30) — mono **and** stereo.
Verified **hybrid-10 ms frame-0 correlation 0.9999 vs a libopus golden** (sample-exact
after a real bit-exact fix, below). Combined with 2.6.0–2.6.3, shravan decodes **10 ms
and 20 ms** Opus, all bandwidths, mono + stereo. 11357 assertions.

### Fixed — `special_hybrid_folding` (RFC 6716 / `bands.c:1579`), a real bit-exact bug

- `quant_all_bands` was **missing `special_hybrid_folding`**: in hybrid mode the second
  coded CELT band has no lower band to fold from, so libopus duplicates enough of the
  first band's fold data (`norm[n1 .. n2] ← norm[2·n1−n2 ..]`) before decoding it. shravan
  left that region zero, so a **folded** second band (0 pulses) decoded wrong. It stayed
  hidden until 10 ms hybrid, where that band *is* folded — the CELT high band's
  time-domain output was scrambled (frame-0 correlation **0.9662**). With the fold added
  it is **0.9999** (bit-exact vs the from-source libopus reference, traced stage-by-stage:
  energy/allocation/PVQ/`X` all matched; only the fold source differed). Benefits **all**
  hybrid decodes — it is a shared-path correctness fix, not 10 ms-specific.

### Added — Opus 10 ms decode (`src/opus.cyr`)

- **SILK 10 ms** (`nb_subfr = 2`): `opus_decode_from_packets` routes config 0 (NB) / 8 (WB)
  mono and their stereo variants through `silk_decode_pcm` / `silk_decode_stereo` inited
  with `nb_subfr = 2`; the state cache keys on `(fs, nb_subfr)` so a stream can mix frame
  sizes. In-suite: `test_opus_silk10_rfc` (config 8, correlation 1.0000).
- **Hybrid 10 ms**: config 12 (SWB, CELT end = 19) / 14 (FB, end = 21) decode the SILK low
  band (WB 16 kHz, `nb_subfr = 2`) + CELT high band (`start = 17`, **`LM = 2`**, accumulate)
  over the shared range coder, mono + stereo. In-suite: `test_opus_hybrid10_rfc`
  (config 14, correlation 0.9999).
- **CELT-only 10 ms** (config 18/22/26/30, `LM = 2`) was landed in 2.6.3's LM refactor and
  is exercised by `test_opus_celt10_rfc` (correlation 1.0000).

### Changed — module split to hold the distlib cap (`src/opus_legacy.cyr`)

- Extracted the **legacy 2.5.x bespoke CELT encoder + Opus encoder framework** (~100 KB,
  91 functions, entirely off the RFC-6716 decode path) from `src/opus.cyr` into a new
  **`src/opus_legacy.cyr`**. It is included by `src/main.cyr` (its PVQ/energy/transient/
  encoder tests still run) but **excluded from the `[lib]` bundle** — the distlib consumer
  ships only the RFC decoder. This drops `opus.cyr` from **261 KB → 162 KB**, well under
  the 256 KB per-module distlib read cap (which had begun truncating the bundle), leaving
  room for the 10 ms dispatch and future decode work.

### Not yet (honest status)

- Non-10/20 ms CELT-only sizes (2.5 ms / 5 ms, `LM = 0/1`). SILK MB, and 40/60 ms mono.
  **Encoder** (real `ec_enc` + RFC-6716 CELT encode so libopus decodes shravan's output)
  is **2.6.5**. Redundant-frame audio. Claim today: "real Opus 10 ms + 20 ms decode, mono
  + stereo, all bandwidths, verified vs libopus."

## [2.6.3] - 2026-07-02

**Opus stereo is real.** shravan now decodes actual libopus-encoded **stereo** `.opus` —
SILK mid/side stereo (config 1/9) and stereo hybrid (config 13/15), 20 ms. Verified
**per-channel correlation 1.0000 vs a libopus golden** (SILK stereo) and **0.9999**
(stereo hybrid); a real stereo SILK file decodes at L 0.999958 / R 0.999975 vs ffmpeg.
Combined with 2.6.0–2.6.2, **every 20 ms Opus config now decodes** (mono + stereo).
11354 assertions.

### Added — Opus SILK stereo decode (`src/silk.cyr`, `src/opus.cyr`)

- **`silk_stereo_decode_pred`** — the mid/side prediction weights (joint iCDF + two
  uniform stages + sub-step interpolation, `pred[0] -= pred[1]`) and
  **`silk_stereo_decode_mid_only`**. Bit-exact vs libopus (`test_opus_silk_stereo_pred_rfc`).
- **`silk_stereo_ms_to_lr`** — the 3-tap predictor interpolation over `STEREO_INTERP_LEN_MS·fs`
  samples + mid/side → L/R reconstruction, with the 2-sample `sMid`/`sSide` history.
- **`silk_decode_stereo`** — the full stereo pipeline: packet header (both channels'
  VAD/LBRR) → predictors + mid-only → decode mid + side as mono frames → MS→LR →
  resample each channel to 48 kHz.
- **VAD refactor** — `silk_decode_indices_ec` / `silk_decode_frame_ec` take a pre-read
  VAD flag (stereo reads both channels' flags up front; mono/hybrid read inline). Mono
  and hybrid stay **bit-exact** (zero behavior change).
- **Dispatch**: `opus_decode_from_packets` routes stereo SILK (config 1/9, stereo bit)
  through `silk_decode_stereo`, and **stereo hybrid** (config 13/15, stereo bit) through
  `silk_decode_stereo` (low band) + CELT stereo (`C=2, start=17`, accumulate) over the
  shared range coder. In-suite guards: `test_opus_silk_stereo_rfc`, `test_opus_hybrid_stereo_rfc`.

### Not yet (honest status)

- **10 ms** frames (config 12/14 + CELT LM=2) and non-20 ms CELT-only sizes remain
  (**2.6.4**). SILK MB / 10/40/60 ms mono. **Encoder** is 2.6.5. Redundant-frame audio.
  Claim: "real Opus 20 ms decode, mono + stereo, all bandwidths, verified vs libopus."

## [2.6.2] - 2026-07-02

**Opus hybrid is real.** shravan now decodes actual libopus-encoded **hybrid** `.opus`
files (SILK low band + CELT high band over one shared range coder — TOC config 13 SWB /
15 FB, 20 ms mono) end-to-end to 48 kHz, verified against a libopus golden at
**correlation 0.9999**, per-frame ≥ 0.99993 across a real file. The same refactor
generalizes the CELT decoder to arbitrary band ranges, so the remaining **CELT-only
20 ms configs** (19 NB, 23 WB, 27 SWB — previously silence) now decode too, alongside the
2.6.0 config-31 (FB). 11345 assertions.

### Added — Opus hybrid decode (`src/opus.cyr`)

- **CELT decoder now takes `(start, end, accum)` + a shared `ec`** — the entire decode
  path (coarse/fine/finalise energy, `tf_decode`, allocation, `quant_all_bands`,
  `denormalise_bands`, synthesis, `anti_collapse`, `deemphasis`, and both
  `celt_decode_frame_ec`/`celt_decode_frame_prefix_ec`) decodes bands `[start,end)` from a
  caller-owned range decoder, optionally accumulating onto the output. Config-31
  (`start=0, end=21, accum=0`) stays **bit-exact**. The postfilter is gated on `start==0`.
- **SILK decoder now takes a shared `ec`** — `silk_decode_indices_ec` /
  `silk_decode_frame_ec` / `silk_decode_pcm_ec`, with backward-compatible payload wrappers.
- **Hybrid orchestrator** in `opus_decode_from_packets`: one `ec` over the packet → SILK
  decodes the low band (WB 16 kHz) from the front → CELT decodes the high band
  (`start=17`, end 19 SWB / 21 FB) from the same `ec` and **accumulates** onto the
  SILK output (÷32768 normalized). Redundancy flag is read to keep the `ec` in sync.
- **CELT-only 20 ms configs 19/23/27** wired via the new `end_band` parameter
  (NB→13, WB→17, SWB→19). Config-23 (WB) verified bit-exact vs libopus
  (`test_opus_celtwb_rfc`).
- In-suite guards: `test_opus_hybrid_rfc` (config-15 frame 0, corr 0.9999 vs golden),
  `test_opus_celtwb_rfc` (config-23 frame 0, corr 0.9999).

### Fixed

- **`quant_all_bands` fold for `start>0`** — the PVQ lowband/fold logic omitted
  `norm_offset = M·eBands[start]` (harmless for config-31 where it is 0). In hybrid the
  *uncoded* high bands folded from the un-decoded low region (zeroed), producing wrong
  collapse masks (`0` instead of `254`) → anti-collapse filled them with noise → wrong
  high band. Threaded `norm_offset` through the `lowband_offset` update and the
  `effective_lowband` clamp, matching libopus `bands.c`. Found via bit-exact stage dumps.

### Not yet (honest status)

- Hybrid **10 ms** (config 12/14, needs CELT LM=2) and **stereo** hybrid (needs SILK MS
  stereo) are not decoded. CELT-only non-20 ms frame sizes (2.5/5/10 ms) and SILK MB /
  10/40/60 ms / stereo remain. **Encoder** is 2.6.3. Claim: "real CELT + SILK + hybrid
  20 ms mono `.opus` decode, verified vs libopus"; not "Opus complete".

## [2.6.1] - 2026-07-02

**SILK is real.** shravan now decodes actual libopus-encoded SILK-mode `.opus` files
(RFC 6716, voice: TOC config 1 = NB / config 9 = WB, 20 ms mono) end-to-end to 48 kHz
PCM — **correlation 0.999994 vs ffmpeg's libopus decoder** on a real file (the residual
is sub-sample delay/pre-skip phase, not decode error). Every internal stage is proven
**bit-exact against libopus** via a from-source reference decoder (`opusdec_dump`,
built with per-stage `SILK_DUMP` instrumentation) — 11343 assertions total. Built
foundation-up as an exact fixed-point port (int16/int32 widths, arithmetic shifts and
saturation emulated in Cyrius i64).

### Added — real RFC-6716 SILK decoder (`src/silk.cyr`)

- **LP-layer header + `silk_decode_indices`** — VAD/LBRR flags, signal type, gains
  (with `silk_gains_dequant` LastGainIndex state), NLSF stage-1/stage-2 indices,
  interpolation factor, and the voiced block (pitch lag, contour, LTP period + taps,
  LTP scale, seed). Bit-exact for NB (order 10) + WB (order 16) (`test_silk_indices_rfc`).
- **NLSF → LPC** — `silk_nlsf_decode` (predictive residual dequant + inverse-weighted
  CB1 + `silk_nlsf_stabilize`) → `pNLSF_Q15`, then `silk_nlsf2a` (LSF cosine-table
  `find_poly` P/Q convolution → `silk_lpc_fit` int16 fitting → the QA=24
  `silk_lpc_inv_pred_gain` stability/bandwidth-expansion loop) → `PredCoef_Q12`.
  Bit-exact vs the reference `PredCoef_Q12` dump, NB + WB.
- **Pitch + LTP** — `silk_decode_pitch` (lag + contour-codebook offset, clamped),
  `silk_decode_ltp` (per-subframe taps from the PER codebook, Q7→Q14), `silk_ltp_scale`.
  Bit-exact on voiced frames.
- **Excitation** — `silk_decode_pulses`: rate level, per-block sum-of-pulses with the
  LSB-shift escape, the 15-node `silk_shell_decoder` split tree over shell-code tables
  0–3, and `silk_decode_signs`. Every sample of 4 excitation vectors bit-exact.
- **Synthesis** — `silk_decode_core`: excitation reconstruction (`silk_RAND` LCG,
  quant-offset, sign), LTP long-term prediction, re-whitening via
  `silk_lpc_analysis_filter`, LPC short-term synthesis (order 10/16), gain scaling.
  Internal-rate output bit-exact for unvoiced (frame 0) and voiced (frame 1) frames.
- **Resampler** — `silk_resampler` (internal 8/16 kHz → 48 kHz): 2× all-pass HQ
  upsampler (`silk_resampler_up2_hq`) + 8-tap fractional FIR interpolation over the
  `frac_FIR_12` ROM, with the 1 ms input-delay buffer. Output bit-exact vs libopus.
- **Orchestration** — `silk_decoder_init` / `silk_decode_frame` / `silk_decode_pcm`:
  the full stateful per-frame pipeline (indices → pulses → NLSF interpolation for
  `NInterp<4` → pitch/LTP → core → resample) with cross-frame state (outBuf, sLPC,
  prev_gain, LastGainIndex, prevNLSF, sMid delay). End-to-end bit-exact bits → 48 kHz.

### Changed

- `opus_decode_from_packets` (`src/opus.cyr`) now routes SILK-only 20 ms mono packets
  (config 1 NB / 9 WB, code 0) through the real SILK decoder → 48 kHz, alongside the
  2.6.0 CELT config-31 path. Other configs (MB SILK, 10/40/60 ms, hybrid, stereo SILK)
  still emit timeline-aligned silence.

### Fixed

- `ogg_decode` now lazy-inits the CRC32 table, so consumers calling `decode_file()` on
  an Ogg/Opus file without the full init sequence no longer dereference a null table.
- `silk_isort_i16` (the NLSF-stabilizer insertion-sort fallback) mistranslated the C
  loop's stop condition — it clobbered the loop index on the `a[j] <= value` exit and
  wrote every element to index 0, corrupting the sort. Caught by an adversarial audit of
  the port against the libopus C reference; the fallback fires only when the 20-loop
  iterative stabilizer fails to converge on extreme NLSF vectors, so it is off the
  common path but reachable from arbitrary valid bitstreams. Now sorts correctly, with a
  direct unit test (`test_silk_isort_rfc`).

### Not yet (honest status)

- SILK **MB** (12 kHz internal, config 4–7), **10/40/60 ms** frame sizes, and **stereo**
  (MS) SILK are not decoded (silence). **Hybrid** (2.6.2) and the **encoder** (2.6.3)
  remain. Claim: "real CELT + SILK 20 ms mono `.opus` decode"; not "Opus complete".

## [2.6.0] - 2026-07-02

**Opus is real.** shravan now decodes actual libopus-encoded `.opus` files
(RFC 6716, CELT-only fullband — TOC config 31, mono + stereo) end-to-end to PCM,
**correlation 1.000000 / SNR ~131 dB vs ffmpeg's libopus decoder**, matching sample
values to ~1e-7. This is the interop milestone the 2.5.x arc kept deferring: the
bespoke, non-RFC LZMA-style stream is retired from the file-decode dispatch and
replaced by a real port of the libopus CELT decoder, built foundation-up with every
stage proven **bit-exact against libopus**. 1033 assertions.

### Added — real RFC-6716 CELT decoder (`src/opus.cyr`)

- **`ec_dec` range decoder** — exact port of libopus `entdec.c`/`entcode.c`
  (`ec_decode`/`ec_dec_update`/`ec_dec_bit_logp`/`ec_dec_icdf`/`ec_dec_uint`/
  `ec_dec_bits`/`ec_tell`/`ec_tell_frac`). Bit-exact vs a libopus-`ec_enc`-produced
  vector (`test_ec_dec_rfc_vectors`).
- **Energy** — `ec_laplace_decode` + coarse/fine/finalise (`quant_bands.c` float path).
  Laplace bit-exact vs libopus (`test_ec_laplace_rfc_vectors`); `e_prob_model`,
  `pred_coef`/`beta`, `eMeans` extracted verbatim.
- **Bit allocation + front-of-frame** — `tf_decode`, `clt_compute_allocation` /
  `interp_bits2pulses` / `bits2pulses` / `init_caps`, and the full silence → postfilter
  → transient → intra → coarse → tf → spread → dynalloc → trim → alloc → fine sequence.
  Bit-exact across 4 real frames incl. band-skipping (`test_celt_allocation_rfc`).
- **PVQ / CWRS / bands** — `cwrsi`/`decode_pulses` (Fischer U-table, replaces the bespoke
  index scheme), `alg_unquant`, `exp_rotation`, `quant_all_bands` / `quant_partition` /
  `quant_band_stereo`, `compute_theta`/`compute_qn`, `bitexact_cos`/`log2tan`,
  intensity+dual stereo, `denormalise_bands`. Verified via CWRS vectors
  (`test_celt_cwrs_rfc`) and full-recursion range-sync + collapse-mask match
  (`test_celt_bands_rfc`, mono+stereo, transient+steady).
- **Synthesis** — inverse MDCT (`clt_mdct_backward`, direct DFT, CELT 120-sample overlap
  window, per-channel overlap-add via `decode_mem`), `anti_collapse`, pitch `comb_filter`
  post-filter, and de-emphasis. IMDCT impulse-exact (`test_celt_imdct_unit`).
- **Wired**: `opus_decode_from_packets` drives the real decoder (config-31 detection,
  per-packet channels, pre-skip trim, granule cap). Full-file decode of
  `real_mono.opus`/`real_stereo.opus` verified 1.0 vs ffmpeg;
  `test_celt_pcm_rfc` + `test_celt_pcm_stereo_rfc` assert PCM correlation in-suite.

### Changed

- The bespoke Opus encoder/decoder path (2.5.9) is now **legacy** — retained for the
  encoder-conformance work (2.6.3) but no longer on the `.opus` file-decode path. The
  three bespoke encode→decode roundtrip tests (which asserted that superseded contract)
  were removed; the real decoder is verified against actual libopus streams instead.

### Scope (honest)

- Done: **CELT-only, fullband (config 31)** decode — what libopus emits for music, so
  real-world music `.opus` files decode today.
- Not yet: **SILK** (voice, 2.6.1), **hybrid** (2.6.2), and the **encoder** so libopus
  can decode shravan's output (2.6.3). See `docs/development/roadmap.md`.

## [2.5.12] - 2026-07-02

**MP3 decode is now comprehensive** — MPEG-2/2.5 (LSF) Layer III, plus **Layer II
and Layer I**, join the MPEG-1 Layer III decoder from 2.5.11. Every common
configuration is verified **sample-exact against minimp3**. 932 assertions
(was 909, +23).

### Added — MPEG-2/2.5 (LSF) Layer III (`src/mp3.cyr`)

- **LSF side info** (one granule, 8-bit `main_data_begin`, no `scfsi`, 9-bit
  `scalefac_compress`) + **LSF scalefactor decode** (ported from mpg123's
  `III_get_scale_factors_2`: the 4 partition slens derive from the sfc range, the
  band counts from the `stab` table). Unified the scalefactor-band tables to one
  8-row set covering all 9 sample-rate configs (minimp3 `sr_idx` order).
- Verified sample-exact vs minimp3: **MPEG-2 22050 Hz mono + joint stereo (M/S)
  1.00000**, **MPEG-2.5 11025 Hz mono 1.00000**.
- **Stereo stage rewritten** to minimp3's model — M/S and intensity now run in
  bitstream order *before* reorder, with the correct `max_band` boundary and the
  MPEG-1 `g_pan` / LSF `ldexp` intensity panning. (The previous pdmp3-derived
  intensity was subtly wrong.)

### Added — Layer II and Layer I (`src/mp3.cyr`)

- Subband-PCM decode ported from minimp3 (CC0): subband **bit allocation** (per
  layer/bitrate/mode alloc tables), **scalefactors**, grouped/ungrouped sample
  **(de)quantization**, feeding the shared polyphase synthesis filterbank
  (refactored into `_mp3_synth_ch`, reused by all three layers). Layer I and II
  share the code via the group size (1 vs 3).
- Verified sample-exact vs minimp3: **Layer II mono, joint stereo, and the
  low-rate allocation table all 1.00000**; **Layer I 1.00000** on a constructed
  stream. `mp3_decode` dispatches Layer I/II/III automatically.

### Tests

- Committed value-level tests (embedded fixtures): Layer II mono + stereo
  (two-tone correlation, 0.999), Layer I (correlation vs an embedded minimp3
  reference frame, 0.999), MPEG-2/2.5 Layer III mono + stereo (0.999).

### Security

- The Layer I/II path parses untrusted input; fuzzed **20,000 malformed frames,
  0 crashes** (the subband structure is bounded within the frame).

### Known limitation

- **MPEG-2.5 at 8000 Hz, low bitrate, short blocks** decodes imperfectly
  (~0.7 correlation) — a narrow bug isolated to that combination (the sfb tables
  and reservoir assembly are both verified byte-exact; the reservoir and short
  blocks each work independently, and 8000 Hz decodes correctly at higher
  bitrate). All other sample rates and block types are sample-exact. Tracked.

## [2.5.11] - 2026-07-02

**MP3 (MPEG-1 Layer III) decodes to real PCM audio** — `mp3_decode` returned an
empty sample vector (metadata only) since 2.0.0; it now produces actual samples,
verified sample-for-sample against a reference decoder (minimp3 waveform
correlation **0.99999**, lag 0) and, in the committed suite, against the source
signal (**0.999** best-lag correlation). 904 assertions (was 875, +29).

### Added — full MPEG-1 Layer III decode chain (`src/mp3.cyr`)

Ported from pdmp3 (Krister Lagerström, public domain) + ISO/IEC 11172-3. The
whole pipeline, all f64-internal:

- **Side info + bit reservoir** — `main_data_begin` back-pointer assembles main
  data from up to two previous frames (real streams carry Huffman data across
  frame boundaries); side info parsed over shravan's `bitreader`.
- **Scalefactors** — long/short/mixed blocks, `scfsi` copy-from-granule-0.
- **Huffman** — the 34 code tables as a compact binary-tree array (2804 nodes,
  generated from pdmp3.c), big-values regions (per-region `table_select`),
  count1 quadruples, `linbits`, sign bits.
- **Requantization** — `x^(4/3)` power law, global gain, per-band scalefactors,
  `subblock_gain`, `preflag`/pretab; long and short blocks.
- **Reorder** (short-block frequency-line interleave), **stereo** (MS +
  intensity, long/short), **antialias** (8-tap butterflies from `cs`/`ca`).
- **Hybrid IMDCT** — long (36) / short (12) with the four block-type windows and
  block-type switching (start/stop/short/mixed), overlap-add across granules;
  frequency inversion.
- **Polyphase synthesis filterbank** — 32→64 matrixing + the 512-tap synthesis
  window (`g_synth_dtbl`) + the 1024-sample V FIFO → 576 PCM samples/granule.

`mp3_decode` now decodes MPEG-1 Layer III (mono + stereo/joint) to PCM and falls
back to the previous metadata-only path for Layer I/II and MPEG-2/2.5 (LSF), so
nothing regresses. Large tables are CSV string literals generated from pdmp3.c;
trig tables (IMDCT/synthesis cosines, windows, `cs`/`ca`, intensity ratios) are
derived at init from formulas.

### Tests

- **Value-level correlation test** (the roadmap's acceptance rule): an embedded
  real MPEG-1-L3 mono fixture (44100 Hz, 32 kbps, 11 frames, base64 in-source +
  a tiny decoder) decodes to 12672 samples and correlates **0.999** with its
  two-tone source — the fixture exercises the bit reservoir and long/start/stop/
  short block switching. Plus a table-derivation self-test (Huffman node count,
  synthesis window, `cs`/`ca`, sfb indices, windows).

### Security

- The MPEG-1-L3 path parses untrusted input, so it was fuzzed (**50,000 malformed
  frames across two runs — plain and joint-stereo/intensity — 0 crashes**) and
  hardened. `big_values*2` (a 9-bit field, up to 1022) is clamped to the 576-line
  spectrum buffer and the Huffman region boundary indices (`r0+1`, `r0+r1+2`) are
  clamped to the 23-entry scalefactor-band table. An adversarial review against
  the reference then found three more boundary over-reads, all fixed: the top
  long-block region (band 21) and short-block region (band 12) now use
  scalefactor/pretab 0 (ISO-correct) instead of reading one past their tables,
  and intensity-stereo gates on `is_pos < 7` (0..6 valid) instead of `!= 7`, so a
  crafted 4-bit scalefactor (8..15) can't index the 6-entry `is_ratios` table out
  of bounds. A committed hostile-input test drives garbage/truncated/stereo
  frames through `mp3_decode` and asserts it returns safely.

### Deferred (MP3 tails — refinements, not blockers)

- MPEG-2/2.5 (LSF) Layer III (half-rate side info, one granule, intensity-stereo
  scalefactor compression) and Layer I/II *decode* (still metadata-only).
- A fast (FFT-based) IMDCT/synthesis (the direct cosine transforms are O(N²) —
  correct but a perf follow-up, matching the AAC/Opus note).

## [2.5.10] - 2026-07-02

**AAC encoder produces real audio** — `aac_encode` → `aac_decode` now round-trips
faithfully (mono **0.999**, stereo **0.999 / 0.999** waveform correlation), where before
it decoded to silence. Every AAC correctness/safety bug the audit found is closed. 870
assertions (was 859, +11).

### Fixed — the transform (root cause of BOTH silent encoders)

- **`fft_mdct`/`fft_imdct` were never a matched analysis/synthesis pair** (`fft_imdct`
  is a valid IMDCT — it decodes real AAC — but `fft_mdct`'s convention did not invert it,
  so encode→decode cancelled to silence). Replaced them with a verified TDAC-invertible
  direct pair (shared cosine basis, standard AAC/Vorbis convention, unity gain). The old
  `test_fft_mdct_roundtrip` only checked "output is nonzero"; it now asserts real
  windowed 50%-overlap reconstruction to `<1e-6`. Also fixed the IMDCT scale (`2/M`) so
  reconstruction is unity-gain (this made Opus decode unity-gain too). O(N²); a fast
  FFT-based pair preserving this convention is a perf follow-up.

### Fixed — AAC encoder/decoder

- **Encoder 50%-overlap framing** (window 2048, hop 1024; stop zeroing the 2nd MDCT
  half) so the decoder's overlap-add reconstructs (TDAC) — mono and stereo mid/side.
- **Invertible quantizer**: `q = round((|x|/step)^0.75)` (was `round(|x|^0.75/step)`,
  which did not invert the decoder's `q^(4/3)·step`), with **peak-based scale factors**
  so tonal peaks survive (the rms-based target ~10 collapsed single-bin tones to zero).
- **Section coding groups by codebook** (was grouping by has-data but writing the first
  band's codebook for the whole run, so a run of data bands with different codebooks was
  mislabeled and decoded as garbage — masked for a single-band tone, fatal for any
  multi-band signal). This one bug was capping mono at 0.975 and breaking stereo entirely.
- **CPE stereo bitstream**: dropped the stray 11-bit per-channel side ICS block the
  decoder never read (11-bit misalignment); the side sections now use the escape codebook
  matching the escape-coded side spectral (was copying the mid codebooks); mid/side
  overlap framing + invertible side quantizer.
- **Scale-factor DPCM predictor** tracks the transmitted (clamped) value, not the raw
  scale factor (mono + CPE) — the confirmed desync bug.
- **Decoder robustness**: reserved section codebooks 12–15 now advance the band (were an
  unguarded infinite loop = DoS on malformed input); removed the band double-increment
  for codebooks 1–4/9/10 (skipped the following band). Reject-not-hang test added.

### Tests

- Value-level round-trip tests (correlation vs. input, not sample counts): AAC mono,
  AAC stereo, and the transient/stereo/mono Opus paths. `aac_decode` of a real AAC AU
  (via MP4) still works — the new transform is standard-conformant.

### Deferred (AAC completions — refinements, not blockers)

- **TNS temporarily disabled**: the analysis-FIR/synthesis-IIR pair is exact only without
  quantization; engaged through the quantizer it amplified quant noise (0.98→0.68).
  Proper prediction-gain-gated noise shaping (+ short-window, stereo/CPE) is a follow-up.
- **Real HCB6 tables** (codebook 6 currently reuses HCB5; shravan's own encoder never
  selects it, so it only affects some third-party files) and the **psychoacoustic ATH
  floor + tonality-adjusted SMR** completion.
- Optional rate/distortion scale-factor loop (replaces the peak heuristic).

## [2.5.9] - 2026-07-02

**Opus/CELT decodes to real PCM audio** — the headline gap the 2.5.0–2.5.8 scaffolding
was built for. `opus_decode_from_packets` now returns actual samples (it returned an
empty vector before), verified by waveform correlation, not sample counts. 859
assertions (was 843, +16).

### Added

- **Magnitude synthesis** (`_opus_denorm_frame`, `src/opus.cyr`) — reconstructs MDCT
  coefficients from the decoded band energies (`2^(q/2)·band_size` sum-of-squares,
  split by width across PVQ sub-blocks) × the unit-L2 shapes. This is the missing step
  that turns the CELT "direction" into real spectral magnitude.
- **Verified TDAC MDCT/IMDCT pair** (`_opus_mdct`/`_opus_imdct`) — a self-matched
  direct cosine-basis pair (measured OLA reconstruction 0.999). Root cause found: the
  shared `fft_mdct`/`fft_imdct` in `fft.cyr` are **not a matched analysis/synthesis
  pair** — `fft_imdct` is a valid IMDCT (it decodes real AAC), but `fft_mdct`'s
  convention does not invert it, so encode→decode cancels to silence. This also
  explains a large part of the silent AAC encoder. (Fixing `fft.cyr`'s pair for the
  AAC encoder + optimizing the O(N²) direct transform is tracked for 2.5.x.)
- **Full decode path wired** — `opus_decode_from_packets` iterates the audio packets,
  decodes each CELT frame (mono via `_opus_celt_decode_mono_time`, stereo via
  `_opus_celt_decode_stereo_time` with `_opus_stereo_decouple`), IMDCTs, applies the
  synthesis sine window, and **50%-overlap-adds** (Princen-Bradley: sin²+cos²=1) into
  PCM. The encoder now frames with 50% overlap (hop = half a frame) to match.
- **Real end-to-end round-trip tests** — encode a tone/sweep, decode via the public
  `ogg_decode` path, assert best-lag normalized cross-correlation with the input:
  mono **0.997**, stereo **L 0.996 / R 0.999**, transient-containing **0.998**.

### Changed

- **Band-energy DPCM range widened 128→256 symbols** (±127 half-log2-power steps).
  The old ±63 clamp saturated the jump from a silent band to a loud tone band, so
  loud and quiet content collapsed to the same reconstructed energy (absolute levels
  were lost — steady tones only passed because correlation is scale-invariant). A
  step-amplitude signal now round-trips at 0.998 (was 0.28).
- **Rate-control complexity from the real MDCT spectrum** (`_opus_frame_complexity`) —
  fraction of spectral energy in the upper band, replacing the time-domain
  first-difference proxy (completes a 2.5.8 deferral; transients raise it naturally,
  so allocation is transient-aware). Two-pass VBR remains future work.
- **Encoder no longer truncates overflow packets** — an over-budget frame is emitted
  at its true length (Ogg carries it; each frame's range coder is independent) instead
  of cutting the trailing bytes and corrupting the whole packet.

### Deferred

- True CELT **overlapping short-window** transient coding (pre-echo reduction):
  transient frames are detected + flagged but decoded via the long MDCT for now (they
  produce correct audio, just without short-block time resolution).
- **RFC 6716 conformance** — the stream is still a bespoke shravan-internal format
  (LZMA-style range coder, uniform energy coding); it does not interoperate with
  libopus. Tracked as a dedicated 2.5.x release.

## [2.5.8] - 2026-07-01

CELT/Opus rate control + VBR — completes the CELT rate model. 843 assertions
(was 830, +13).

### Added

- **Bit-reservoir rate control + VBR** (`src/opus.cyr`):
  - `_opus_rate_new` / `_opus_rate_target` — a reservoir rate controller. CBR emits
    the nominal size every frame; **constrained VBR** lets a complex frame spend saved
    bits from the reservoir but keeps the average at/under the target bitrate (the
    reservoir stays ≥ 0, so total ≤ frames × nominal); **unconstrained VBR** sizes each
    frame purely by complexity.
  - `_opus_frame_complexity` — a cheap per-frame complexity in [0,100] (high-frequency
    first-difference energy / total energy): tones are simple, noise/transients complex.
  - `opus_encode` refactored onto `_opus_encode_impl` (CBR — behavior unchanged) and a
    new `opus_encode_vbr` (constrained VBR) that varies packet size with content.
- **Tests** (`+13` assertions): CBR target = nominal for all complexities; VBR
  monotonic in complexity (c=50 → nominal); CVBR per-frame bounded + average ≤ target
  over mixed frames; complexity(noise) > complexity(tone) + silence = 0; `opus_encode_vbr`
  yields a valid Ogg stream.

## [2.5.7] - 2026-07-01

MP4/M4A container demux — enables real `.m4a` playback. 830 assertions
(was 809, +21).

### Added

- **MP4/M4A container module** (`src/mp4.cyr`) — ISO Base Media File Format demux:
  - Box-tree parser (`mp4_find`) + navigation `moov → trak → mdia → minf → stbl`,
    picking the track whose `hdlr` handler is `soun`.
  - `mp4_audio_info` reads channels + sample rate from the `stsd` `mp4a` entry;
    `mp4_build_sample_table` computes each AAC access unit's file offset + size from
    `stsz` + `stco`/`co64` + `stsc` (sample-to-chunk).
  - `mp4_demux` returns the track info + per-sample table; `mp4_decode` wraps each AU
    in an ADTS header (`aac_build_adts_header`) and hands the stream to `aac_decode`.
  - New `FMT_MP4` format + `detect_format` recognition (`ftyp` box) + decode dispatch,
    so `codec_open`/`decode_file` handle `.m4a` transparently.
- **Tests**: a constructed minimal MP4 demuxes to the correct track info + sample
  table (`detect_format → FMT_MP4`), an MP4 wrapping a real AAC access unit decodes
  to 1024 PCM samples, and hostile inputs are rejected safely.

### Security

- The demuxer parses untrusted input, so it was adversarially reviewed and hardened:
  every attacker-controlled count/offset/size is bounds-checked against its box and
  the buffer (`stsz`/`stco`/`co64`/`stsc` counts, per-AU offset+size before `memcpy`),
  every FullBox header read (`hdlr`/`stsz`/`stco`/`stsc`) is guarded against an
  empty/truncated box, the largesize (`size==1`) read is guarded, and all
  allocations are null-checked. Malformed files return an error instead of reading
  out of bounds, over-allocating, or crashing.
  - Deferred: adding `mp4_decode` to the fuzz harness (needs the AAC chain vendored
    into the standalone harness).

## [2.5.6] - 2026-07-01

CELT stereo coupling: full two-channel bitstream (replaces the mono downmix).
809 assertions (was 799, +10).

### Added

- **Two-channel stereo CELT frame** (`src/opus.cyr`) — building on the 2.5.5
  coupling primitives, the encoder now codes stereo instead of downmixing:
  - `_opus_encode_celt_frame` gained a stereo branch: deinterleave L/R → window +
    MDCT each → `_opus_stereo_couple` → mid/side + per-band `ms_flags`; range-codes
    `isTransient` + 21 `ms_flags` bits + mid/side band energies + mid/side PVQ shapes,
    with the shape budget split evenly between the two channels. Stereo TOC bit set.
  - `_opus_decode_celt_stereo_frame` — mirror decode: reads `ms_flags` + both channels'
    energies and shapes; the caller applies `_opus_stereo_decouple` for L/R.
  - Shape encode/decode refactored to an explicit-bit-budget core
    (`_opus_encode_spectral_shape_bits` / `_opus_decode_spectral_shape_bits`) so the
    mid/side split is data-independent (both sides derive it from the packet size).
  - The mono path is unchanged.
- **Tests** (`+10` assertions): full stereo frame roundtrip (`ms_flags` + mid/side
  energies + mid/side shapes all bit-exact, couple∘decouple recovers L/R MDCT) and an
  encoder-path smoke test (stereo TOC bit + decodable packet).

### Deferred

- Stereo + transient (short blocks) combined; intensity stereo for high bands;
  stereo bit-budget beyond an even mid/side split.

## [2.5.5] - 2026-07-01

CELT stereo coupling foundation + toolchain bump to 6.3.27. 799 assertions
(was 796, +3).

### Added

- **CELT stereo coupling primitives** (`src/opus.cyr`) — the invertible core that
  replaces the mono downmix (full two-channel bitstream integration lands in 2.5.6):
  - `_opus_stereo_ms_forward` / `_opus_stereo_ms_inverse` — energy-preserving
    mid/side transform (M=(L+R)/√2, S=(L-R)/√2), exactly invertible.
  - `_opus_stereo_couple` — per-band coupled-vs-dual decision: chooses M/S when it
    concentrates energy (`min(E_M,E_S) < min(E_L,E_R)`), else dual L/R; records the
    choice in `ms_flags[band]`.
  - `_opus_stereo_decouple` — reconstructs L/R from the coupled channels + `ms_flags`;
    `couple∘decouple` is the identity for any per-band choice.
- **Tests** (`+3` assertions): M/S invertibility, per-band decision (correlated band
  → M/S), and an exact couple/decouple roundtrip over mixed correlated/uncorrelated bands.

### Changed

- **Toolchain pin bumped 6.3.25 → 6.3.27** (`cyrius.cyml`) — re-vendored stdlib via
  `cyrius lib sync`, lock refreshed. Clears the pin drift; all tests pass on 6.3.27.

## [2.5.4] - 2026-07-01

CELT transient detection + short-block MDCT. 796 assertions (was 785, +11).

### Added

- **CELT transient detection + short-window switching** (`src/opus.cyr`):
  - `_opus_detect_transient` — flags a frame whose sub-block energy sharply exceeds
    the running average of the preceding sub-blocks (or rises out of near-silence).
  - `_opus_short_mdct` — on a transient, replaces the single long MDCT with
    `CELT_SHORT_BLOCKS` (8) windowed short MDCTs whose coefficients interleave into
    the same 480-coefficient buffer (`out[f*M + b]`), localizing energy in time and
    cutting pre-echo. The band-energy + PVQ-shape path is unchanged (layout-agnostic).
  - `isTransient` coded as one range-coder bit at the head of the CELT frame; the
    decoder reads it back (new `transient_out` on `_opus_decode_celt_frame`).
- **Tests** (`+11` assertions): detector (steady tone → not transient, onset after
  silence → transient, block-0 click → transient — via a symmetric adjacent-pair
  test that catches onsets *and* decays anywhere in the frame) and a full
  transient-frame roundtrip (isTransient bit + energies bit-exact + every K>0
  sub-block shape bit-exact vs the encoder's PVQ).

## [2.5.3] - 2026-07-01

AAC psychoacoustic masking model driving scale-factor allocation. 785 assertions
(was 775, +10).

### Added

- **AAC psychoacoustic model** (`src/aac.cyr`) — simultaneous-masking model that
  coarsens the quantization of masked bands:
  - `_aac_psy_init` precomputes an asymmetric spreading function over scalefactor
    bands (which approximate critical bands): gentle toward higher frequency
    (`PSY_SLOPE_UP` 10 dB/SFB), steep toward lower (`PSY_SLOPE_DOWN` 25 dB/SFB).
  - `_aac_psy_masking_offsets` spreads each band's energy to its neighbours and
    derives a per-band coarseness offset `2·log2(spread/energy)` (0 for an isolated
    band, positive where neighbours mask it, capped at `PSY_MAX_OFFSET` 24).
  - The encoder adds the offset to the baseline scale factor, so masked bands are
    quantized coarser (fewer bits) while unmasked bands are unchanged. Conservative
    by construction — no change to the stereo/CPE path or the decoder.
- **Tests** (`+10` assertions): isolated band → no masking, neighbour masking,
  distance falloff, louder-masker → larger offset, upward/downward asymmetry, and a
  full mono encode→decode frame roundtrip through the masking-driven scale factors.

## [2.5.2] - 2026-07-01

Toolchain modernization (cyrius 6.3.25), the serde Str-deserialize repair, and
AAC TNS (Temporal Noise Shaping). 775 assertions (was 748, +27). The
psychoacoustic model moves to 2.5.3.

### Added

- **AAC TNS (Temporal Noise Shaping)** (`src/aac.cyr`) — AAC-LC long-window,
  mono/SCE path. Linear prediction across frequency flattens the temporal
  envelope so quantization noise tracks transients (reduces pre-echo):
  - Encoder: `_tns_autocorr` → `_tns_levinson` (reflection coefs + prediction-gain
    gate) → quantize (`_tns_quant_coef`) → analysis FIR (`_tns_filter`) over the top
    spectral bands before quantization; `_aac_write_ics_ext` emits the AAC-LC ICS
    extension (`pulse`/`tns`/`gain` presence + `tns_data()`).
  - Decoder: `_aac_parse_ics_ext` / `_tns_parse_data` read `tns_data()`; `_aac_synth`
    applies the synthesis IIR (`_tns_synth_apply`) to the dequantized spectrum before
    the IMDCT. Analysis and synthesis are built from the *same* quantized reflection
    coefficients, so the filter pair is an exact inverse — TNS shapes only quant noise.
  - Stereo/CPE path is untouched (TNS gated to the mono encoder; CPE decode passes a
    zeroed state), keeping that bitstream byte-identical.
- **Tests** (`+27` assertions): filter analysis/synthesis identity (both directions),
  coefficient-quantization roundtrip, `tns_data()` bitstream roundtrip, encoder-analyze
  → decoder-synth exact recovery (TNS engaged), and a full mono encode→decode frame
  roundtrip proving the stream stays in sync.

### Changed

- **Toolchain pin bumped 6.3.19 → 6.3.25** (`cyrius.cyml`) — re-vendored stdlib
  via `cyrius lib sync`; `cyrius.lock` refreshed. Clears the pin-vs-cycc drift
  warning. All 748 assertions pass, fuzz 90K/0, `cyrius vet` clean on 6.3.25.
- **Retired the serde Str-deserialize workaround** (`src/serde.cyr`) — cycc 6.3.25
  fixes `#derive(Serialize)`'s generated `_from_json` for `Str` fields, so
  `ShrAudioMetadata` now round-trips through the derived `ShrAudioMetadata_from_json`;
  the hand-written `audio_metadata_from_json` / `_meta_get_str` (2.4.1) are gone.
  Resolves cyrius issue `2026-07-01-derive-serialize-str-field-deserialize-broken`.

## [2.5.1] - 2026-07-01

shravan's first CELT decode: a matched, provably-invertible range coder, PVQ
shape decode, and a full-stream encode→decode roundtrip proving the entropy
stream inverts exactly (band energies bit-exact, spectral shapes bit-exact).
748 assertions (was 727, +21).

### Added

- **CELT decode path** (`src/opus.cyr`) — inverse of the 2.5.0 encode path:
  - `opus_range_dec_init` / `opus_range_dec_uint` / `opus_range_dec_bit` — a
    byte-oriented range decoder matched to a rewritten LZMA-style encoder.
  - `_pvq_decode_band` — reads a CWRS index over `[0, V(N,K))` and expands it to
    the pulse vector (inverse of `_pvq_encode_band`).
  - `_opus_decode_band_energies` — inverse DPCM of the band-energy sub-stream.
  - `_opus_decode_spectral_shape` — walks bands/sub-blocks in lock-step with the
    encoder (same data-independent K budget) and reconstructs each unit-L2 shape.
  - `_opus_decode_celt_frame` — TOC parse (CELT/FB config 31) → energies + shape.
- **Tests** (`+21` assertions): range-coder roundtrip (mixed uint/bit incl.
  edge cases 0/3 and 65535/65536), PVQ band symbol-exact roundtrip, band-energy
  DPCM roundtrip, and a full-stream CELT frame roundtrip asserting bit-exact
  energies and bit-exact per-sub-block shapes.

### Changed

- **Range coder rewrite** (`src/opus.cyr`) — replaced the custom forward coder
  (which was not bit-exact invertible) with a standard LZMA-style carry coder
  (one-byte cache + deferred `0xFF` run), keeping the public `opus_range_enc_*`
  names so existing call sites are unchanged. Opus encoder output bytes change.
- **Band-energy DPCM is now closed-loop** — the encoder's predictor tracks the
  reconstructed (clamped) value instead of the raw quantized energy, so the
  decoder stays in sync even when a delta saturates.

## [2.5.0] - 2026-07-01

Full PVQ (Pyramid Vector Quantization) spectral shape for the CELT encoder,
replacing the sign-only stub — the CELT quality root the rest of the Opus
encoder work (2.5.x) allocates bits against. 727 assertions (was 610, +117).

### Added

- **PVQ spectral shape** (`src/opus.cyr`) — each band's unit-L2 shape is
  quantized to an integer pulse vector `y` (`sum|y_i| = K`) coded by its CWRS
  index:
  - `V(N,K)` pyramid-count grid (saturating, range-coder-safe) + `_pvq_bits` /
    `_pvq_choose_k` (K from a bit budget).
  - `_pvq_index_encode` / `_pvq_index_decode` — bijective CWRS index ↔ vector,
    roundtrip-tested (exhaustive N=2,K=2 + (3,2)/(4,3)/(2,10)/(1,3); out-of-range
    decode rejected).
  - `_pvq_search` — greedy `Rxy²/Ryy` pulse search; `_pvq_denormalize` shape
    reconstruction. Reconstructed shape cosine **0.9798 vs sign-only's 0.800**.
  - Wired into `_opus_encode_spectral_shape`: per-band split to `N ≤ 32`,
    data-independent K budget (derived from packet size + band geometry so a
    future decoder computes identical K).
  - 4 test functions (+117 assertions → 727).

### Fixed

- **TOC config 30 → 31.** `_opus_encode_celt_frame` labeled its 20 ms CELT/FB
  frames as config 30 (which is 10 ms); corrected to config 31.

### Deferred

- **Full packet encode→decode stream roundtrip → 2.5.1.** Needs a CELT decoder
  (mirrored range decoder + `_opus_decode_spectral_shape`); shravan has no CELT
  audio decode today (`opus_decode_from_packets` is header-only). The PVQ
  quantizer itself is self-consistently roundtrip-validated at the CWRS/shape
  level (above).

## [2.4.4] - 2026-07-01

Opus encoder **framework** — the opening/foundational work for a full Opus
encoder. The feature set (SILK, hybrid, VBR, PVQ, stereo, transient) is
roadmapped as 2.5.x; this release lands only the scaffolding, additive to the
existing CELT-mode encoder. 610 assertions (was 563, +47).

### Added

- **Opus encoder framework** (`src/opus.cyr`, additive — the existing
  `opus_encode` Ogg path is untouched):
  - `OpusEncoder` config/state struct + `OpusMode`/`OpusBandwidth`/`OpusSignal`/
    `OpusApp` enums.
  - `opus_encoder_new` + setters (bitrate / vbr / bandwidth / mode).
  - Mode + bandwidth **selection logic** (`opus_select_mode`,
    `opus_select_bandwidth`, `opus_bandwidth_cap`) — bitrate thresholds + a
    sample-rate cap.
  - RFC 6716 §3.1 **TOC byte** (`opus_toc_config` / `opus_toc_byte`, config 0–31).
  - `opus_encode_frame` dispatch — routes CELT to the existing encoder;
    SILK/HYBRID return `ERR_UNSUPPORTED_FMT` (documented 2.5.x seams).
  - 5 test functions (+47 assertions).
- **Design doc** `docs/adr/0001-opus-encoder-framework.md` (architecture + 2.5.x plan).
- **2.5.x roadmap** — full Opus encoder sequenced (PVQ → transient → stereo →
  rate-control/VBR → SILK → hybrid), with the 2.3.x deferrals + hi-res/DSD interleaved.

### Known issues

- `_opus_encode_celt_frame` hardcodes TOC config 30 (CELT/FB 10ms) for its 20ms
  frames (should be config 31, which `opus_toc_byte` computes); logged as a 2.5.0
  cleanup — it needs a decode round-trip guard, so it's kept out of this additive
  release.

## [2.4.3] - 2026-07-01

Distlib bundle for consumers + a source-layout cleanup. No codec behavior
changes; the 563-assertion test build is unchanged.

### Added

- **`dist/shravan.cyr` — the consumable distlib bundle** (`cyrius distlib`).
  Concatenates the library (`src/shravan.cyr`) + all codec modules into one
  self-contained file (0 unresolved `include`s). Consumers `include` it and
  supply stdlib + `bayan` + `sankoch` from their own manifest, then call
  `shravan_init_constants()`. Verified end-to-end by a consumer smoke test
  (encode → decode → detect through the bundle alone). Declared via `[lib]`
  in `cyrius.cyml`; committed under `dist/`.

### Changed

- **Source layout: `src/main.cyr` split into library + test harness.**
  `src/shravan.cyr` now holds the library (error/format/pcm/wav/aiff/alac +
  codec dispatch + `decode_file`/`decode_reader`); `src/main.cyr` is the test
  harness that `include`s it + the codecs and runs the suite.
- **Codec modules relocated `lib/` → `src/`** (flac, ogg, mp3, tag, fft, opus,
  aac, resample, dither, simd, stream, serde). `lib/` is now **only** the
  vendored Cyrius stdlib — `cyrius distlib` no longer mistakes shravan's own
  modules for external stdlib leaves, and `cyrius vet`/audit is cleaner.
  Updated: `main.cyr`/`bench.cyr`/`fuzz` includes, CI security scan (`src/` +
  `fuzz/` sweep), `cyrius.lock` (31 entries — stdlib only), and docs.

## [2.4.2] - 2026-07-01

Stabilization — rebuild the fuzz harness for the 6.3.19 toolchain. No codec or
serde changes; the 563-assertion suite is unchanged.

### Fixed

- **Fuzz harness (`fuzz/fuzz_codecs.cyr`) builds + runs under Cyrius 6.3.19.**
  6.3.19's stricter linker refuses *reachable* undefined fns; the standalone
  harness referenced `src/main.cyr` helpers it didn't include. Added the missing
  self-contained helpers (`fmtinfo_format`/`sample_rate`/`channels`/`bit_depth`,
  `decode_result_info`/`samples`, `write_u32_le`, `read`/`write_u32_be`, and an
  unreachable `opus_decode_from_packets` stub) and corrected the `ogg_parse_page`
  (4-arg) / `mp3_scan_frames` (2-arg) call sites. Clean build, **90,000 calls /
  0 crashes** via `./fuzz/run.sh`.

### Changed

- **CI** gains `cyrius vet` (include-dependency audit — 0 untrusted / 0 missing)
  and a fuzz smoke pass (1000 iters, gated on "0 crashes").

## [2.4.1] - 2026-07-01

Full serde type coverage — restores the complete Rust `#[derive(Serialize,
Deserialize)]` surface as JSON serialization. **563 assertions pass** (539 + 24).

### Added

- **Enums** (value-based JSON, like AudioFormat): `MpegVersion`, `MpegLayer`,
  `ChannelMode` (mp3), `ResampleQuality` (resample) — `_to_json`/`_from_json`.
- **Int-field structs** via `#derive(Serialize)`: `ShrAlacConfig` (10 fields),
  `ShrMp3FrameInfo` (7), `ShrOpusHead` (6) — full roundtrip.
- **`ShrAudioMetadata`** (7 `Str` tag fields: title/artist/album/track_number/
  year/genre/comment). `to_json` via `#derive(Serialize)`; roundtrip verified.
- **Codec markers** `WavCodec`…`AlacCodec` (8) — serialize their identity
  (`{"codec":"…"}`).
- Roundtrip tests for every type (+24 assertions → 563).

### Fixed / worked around

- **`#derive(Serialize)` `Str`-field deserialize is broken on cyrius 6.3.x** —
  the generated `_from_json` yields garbage for `Str` fields (`to_json` is fine;
  int fields roundtrip fine). Filed upstream (cyrius issue
  `2026-07-01-derive-serialize-str-field-deserialize-broken`, with repro).
  shravan ships a hand-written `audio_metadata_from_json` (bayan
  `json_get`, 8-byte `Str`-handle fields) as the stopgap — remove once the
  derive is fixed.

## [2.4.0] - 2026-07-01

Language/toolchain modernization to Cyrius **6.3.19** (from cc3 4.10.3). No
codec behavior changes, no dependency removals; the previously-orphaned JSON
serialization module is now live. **539 assertions pass** (520 + 19 serde).

### Added

- **`serde` metadata serialization wired in.** `lib/serde.cyr` (a partial port
  of Rust's `#[derive(Serialize, Deserialize)]` usage, present-but-orphaned
  since 2.0.0) is now `include`d and tested. `ShrFormatInfo_to_json`/`from_json`
  are generated by Cyrius 6.x's `#derive(Serialize)` (unavailable under 4.10.3,
  which is why it never linked before); `AudioFormat`/`PcmFormat`/`ShravanErr`
  keep their hand-written `_to_json`/`_from_json`. 5 tests join the suite
  (+19 assertions → 539). Full Rust type coverage tracked for **2.4.1**.
- **`bayan` stdlib dep** — the modern home of the JSON API (`bayan_json_parse`/
  `bayan_json_get_int`), which serde uses. Replaces the old `json` stdlib leaf,
  removed upstream at 6.1.25 (carved into `bayan`).

### Changed

- **Toolchain: cc3 4.10.3 → Cyrius 6.3.19.** The pin now lives in
  `cyrius.cyml [package].cyrius` (single source of truth read by CI); the
  `.cyrius-toolchain` file is removed (retired upstream at 5.11.5).
- **Manifest: `cyrius.toml` → `cyrius.cyml`.** `version = "${file:VERSION}"`
  interpolates the `VERSION` file; adds `language = "cyrius"` and the toolchain
  pin. `[build]` entry/output are metadata; the codec `include`s are unchanged.
- **Stdlib vendoring via `cyrius lib sync`.** `lib/` now holds the
  version-matched 6.3.19 stdlib snapshot (28 files incl. platform variants),
  replacing the stale hand-vendored 4.10.3 copies that shadowed the toolchain.
  `cyrius.lock` records per-file hashes (committed).
- **`math` split: `ganita` added to `[deps].stdlib`.** The transcendentals
  shravan calls (`f64_pow/asin/acos/atan2/sinh/cosh/tanh/hypot`) moved out of
  `math` into `ganita` in the 6.x line; `f64_sin/cos/sqrt/exp/ln/…` are builtins.
  `math`, `tagged`, and the `sankoch` compression dependency are **retained**.
- **CI/release workflows modernized.** Toolchain installed via the canonical
  `install.sh` reading the `cyrius.cyml` pin (no hardcoded version); build flow
  is `cyrius lib sync` → `cyrius deps` → `cyrius build` → tests/bench; actions
  SHA-pinned; security scan scoped to shravan-authored modules.
- **`.gitignore`**: ignore `build/` only; keep `cyrius.lock`, `lib/`, and
  `bench-history.csv` tracked. Rust-era cruft dropped.

### Fixed

- **`file_open` arity** — 6.x signature is `file_open(path, flags, mode)`;
  `decode_file` now passes the mode argument.
- **`flac_stream_new` call arity** — dropped the silently-ignored `chunk_frames`
  argument (the FLAC streaming decoder is full-buffer by design).
- **Symbol collisions with the modern stdlib**: dropped shravan's `F64_HALF`
  (now from `math.cyr`); dropped shravan's `is_err`/`err_code` (byte-identical to
  the `syscalls` stdlib, pulled via `io`); renamed the packed-Result `is_ok` →
  `res_ok` to avoid clashing with `result.cyr`'s tagged-Result `is_ok`. The
  `sankoch`/audio `detect_format` last-wins warning is unchanged from baseline.
- **Stale `v2.0.0` startup/benchmark banners** → `v2.4.0`.

## [2.3.2] - 2026-04-15

### Added

- **AAC per-band codebook selection**: Encoder now selects the most efficient Huffman codebook per scale factor band based on max quantized magnitude. Codebook 1-2 for |q|<=1, 3-4 for |q|<=2, 5-6 for |q|<=4, 7-8 for |q|<=7, 9-10 for |q|<=12, 11 for escape. Quad codebooks used when band width is divisible by 4.
- **AAC M/S stereo encoding**: Stereo input now encoded as CPE (Channel Pair Element) with Mid/Side transform (M=(L+R)/2, S=(L-R)/2) and ms_mask_present=2 (all bands). Side channel gets own MDCT, quantization, section coding, scale factors, and spectral data. Previously downmixed to mono SCE.
- **AAC VBR mode**: `aac_encode_vbr(samples, rate, channels, quality, out)` with quality levels 1-5 (64-320 kbps). Maps quality to target bitrate and delegates to CBR encoder.
- **AAC spectral encoding per-codebook**: Encoder now writes spectral data using the selected codebook (1-10 + escape) instead of always using escape pairs (cb=11). Signed quads, unsigned quads with sign bits, signed pairs, unsigned pairs with sign bits all supported.

## [2.3.1] - 2026-04-15

### Added

- **AAC spectral codebooks 9-10**: ISO 14496-3 HCB9/HCB10 (169 entries each, 13x13 unsigned pairs, values 0-12). Full Huffman decode with 2-level LUT + sorted fallback. Wired into spectral decoder dispatch.
- **AAC 4-tuple decoder**: `_aac_decode_spectral_quad()` for codebooks 1-4. Decodes 4 spectral values per Huffman symbol. HCB1/2 (signed quads, values -1,0,1), HCB3/4 (unsigned quads with sign bits, values 0,1,2). Dispatch wired into spectral decoder for bands using codebooks 1-4.
- **AAC codebook 1-4 table data**: ISO 14496-3 HCB1-4 (81 entries each, 3^4 4-tuple codebooks). Tables loaded with 2-level LUT decode.

## [2.3.0] - 2026-04-15

### Added

- **`decode_file(path)`**: Read audio file from disk, auto-detect format, decode. Returns decode_result or error.
- **`decode_reader()` / `decode_reader_feed()` / `decode_reader_flush()`**: Streaming decoder with format auto-detection on first feed. Dispatches to WAV/FLAC/AIFF stream decoders.
- **AAC 2-level Huffman lookup**: 256-entry level-1 table for O(1) decode of short codes (<=8 bits), sorted-scan fallback for long codes. Applied to SCF and all spectral codebooks.
- **AAC codebook 1-4 dispatch**: Band skipping for 4-tuple codebooks (structural framework, tables deferred to v2.3.1).
- **AAC codebook 9-10 dispatch**: Unsigned pair decode (13x13 grid) wired into spectral decoder (tables deferred to v2.3.1).

### Security

- **Full security audit**: 21 findings (7 P0, 5 P1, 6 P2, 3 P3) — all fixed. Report: `docs/audit/2026-04-15-security-audit.md`.
- **P1 fixes**: FLAC unary decode bound (1M→32768), ALAC INT64_MIN guard, MP3 frame_size OOB guard, AAC frame count cap (65536), FLAC SEEKTABLE cap (1024).
- **P2 fixes**: Vorbis Comment zero-length/count cap, MDCT size validation (n>=4, n%4==0, n<=8192), bitreader parameter validation (0..64), `_safe_alloc_mul()` overflow-checked allocator (256MB cap), tag COMM validation.
- **Fuzzing harness**: `fuzz/fuzz_codecs.cyr` with `fuzz/run.sh` — FLAC metadata, Ogg page parse, MP3 frame scan, ID3v2 tag read, random bytes. 90,000 calls, 0 crashes.
- **Upstream filed**: 3 Cyrius stdlib issues for v5.0.1 (alloc overflow, vec capacity overflow, allocation size cap).
- 520 tests, 0 failures

## [2.2.0] - 2026-04-15

### Added

- **PCM reciprocal constants**: Pre-computed `F64_RCP_32768`, `F64_RCP_8388608`, `F64_RCP_128`, `F64_RCP_2147483648` for mul-instead-of-div in all PCM conversion hot paths. Pre-computed `F64_2_NEG_149` for f32 denormal conversion (was calling `f64_pow` per sample).
- **Vec pre-allocation**: `_vec_new_cap(n)` helper allocates vectors with known capacity, eliminating growth during PCM decode.
- **FFT twiddle pre-computation**: Per-recursion-level twiddle factor tables in `_fft_forward`. MDCT/IMDCT pre/post twiddle tables cached and reused across calls. Eliminates `f64_cos`/`f64_sin` from inner loops.
- **AAC sorted Huffman decode**: `_aac_huff_decode_fast` uses sorted-by-length codebook tables with skip-ahead, replacing linear scan of all entries per bit. Applied to SCF (121 entries) and spectral codebooks 5-8 (64-81 entries).
- **MDCT N/2-point FFT**: Rewrote `fft_mdct` and `fft_imdct` to use N/2-point (forward) and N/4-point (inverse) complex FFTs with folding and pre/post rotation, replacing the 2N-point zero-padded approach. 4x reduction in FFT size for AAC/Opus transforms.
- **Resample polyphase filter table**: Pre-computes 256-phase windowed sinc kernel table, replacing per-sample `f64_sin`/`f64_cos` calls (65 trig calls per sample for BEST quality → 0).
- **Struct size constants**: `FMTINFO_SIZE`, `DECODE_RESULT_SIZE`, `BITREADER_SIZE` replace magic numbers.

### Security (P0 — from 2026-04-15 audit)

- **SEC-001**: WAV chunk size overflow — added bounds guard before pos advancement.
- **SEC-002**: AIFF SSND offset underflow — validated `ssnd_offset <= ck_size - 8` and `pcm_start <= len`.
- **SEC-003**: AIFF chunk pos overflow — added overflow guard matching WAV fix.
- **SEC-004**: WAV extensible format OOB — added `pos + 34 <= len` guard.
- **SEC-005**: FLAC block_size cap — enforced `block_size <= 65535` in `flac_decode_block_size()`.
- **SEC-006**: FLAC metadata block OOB — validated `pos + block_size <= len` before advancing.
- **SEC-007**: Ogg packet accumulation overflow — added integer overflow check and 16MB hard cap.
- Full report: `docs/audit/2026-04-15-security-audit.md` (14 deferred P1-P3 items on v2.3.0 roadmap, 3 upstream issues filed for Cyrius 5.0.1).

### Changed

- **Compiler requirement**: cc3 >= 4.10.3 (from 3.4.3). Gains linalg, security fixes, improved stdlib.
- **Stdlib modernization**: Removed 12 vendored stdlib files from `lib/`. Stdlib modules now resolved via `[deps] stdlib` in `cyrius.toml` (matches modern Cyrius project pattern). `lib/` now contains only project-specific codec modules.
- **Sankoch compression dep**: Added sankoch 1.0.0 (LZ4, DEFLATE, zlib, gzip) via `[deps.sankoch]`. Provides `compress()`, `decompress()`, `detect_format()` for container format support.
- **Stdlib upgrades**: math.cyr gains inverse trig (asin, acos, atan2), inverse hyperbolic, lerp/hypot/sign/trunc/fract, gcd/lcm/fibonacci/binomial, f64_parse. string.cyr gains word-at-a-time strlen, rep movsb memcpy/memset, ASCII case helpers. str.cyr gains type annotations + 15 extended functions. fmt.cyr gains fmt_int_fd, efmt_int, bounds-checked fmt_sprintf.
- **WAV chunk parsing**: Replaced 12 nested single-byte comparisons with `read_u32_le` for RIFF, WAVE, fmt, data chunk detection.

### Performance

- **MDCT forward 2048: 8.534ms → 1.566ms (5.45x faster)** — N/2-point FFT rewrite
- WAV decode 1sec i16: 1.135ms → 766us (1.48x faster)
- PCM i16→f64 4096: 93.8us → 70.1us (1.34x faster)
- PCM i24→f64 4096: 94.8us → 67.3us (1.41x faster)
- PCM u8→f64 4096: 78.5us → 50.3us (1.56x faster)
- FFT forward 1024: 1.714ms → 1.528ms (1.12x faster)
- FLAC encode/decode: stable (not affected by FFT/PCM changes)
- Binary: 475KB (from 333KB in v2.1.1 — sankoch + polyphase tables)
- 520 tests, 0 failures

## [2.1.1] - 2026-04-11

### Added

- **AAC spectral Huffman codebooks 5-8**: HCB5 (signed pairs, 81 entries), HCB7/HCB8 (unsigned pairs, 64 entries each). Decoder dispatches by codebook number — codebooks 5-8 use ISO standard tables, codebook 11 uses existing escape pair decoder. Codebooks 1-4 and 9-10 skip bands (not yet implemented).
- **AAC short window support**: EIGHT_SHORT_SEQUENCE (window_seq=2) with 8x MDCT-256 transforms, sine windowing, sub-window overlap-add. Short SWB offset table (14 bands for 48kHz). Scale factor grouping parsed.
- **Huffman decoder**: general-purpose `_aac_huff_decode()` — bit-by-bit accumulation with linear codebook search. Used by scale factor and spectral codebook decoders.

### Changed

- **AAC section coding**: now uses correct escape values for short windows (3-bit length, max 7) vs long windows (5-bit length, max 31).
- Binary: 333KB (from 313KB in v2.1.0)

## [2.1.0] - 2026-04-11

### Added

- **AAC-LC decoder**: from-scratch implementation — bitreader, ICS/section/scale factor parsing, escape-pair spectral decode, inverse quantization (|q|^4/3), IMDCT via fft.cyr, sine window overlap-add. Encode/decode roundtrip verified.
- **AAC M/S stereo**: CPE (Channel Pair Element) parsing, common window flag, per-band and all-band M/S mask, stereo reconstruction (L=M+S, R=M-S). Stereo roundtrip tested.
- **AAC scale factor Huffman codebook**: ISO 14496-3 standard codebook (121 entries). Both encoder and decoder now use real Huffman codes instead of simplified 7-bit fixed encoding.
- **Metadata writing**: `tag_write_id3v2()` and `tag_write_vorbis()` — write/read roundtrip tested for title, artist, album, year, genre fields.
- **FLAC incremental streaming**: `flac_stream_feed` now decodes frame-by-frame via `flac_decode_range` with sample offset tracking, emitting SAMPLES events incrementally instead of buffering everything.
- **FLAC SEEKTABLE parsing**: `flac_parse_metadata` now parses SEEKTABLE blocks (type=3) into SeekPoint structs for fast seeking.
- **FLAC decode_range()**: sample-accurate seeking with SEEKTABLE support.
- **Opus decode path**: `opus_decode_from_packets` parses OpusHead/OpusTags, scans granule positions for duration. `ogg_decode` dispatches to Opus on OpusHead magic.
- **Opus parse_tags()**: reads Vorbis Comment metadata from OpusTags packets.
- **codec_open ALAC/Opus dispatch**: `codec_open` now dispatches all 8 formats including ALAC and Opus.
- **PCM i24 unpacked conversions**: `i24_to_f64()` and `f64_to_i24()` for unpacked i32-stored 24-bit samples.
- **Stream chunk_size**: WAV and AIFF streaming decoders now use `chunk_frames` parameter — emit multiple SAMPLES events of controlled size.
- 21 new test assertions (520 total, 0 failures)

### Changed

- **Compiler requirement**: cc3 >= 3.4.3 (tok_names overflow fix)
- **Rust source removed**: `rust-old/` deleted, preserved at git tag `1.1.0`
- **CI workflows**: bumped to Cyrius 3.4.3, added format check, security scan jobs
- **Benchmarks**: 9 benchmarks (WAV encode/decode, PCM i16/i24/u8, FFT 1024, MDCT 2048, FLAC encode/decode)

### Performance

- FLAC decode 1sec i16: 8.0ms (vs Rust 1.53ms — 5.2x)
- FLAC encode 1sec i16: 20.7ms (vs Rust 2.77ms — 7.5x)
- WAV decode 1sec i16: 2.0ms (vs Rust 47µs — 42x)
- Compile: 420ms full rebuild (vs Rust 6.9s — 16x faster)
- Binary: 313KB (vs Rust 1.83MB — 6x smaller)

## [2.0.0] - 2026-04-10

Full rewrite from Rust to Cyrius. Zero external dependencies.

### Changed

- **Language**: Rust → Cyrius (self-hosting systems language, zero LLVM/crate dependency)
- **Sample format**: f32 → f64 internally (native Cyrius SSE2, higher precision)
- **Error handling**: thiserror enum → packed Result (negative = error, bit 63 set)
- **Build system**: Cargo → `cyrius build` (211ms full compile, 253KB static ELF)
- **Architecture**: Library crate → include-based modules (`lib/*.cyr`)
- Rust implementation archived in `rust-old/` (removed in v2.1.0, preserved at tag `1.1.0`)

### Preserved

- All 16 modules ported: error, format, pcm, codec, wav, aiff, alac, flac, ogg, mp3, tag, fft, opus, aac, resample, dither, simd, stream
- Full encode/decode for WAV, FLAC, AIFF, Opus, AAC
- Decode-only for ALAC, MP3 (header parsing)
- Ogg container parsing/muxing, metadata tag reading (ID3v2, Vorbis Comment)
- Mixed-radix FFT, MDCT/IMDCT, sinc resampler, TPDF + noise-shaped dithering
- Streaming decoders for WAV, FLAC, AIFF
- 415 test assertions, all passing

### Performance

- WAV decode 1sec i16: 1.1ms (vs Rust 22.8µs — ~50x, expected without LLVM optimizer)
- PCM i16→f64 4096: 93µs (vs Rust 429ns — ~217x, scalar vs auto-vectorized)
- Compile: 211ms (vs Rust 5.1s — 24x faster)
- Binary: 253KB (vs Rust 1.8MB — 7x smaller)

## [1.1.0] - 2026-04-02

Four new codecs to eliminate tarang's C FFI audio codec dependencies.

### Added

- **AAC-LC decoder**: ADTS parsing + symphonia-codec-aac backend, mono through 7.1 channels. Feature: `aac` (requires `std`)
- **AAC-LC encoder**: From-scratch MDCT-based encoder producing ADTS bitstream. Per-band quantization, scale factor Huffman coding, escape-pair spectral coding. Mono/stereo, all standard AAC sample rates, CBR. Feature: `aac`
- **Opus CELT encoder**: From-scratch CELT-mode encoder producing valid Ogg/Opus files. Mono/stereo, 48 kHz, CBR 32-256 kbps, 20ms frames. Feature: `opus`
- **ALAC decoder**: From-scratch Apple Lossless decoder for raw frames from MP4. 16/20/24/32-bit, mono/stereo, LPC prediction, adaptive Rice-Golomb coding, stereo de-matrixing. `no_std` compatible. Feature: `alac`
- **Ogg muxer**: Page construction with CRC-32, lacing, BOS/EOS flags
- **Mixed-radix FFT** (`src/fft.rs`): Shared O(N log N) FFT (factors of 2, 3, 5) and MDCT, used by Opus and AAC encoders
- `AudioFormat::Aac`, `AudioFormat::Alac` variants with ADTS format detection
- `AacCodec`, `AlacCodec` structs implementing `AudioCodec` trait
- `AlacConfig` for parsing ALACSpecificConfig from MP4 extradata
- `#[must_use]` on all 22 public `Result`-returning functions
- Opus encode benchmark

### Fixed

- ADTS/MP3 format detection correctly distinguishes AAC (layer=0) from MP3 (layer!=0)
- Opus encoder TOC byte reflects mono-coded bitstream regardless of input channel count

## [1.0.1] - 2026-03-28

### Fixed
- `streaming` feature now compiles without requiring `flac` or `aiff` features — `FlacStreamDecoder` and `AiffStreamDecoder` are properly gated behind their respective feature flags

## [1.0.0] - 2026-03-28

### Added

- **Reference implementation tests**: Validate WAV, FLAC, and AIFF decode against ffmpeg-generated reference files. Cross-format consistency checks (WAV vs FLAC vs AIFF from same source).
- **WAVE_FORMAT_EXTENSIBLE support**: WAV decoder now handles format code 0xFFFE with SubFormat GUID extraction, enabling 24-bit and multi-channel WAV files from professional tools.
- Performance validated within 2x of C reference implementations (libFLAC, dr_wav)
- Test coverage: 85%+ line coverage (90%+ excluding platform-conditional dead code)

## [0.5.0] - 2026-03-28

### Changed

- **SIMD-accelerated resampling**: Inner kernel loop uses `simd::weighted_sum()` when `simd` feature is enabled, replacing manual f64 accumulation with vectorized f32 path
- **Multi-channel resampling optimization**: Deinterleave → per-channel resample → reinterleave for sequential memory access. Significant improvement for stereo and multi-channel audio
- **Async I/O**: `StreamDecoder::feed()` is async-compatible by design (non-blocking, caller-driven). No runtime dependency needed — callers use their async runtime to drive the streaming trait.

## [0.4.0] - 2026-03-28

### Added

- **Streaming decoders**: `StreamDecoder` trait with chunk-at-a-time `feed()`/`flush()` API, `StreamEvent` enum (`Header`, `Samples`, `End`)
- **WavStreamDecoder**: Streaming WAV decoder with configurable chunk size, state-machine header parsing, incremental PCM conversion
- **FlacStreamDecoder**: Streaming FLAC decoder using `decode_range()` with sample offset tracking to avoid duplicate emission
- **AiffStreamDecoder**: Streaming AIFF/AIFF-C decoder with big-endian PCM, `sowt` little-endian support
- **`decode_reader()`**: Read entire `std::io::Read` stream and auto-detect/decode (std-only)
- **`decode_file()`**: Read file from path and auto-detect/decode (std-only)
- Feature gate: `streaming` (requires `std`)

## [0.3.0] - 2026-03-28

### Added

- **Ogg container parser**: Page parsing, packet extraction, CRC-32 verification, continuation page handling, codec detection (delegates to Opus)
- **AIFF decoder/encoder**: FORM/AIFF and FORM/AIFC parsing, COMM chunk with 80-bit extended float sample rate, SSND chunk, big-endian PCM 8/16/24/32-bit decode and encode
- **MP3 frame sync**: Frame header parsing (MPEG 1/2/2.5, Layer I/II/III), bitrate and sample rate tables, frame size calculation, ID3v2 tag skipping, multi-frame scanning
- **Opus header parsing**: OpusHead identification header, OpusTags comment header (via Ogg container), duration from granule position
- Format detection for Ogg (`OggS`), AIFF (`FORM....AIFF`), AIFF-C (`FORM....AIFC`), MP3 (ID3v2 or MPEG sync word)
- Codec structs: `OggCodec`, `AiffCodec`, `Mp3Codec`, `OpusCodec` (all with Serialize/Deserialize)
- Feature gates: `ogg`, `aiff`, `mp3`, `opus` (opus depends on ogg)

### Fixed

- MP3 frame size calculation for Layer II with MPEG-2/2.5 (used wrong samples-per-frame divisor)
- Dither functions now clamp `target_bits` to 1..=32 instead of panicking on 0

## [0.2.1] - 2026-03-28

### Added

- **SIMD-accelerated PCM conversion**: SSE2 (x86_64) + NEON (aarch64) kernels for i16/f32 conversion and weighted dot product, behind `simd` feature gate
- **Dithering module**: TPDF and noise-shaped dithering for bit-depth reduction, behind `dither` feature gate
- **Extended PCM conversions**: `f64_to_f32`, `f32_to_f64`, `u8_to_f32`, `f32_to_u8`

## [0.2.0] - 2026-03-28

### Added

- **FLAC encoder**: Fixed prediction (orders 0-4) with automatic order selection, Rice entropy coding with optimal parameter selection, mid-side stereo channel decorrelation, MD5 signature computation
- **LPC subframe decoding**: Full support for LPC orders 1-32 with quantized coefficients and variable precision
- **CRC verification**: CRC-8 (frame header) and CRC-16 (full frame) validation on decode, correct CRC emission on encode
- **FLAC seeking**: `decode_range()` for sample-accurate seeking, SEEKTABLE metadata block parsing, range-based decoding with start/end sample positions
- **BitWriter**: MSB-first bitstream writer for FLAC frame construction
- FLAC encode/decode benchmarks (Criterion)

### Fixed

- `resample()` now rejects `source_rate=0` instead of panicking with capacity overflow
- WAV chunk parser uses saturating arithmetic to prevent overflow on malicious chunk sizes

## [0.1.0] - 2026-03-26

### Added

- **WAV codec**: RIFF WAVE encoder/decoder supporting PCM 8/16/24/32-bit integer and IEEE float 32-bit
- **FLAC decoder**: STREAMINFO parsing, frame sync, Constant/Verbatim/Fixed subframes, Rice entropy coding, channel decorrelation (independent, left-side, right-side, mid-side)
- **PCM conversions**: i16/i24/i32 to f32 and back, interleave/deinterleave, packed 24-bit support
- **Sinc resampler**: Windowed sinc interpolation with Blackman-Harris window, Draft/Good/Best quality levels
- **Tag reading**: ID3v2 (v2.2/v2.3/v2.4) and Vorbis Comment metadata parsing
- **Codec trait**: Unified `AudioCodec` trait and auto-detect `open()` function
- **Format detection**: Magic byte detection for WAV and FLAC
- Feature-gated modules: `wav`, `flac`, `pcm`, `resample`, `tag`, `logging`
- `no_std` support (with `alloc`)
- Serde serialization for all public types
- Comprehensive test suite with integration tests
- Criterion benchmarks for WAV decode, PCM conversion, and resampling
