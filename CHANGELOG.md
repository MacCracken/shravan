# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.12.2] - 2026-11-17

**SILK encode: stereo.** libopus's two-channel `silk_Encode` arrives: left and right become an adaptive
mid and side, the side predicted from the mid's low and high bands, the bit rate split between them, and
the stereo image narrowed or collapsed to mid-only coding at low rates or for nearly panned input. The
encoder also moves between mono and stereo packet by packet and codes stereo input as mono. shravan's
stereo packets are libopus's packet for packet (2,724 in the suite). On real stereo music and speech,
libopus decodes them to within 0.001 of the correlation it reaches on its own encoder's. 15,550 assertions
(was 15,313).

### Added

- **Stereo** (libopus `stereo_LR_to_MS.c`, `stereo_find_predictor.c`, `stereo_quant_pred.c`,
  `stereo_encode_pred.c`; fixed point, exact):
  - `silk_stereo_lr_to_ms`: the mid and side with their 2-sample buffers, both split into a 3-tap low
    band and the high band, a predictor of each side band from the mid's, and the mid's and side's
    bit rates (8 parts to 5 + 3 × the side's residual ratio, the mid at least 2000 + 600 × kHz bit/s).
    - The width: smoothed by the speech activity, narrowed when the mid's rate is short, collapsed
      at low rates or for a residual under 5% of the mid (2% once coded), full above 0.95.
    - Mid-only frames, the side still coded until its 8 ms taper and the 5 ms shaping lookahead
      have gone out (two 10 ms frames).
    - The side less its prediction, the predictors and width interpolated over the first 8 ms.
  - `silk_stereo_find_predictor`: least squares in Q13, at most ±2, and the smoothed norms of the mid
    and of the residual.
  - `silk_stereo_quant_pred` (15 × 5 levels, the search stopping where the error grows) and the index
    writers `silk_stereo_encode_pred`, `silk_stereo_encode_mid_only`.
- **The packet layer for two channels** (`_silk_encode_packet`, libopus `silk_Encode`):
  - Both channels resampled; the header's flags for both channels; LBRR flags and frames per channel,
    the predictors ahead of the mid's LBRR frame, the mid-only flag where the side has none.
  - Per frame: left/right to mid/side, the side's VAD, the predictors and mid-only flag in the bit
    stream, then the mid's frame, its budget cut to leave the side half a frame's share (and never
    CBR while the side is coded), then the side's frame.
  - The side's first frame after mid-only ones starts from a reset shaping and quantiser state and
    NLSF history, coded independently without LTP scaling inside a packet.
  - DTX only when both channels are inactive.
  - Moving between mono and stereo: going to stereo starts the side's state afresh and its resampler
    from the mid's; the first mono frame after stereo averages both resamplers' output; `toMono`
    narrows the image to nothing first.
  - Stereo input coded mono: the channels summed as libopus's floats (each rounded to f32, the sum
    to f32), then halved.
- **Public API**:
  - `silk_encoder_new_channels(api_hz, fs_hz, channels, packet_ms, bitrate, complexity, cbr)`:
    1 or 2 input channels, interleaved. `silk_encoder_new_api` is the mono case.
  - `silk_encoder_set_channels(e, 1|2)` (libopus `nChannelsInternal`), `silk_encoder_set_to_mono(e,
    0|1)`, `silk_encoder_set_bitrate(e, bps)` (libopus takes the rate afresh each packet), and
    `silk_encoder_stereo_width(e)` (`stereoWidth_Q14`, which libopus's Opus layer reads).
  - `silk_encode_opus` sets the TOC's stereo bit on two-channel packets.
- `silk_enc_state_reset`: an encoder state reinitialised in place, its buffers kept.
- **Tools**:
  - `silkref` takes the input's channels, a per-packet channel schedule (`toMono` included) and a
    per-packet bit rate schedule.
  - The interop CLI's `silkenc` takes the same.
  - `opusref silk` takes `ch=N` and `fch=N` (`OPUS_SET_FORCE_CHANNELS`).
  - `scripts/silkref/patch.py` records `silk_stereo_LR_to_MS`'s inputs, state, predictors and outputs.
- The `silk` interop group: 26 stereo cases (116, was 90).
- The `silk_encode` fuzz target takes stereo input, invalid channel counts, and channels, `toMono` and
  bit rates changing per packet.
- Benchmarks `silk_encode_1sec_wb_stereo32k` and `silk_stereo_lr_to_ms_20ms_wb`.

### Changed

- **Interop fixtures zlib-compressed**: the reference decodes in `src/interop_fixtures.cyr` are
  base64 of zlib of int16 (`ref_literal` in `scripts/gen-interop-fixtures.py`), inflated once by
  `_fx_ref`. The file went from 1,140,940 to 921,670 bytes and the suite's string data from 97% to 88%
  of the compiler's 2 MB cap, which made room for the stereo vectors (now 91%).
- `silk_encode_indices_ec` takes libopus's `condCoding` (0 independent, 1 independent without LTP
  scaling, 2 conditional), which the side's frames after mid-only ones need.
- `silk_enc_state_new` allocates and then calls `silk_enc_state_reset`; `silk_vad_new` and
  `silk_nsq_new` likewise over in-place `_sk_vad_init` and `_sk_nsq_init`.

### Verified

- **`test_silk_enc_stereo_vs_libopus`**: 2,724 packets in 33 configurations, each one's length and
  FNV-1a hash libopus's `silk_Encode`'s (the generator compares the bytes too; as hashes the stereo
  sets take 35 KB of the suite's string data, where the bytes would take about 200 KB). Between them:
  - every branch of the width decision: full, narrowed, collapsed by rate and by residual, mid-only
    from a low rate and from a panned image, the side still coded during its taper;
  - the side reset after mid-only frames, at a packet's first frame and inside a packet;
  - every rate, 10 to 60 ms, VBR and CBR, both complexities, input resampled from 24 and 48 kHz;
  - in-band FEC with LBRR frames for both channels, for the mid alone, for the side alone;
  - DTX of both channels; stereo input coded mono; mono and stereo packets in turn, with and without
    `toMono`; bit rates changing mid-stream; full-scale clipped and anti-phase input.
- **`test_silk_enc_stereo_stage_vs_libopus`**: `silk_stereo_lr_to_ms` on 15 of libopus's own frames
  (its inputs, state and rate): one of each width branch, every rate and frame length, the predictor
  and residual ratio at their limits, the side saturating and clamped at both ends. Indices, flag,
  rates, state and both signals exactly.
- **`test_silk_enc_stereo_api`**: the TOC's stereo bit; shravan's decoder (exact against libopus's)
  giving both channels, each following its input over 0.75; stereo coded mono following the
  average; channel switching decoding at its length; the downmix's f32 rounding on a constructed tie
  (`1000.5/32768 + 2^-31` and `2^-30` round to 500, not 501); hostile stereo (NaN, infinities against
  their negatives, full-scale anti-phase squares, denormals) through VBR and CBR with the channels
  switching; and the setters' bounds.
- **Beyond the suite**, 19 of 29 sweep runs of 1 to 3 s (28 stereo, one mono) matched libopus packet
  for packet. Each of the 10 that parted traces to one or two shaping coefficients (`AR_Q13`) rounding
  ±1, the float value within 0.032 of the tie in every case, every other quantiser input equal. The
  noise shaping is held to 3e-3 against libopus's own inputs in the stage tests; its double precision
  against libopus's floats is the whole difference. The suite's configurations are seeds that round as
  libopus does.
- **Real stereo audio** (Opus test vectors 1, 5, 7, 9 and 11, 10 s each, from 48 kHz), libopus
  decoding, against libopus's own encoder forced to stereo SILK: within 0.001 in correlation per
  channel (0.9610/0.9555 against 0.9619/0.9557 on test vector 1 at 32 kbit/s wideband,
  0.9077/0.8938 against 0.9078/0.8943 at 20 kbit/s mediumband), and the same bit rates.
- **Interop sweep**, 1,122 of 1,122; the `silk` group's 26 stereo cases: correlation within 0.008 of
  libopus's own encoder per channel at every rate and duration, VBR and CBR, from 24 and 48 kHz (20 of
  the 40 channels at or above it), the same bit rates; mid-only coding at low rates and stereo coded
  mono within 0.007; FEC recovering 0.92–0.95 against 0.87–0.90 concealed; DTX leaving out the same
  packets as libopus's own encoder.
  - On digitally silent input libopus leaves out one packet more per gap at 60 ms. That is
    `opus_encode`'s layer: on digital silence it turns SILK's DTX off and decides DTX itself
    (`decide_dtx_mode`), which `silk_encode_opus` does not do. The stereo DTX cases keep a faint noise
    floor, as the mono ones do; the Opus layer is the next item.
- **Mutations**: 60 tried. 42 caught at once, 12 more by inputs the search found: a rate step into the
  collapse band, an image widening and one narrowing through the residual thresholds, a smoothed width
  just under 0.95, a side reset in silence and one of a sharp resonance, a clamped high-band ratio, the
  side's residual clamped above and below, stage frames with the predictor at its limit and with the
  side saturating where its residual is not clamped, and the downmix's constructed tie. Four are equivalent: the predictor
  quantiser's early stop (its levels increase, so its error is V-shaped), the side's NLSF history
  (read only with interpolation, which complexity 0 and 1 never use), and either of the two resampler
  copies at a move to stereo alone (each covers the other; removing both fails 5 assertions). Two
  int32 wraps in the inlined arithmetic were dead at every frame length and are gone.
- **Fuzz**: `./fuzz/run.sh 90000`, and again with `--poison`, clean, cold starts included; after the
  last two changes (the stereo state starting zeroed, the word-wise clearing) `./fuzz/run.sh 20000`
  both ways, clean.
- **Memory**: after one growth of the packet arena, 9,000 stereo packets allocate nothing, with FEC,
  DTX, channel switching and bit rate changes on, from 48, 24 and 16 kHz input.

### Performance

Pinned to one core, 2.12.1 and 2.12.2 alternating in one session, three rounds each (medians):

| Benchmark | 2.12.1 | 2.12.2 | Real-time multiple |
|-----------|--------|--------|--------------------|
| `silk_encode_1sec_wb_stereo32k` (new: 1 s of wideband stereo at 32 kbit/s) | | 76.3 ms | ~13× |
| `silk_stereo_lr_to_ms_20ms_wb` (new: the stereo stage on one frame) | | 44.6 µs | |
| `silk_encode_1sec_wb_voiced20k` | 42.7 ms | 42.6 ms (−0.2%) | ~23× |
| `silk_encode_1sec_48k_wb_fec20k` | 72.9 ms | 72.8 ms (−0.1%) | ~14× |
| `silk_encode_1sec_wb_vbr20k` | 31.7 ms | 31.9 ms (+0.5%) | ~31× |
| `silk_encode_1sec_wb_cbr12k` | 71.1 ms | 71.2 ms (+0.1%) | ~14× |
| `silk_lpc_analysis_20ms_wb` | 321 µs | 262 µs (−18.4%) | |

Stereo costs 1.8× the voiced mono bench, the stereo stage 3% of it. That stage first ran at 118 µs a
frame: each sample went through a dozen helper calls (`silk_SMULWB`, `silk_SAT16` and the rest). Its
loops now do the arithmetic inline, on bounds that make the int16 and int32 casts no-ops where they
are dropped (stated beside each loop, and checked by the stage test on libopus's frames): 2.6× faster,
the stereo encode 5% faster. `silk_lpc_analysis_20ms_wb` creates an encoder state on every run: the
state is now cleared a word at a time instead of by the stdlib's byte-wise `memset`, and
`silk_enc_state_new` went from 78–80 µs to 17 µs, although the reset now clears every buffer (2.12.1
cleared a few). Every other benchmark moved between −0.3% and +1.4%, which is noise.

## [2.12.1] - 2026-11-17

**SILK encode: input at any API rate, in-band FEC, DTX.** This completes libopus's mono `silk_Encode`:
an 8 to 48 kHz input is resampled to the internal rate, voice-active frames can carry a coarser copy (LBRR)
in the next packet, and DTX leaves out packets of silence. shravan's packets are still libopus's byte for
byte (2,537 in the suite). On real speech from a 48 kHz input, libopus decodes them as well as its own
encoder's, within 0.002 in correlation, and it recovers lost packets from shravan's LBRR frames.
15,313 assertions (was 15,115).

### Added

- **Input resampling** (libopus's encoder-side `silk_resampler`): `silk_enc_resampler_new`,
  `silk_enc_resample`.
  - From 8, 12, 16, 24 or 48 kHz to the internal rate, with each pair's delay (`delay_matrix_enc`).
  - Equal rates copy through that delay, which the 2.11.3 packet layer did by hand.
  - Lower internal rates go through a second-order AR filter and a FIR interpolation
    (`silk_resampler_private_down_FIR`) at 3/4, 2/3, 1/2, 1/3, 1/4 or 1/6, in 10 ms batches.
  - Fixed point, exact. The tables come from libopus's `resampler_rom.c` by script.
- **In-band FEC** (LBRR):
  - `silk_lbrr_encode_flp` (libopus `silk_LBRR_encode_FLP`). With FEC on and a frame over 30% speech
    activity, the frame is quantised a second time. It uses the same indices, the first gain raised
    `LBRR_GainIncreases` steps unless an LBRR frame precedes it in the packet, and the gains dequantised
    as the decoder will. The quantiser starts from a copy of the regular coding's state.
  - `silk_setup_LBRR` once a packet: the gain step is 7 after a packet without FEC, otherwise
    7 − 0.2 × loss, at least 3.
  - The packet layer writes the previous packet's LBRR frames after the flags: their flag symbol for 2
    and 3 frames a packet, then each frame's indices (coded conditionally after another LBRR frame) and
    pulses.
  - The LBRR flag in the packet's first bits, and in the LTP scaling's loss term.
  - The bit target subtracts a running average of the LBRR bits. As in libopus, only the first frame of
    a packet sees them: the average is back to zero for the frames after it, which count those bits
    as overshoot.
- **DTX**. After 10 frames without voice activity, up to 20 more at a time, a packet carries nothing:
  `silk_encode` returns 0 bytes, and `silk_encode_opus` returns the TOC alone, even under CBR, as
  `opus_encode` does.
- **Public API**:
  - `silk_encoder_new_api(api_hz, fs_hz, packet_ms, bitrate, complexity, cbr)`. An internal rate above
    the input's codes at the input's, as libopus clamps it. `silk_encoder_new` is the equal-rate case.
  - `silk_encoder_samples` now counts at the input's rate.
  - `silk_encoder_set_fec(e, 0|1)` (libopus `LBRR_coded`) and `silk_encoder_set_dtx(e, 0|1)`.
  - `silk_encoder_set_packet_loss` now also sets the LBRR frames' gain step.
- `_sk_nsq_wrapper`: the quantiser with a given state and pulse buffer.
- **Tools**:
  - `silkref` takes FEC, DTX and an API rate.
  - The interop CLI's `silkenc` takes them too, with `opus` naming the Opus-packet mode.
  - `opusref dec` takes `lose=N` (every Nth packet lost) and `fec=1` (decoded from the next packet's
    in-band FEC).
  - `opusref silk` takes `fec=P`, `dtx=1` and `api=HZ`.
  - Kind 6's gate 3 silences 0.7 s of every second.
- The `silk` interop group: 8 API-rate cases, 5 FEC cases and 5 DTX cases (90, was 72).
- The `silk_encode` fuzz target sets random input rates, FEC and DTX, valid and not.
- Benchmark `silk_encode_1sec_48k_wb_fec20k`.

### Verified

- **`test_silk_enc_packets_vs_libopus`**: 2,537 packets in 48 configurations, byte for byte libopus's
  `silk_Encode`. 1,292 are new, in 19 configurations:
  - every input/internal rate pair, so every FIR ratio and every delay;
  - FEC at every rate and packet length, under 0 to 30% loss (every gain step), with conditionally coded
    LBRR frames and the flags of 2 and 3 frames;
  - frames just over LBRR's speech-activity threshold;
  - DTX leaving packets out, and sending one after 400 ms of them;
  - all three at once.
  - Beyond the suite, 113 of 122 runs of 1 to 3 s matched libopus packet for packet: every rate pair,
    FEC setting and DTX case. Each of the 9 that parted traces to one coefficient rounding ±1 with every
    other input equal: a shaping coefficient (`AR_Q13`) in 7, the low-frequency shaping (`LF_AR_Q14`,
    a near-tie at 8280.5) in 2.
- **`test_silk_enc_api_fec_dtx`**:
  - DTX's Opus packets are the TOC alone, under CBR too, exactly where `silk_encode` is empty, and
    shravan decodes the stream to its length.
  - FEC payloads set their LBRR flag.
  - A 48 kHz input codes wideband and decodes following the input.
  - The API refuses 44.1 and 96 kHz, clamps an internal rate above the input's, and refuses FEC and DTX
    values other than 0 and 1.
- **Real speech** (the Opus test vectors) from a 48 kHz input, libopus decoding, against libopus's own
  encoder: 0.917–0.991 against 0.915–0.991 at 16 kHz, 0.956–0.991 against 0.953–0.991 at 8 kHz
  (12 and 24 kbit/s).
- **FEC in libopus**: with every fifth packet lost and decoded from the next packet's LBRR frames, the
  correlation of 16 kHz speech rose from 0.82 to 0.92 and from 0.71 to 0.92.
  - libopus's own `silk_Encode`, packetised the same way, gives the same within 0.005.
  - libopus's whole encoder recovers about 0.02 better. That is `opus_encode`'s layer around SILK: its
    FEC decision by rate and bandwidth (`decide_fec`) and its input high-pass, for 2.12.x.
- **Interop sweep**, `silk` group, 90 of 90:
  - API rates: correlation within 0.003 of libopus's own encoder.
  - FEC: recovered 0.82–0.95 against 0.78–0.90 concealed.
  - DTX: exactly as many packets left out as libopus's own encoder at every setting.
- **Mutations**: 43 tried, 39 caught.
  - The first run's search found the inputs the suite lacked: three rate pairs' delays, and frames just
    over the speech-activity threshold.
  - Four are equivalent:
    - the resampler's batch size (its state carries across batches) and its final-batch test (whole
      frames never leave one sample);
    - the LBRR average's reset threshold (a packet's LBRR bits are 0 or well over 10);
    - the LBRR term of the bit balance (libopus resets that average to 0 for every frame it applies
      to).
- **Fuzz**: `./fuzz/run.sh 90000`, and again with `--poison`, clean, cold starts included. 24 further
  poisoned seeds were clean too (236,496 calls).
- **Memory**: 3,000 packets allocate nothing past the first hundred: 48 kHz to wideband, 24 kHz to
  mediumband at 60 ms CBR, and 16 kHz to narrowband at 10 ms, all with FEC and DTX on.

### Performance

Pinned to one core, 2.12.0 and 2.12.1 alternating in one session, three rounds each (medians):

| Benchmark | 2.12.0 | 2.12.1 | Real-time multiple |
|-----------|--------|--------|--------------------|
| `silk_encode_1sec_48k_wb_fec20k` (new: 48 kHz voiced input to wideband, FEC on) | | 70.7 ms | ~14× |
| `silk_encode_1sec_wb_voiced20k` | 41.3 ms | 41.4 ms (+0.5%) | ~24× |
| `silk_encode_1sec_wb_vbr20k` | 30.8 ms | 30.9 ms (+0.5%) | ~32× |
| `silk_encode_1sec_wb_cbr12k` | 68.8 ms | 69.0 ms (+0.3%) | ~14× |
| `silk_lpc_analysis_20ms_wb` | 299 µs | 315 µs (+5.5%) | |

The FEC bench quantises every voice-active frame twice and resamples from 48 kHz. `silk_lpc_analysis_20ms_wb`
creates an encoder state on every run, and the state now holds the LBRR frames' indices, pulses and
quantiser state: `silk_enc_state_new` alone went from 59–62 µs to 78–79 µs, the bench's whole difference.
The analysis did not change. Every other benchmark moved between −1.6% and +2.0%, which is noise. All
numbers this session run about 5% above 2.12.0's own session, so compare rows only within one.

## [2.12.0] - 2026-11-17

**SILK encode: the voiced path.** Pitch estimation and long-term prediction arrive, so shravan's SILK
encoder now codes voiced frames, and its packets are libopus's `silk_Encode`'s byte for byte on them
too. On real speech, libopus decodes shravan's packets to within 0.003 of the correlation it reaches on
its own encoder's (2.11.3 was 0.01–0.07 short). 15,115 assertions (was 14,187).

### Added

- **Pitch estimation**: `silk_pitch_analysis_core_flp` (libopus `silk_pitch_analysis_core_FLP`).
  - The pitch residual through int16 to 8 kHz and 4 kHz: `silk_resampler_down2`, and
    `silk_resampler_down2_3` over `_sk_resampler_ar2`. Fixed point, exact.
  - The first stage, at 4 kHz: normalised correlations over lags of 2 to 18 ms, a short-lag bias, and
    the best 4, 6 or 8 (by the estimator's complexity) within a threshold of the best.
  - The second stage, at 8 kHz, over those candidates, each widened by up to two lags: per subframe, the
    lag contour codebooks, biased towards short lags and towards the previous frame's lag. The frame is
    voiced when the best clears the voicing threshold.
  - The third stage, at 12 and 16 kHz: the lag ±2 and every contour at the full rate, under a bias
    towards flat contours.
  - `silk_find_pitch_lags_flp` now makes the voicing decision. Its threshold (`_sk_pitch_threshold`)
    falls with the analysis order, speech activity, a voiced previous frame and the input's tilt.
  - `silk_enc_setup_complexity` sets the estimator's complexity and candidate threshold.
- **Long-term prediction**:
  - `silk_find_ltp_flp` (libopus `silk_find_LTP_FLP`), over `silk_corrmatrix_flp` and
    `silk_corrvector_flp`: per subframe, the 5-tap correlations of the pitch residual with itself one
    lag back, normalised by its energy.
  - `silk_quant_ltp_gains` and `silk_vq_wmat_ec`, fixed point and exact, and `silk_quant_ltp_gains_flp`
    around them. Each of the three codebooks is searched by entropy-constrained matrix-weighted VQ, with
    the running sum of log gains capping each subframe's gain.
  - `silk_ltp_scale_ctrl_flp`: on an independently coded frame, the long-term predictor is scaled down by
    the expected loss.
  - `silk_ltp_analysis_filter_flp`: the input less its long-term prediction, which Burg then sees.
  - `silk_find_pred_coefs_flp`'s voiced branch. It now takes `(st, ctrl, res_pitch, x, cond)`.
- **`silk_hp_variable_cutoff`** (libopus `silk_HP_variable_cutoff`), run every frame in the packet layer.
  It tracks a high-pass cutoff of 60–100 Hz from the pitch of voiced frames, which the Opus layer's VOIP
  high-pass will read (2.12.x).
- **`silk_encoder_set_packet_loss(e, perc)`**: libopus's `packetLossPercentage`, 0 to 100. Without
  in-band FEC it only scales the long-term predictor down.
- **Tools**:
  - `silkref` and the interop CLI's `silkenc` take a packet loss.
  - `scripts/silkref/patch.py` also records the voicing decision, the pitch estimator's stages, the
    long-term predictor's and the high-pass smoother.
  - `scripts/gen-silk-vectors.py` writes the pitch, LTP and high-pass sets, and 13 more packet sets
    (k16–k28). Kind 6, white noise through a comb at a gliding pitch, is as periodic as speech to the
    estimator, but flat within the LPC orders, so libopus's float stages round as shravan's doubles do.
    Kind 7, a period of noise repeated, is periodic from its first sample.
- The interop sweep's `silk` group has a voiced source: eight harmonics of a 110 Hz pitch with vibrato
  (72 cases, was 48).
- The `silk_encode` fuzz target sets random packet losses, valid and not. It also feeds impulse trains
  and squares at lags inside and outside the pitch range.
- Benchmarks `silk_pitch_analysis_20ms_wb`, `silk_ltp_analysis_20ms_wb` and
  `silk_encode_1sec_wb_voiced20k`.

### Changed

- `src/silk_enc_vectors.cyr` stores each field as one zlib stream in base64, which the suite inflates once
  with sankoch. With 2.12's sets in, the file is 634 KB, where 2.11.3's was 647 KB without them. The
  suite's 2 MB of string data would not have held them otherwise.

### Verified

- **Stage by stage, on libopus's own inputs** (`src/silk_enc_vectors.cyr`):
  - **`test_silk_enc_pitch_vs_libopus`**, 55 frames: every rate, 10 and 20 ms, estimator complexity 0 to
    2; voiced after voiced and after unvoiced; both unvoiced exits; lags at both ends of the range;
    contours to the last of 34.
    - Exact: the decimations, the voicing decision, the lag and contour indices, every subframe's lag,
      and the estimator's settings at each encoder complexity.
    - Within 1e-5: LTPCorr.
    - The threshold agrees to 1e-6.
  - **`test_silk_enc_ltp_vs_libopus`**, 26 frames: every periodicity codebook and scaling index, both
    coding modes, all three rates.
    - Within 1e-5: the correlations (2e-7 seen).
    - Exact: the quantiser, ties between codebooks included, and the scaling.
    - Within 1e-6: the LTP residual.
    - On 10 frames of the comb, the whole voiced `find_pred_coefs` from the pitch residual and the
      input: the same LTP, NLSF and interpolation indices, residual energies within 1e-3.
  - **`test_silk_enc_hp_vs_libopus`**: the smoother over 397 frames, exactly, from libopus's initial
    value.
  - Beyond the suite, on libopus's inputs from 2 s runs of a sung vowel and of the comb at every rate,
    frame size and complexity, everything above matched:
    - 2,148 pitch estimates and 4,260 whole voicing decisions from libopus's input;
    - 2,050 voiced frames' LTP stages;
    - 5,397 smoother updates;
    - and, after the fix below, 198 search inputs (66 runs of 11 signals at three complexities).
- **`test_silk_enc_packets_vs_libopus`**: 1,245 packets in 29 configurations, byte for byte libopus's
  `silk_Encode`. 590 of them are new, voiced, in 13 configurations. They cover:
  - every rate, 10 to 60 ms, complexity 0 and 1, VBR and CBR at 8 to 20 kbit/s;
  - lags coded by delta and absolutely in conditionally coded frames;
  - LTP scaling 1 and 2 under 1 to 90% loss;
  - lags at both ends of the range;
  - moves between voiced, unvoiced and silent frames;
  - a first frame that a pitch estimator would find voiced.

  After each set, the high-pass smoother is checked against libopus's.
  - Beyond the suite, 2 s of the comb in 33 further configurations matched packet for packet.
  - Packets part on harmonic input with sharp formants. A sung vowel's shaping analysis is
    ill-conditioned there, and libopus's float window rounds away from shravan's doubles within the
    first frame (an unvoiced one). They also part, as in 2.11.3, where one frame in about 500 rounds a
    shaping coefficient the other way (one such, on the comb at 60 ms and 5% loss, traced to a single
    ±1 in `AR_Q13` with every other input equal).
- **Real speech** (the Opus test vectors), libopus decoding, against libopus's own encoder at the same
  rates, the same delay and the same bits:

  | | shravan | libopus's own encoder | 2.11.3 |
  |-|---------|-----------------------|--------|
  | 8 kHz, 12 and 24 kbit/s | 0.946–0.998 | 0.947–0.997 | 0.926–0.993 |
  | 16 kHz, 12 and 24 kbit/s | 0.914–0.991 | 0.915–0.991 | 0.850–0.979 |
  | 16 kHz, 10 to 60 ms, CBR | 0.947–0.976 | 0.950–0.976 | |

  SNR is within 0.25 dB either way.
- **Interop sweep**, `silk` group, 72 of 72:
  - Voiced source: shravan 0.947–0.997, libopus 0.949–0.997.
  - On it the best lag can land a sample from libopus's (correlations equal to the fourth decimal), so
    that source alone allows one sample. The noise and the tones still pin the delay exactly.
- **Mutations**: 81 tried, 78 caught.
  - 20 survived the first run and were searched for inputs. The search for one of them found a real bug:
    at the estimator's complexity 2 the third stage searched 32 contours, not libopus's 34. Restoring 32
    now fails 6 assertions.
  - The inputs the search found are in the suite: tones at both ends of the lag range, a fast vibrato, a
    gliding pitch, pitch jumps, a weak comb, 1% loss, and periodic noise.
  - Three are equivalent:
    - the 2/3 decimator's final state, which the estimator discards;
    - the third stage's 1e-3 on energies of voiced subframes;
    - the in-band FEC term of the LTP scaling, unreachable until LBRR (2.12.x).
- **Fuzz**: `./fuzz/run.sh 90000`, and again with `--poison`, clean, cold starts included. 24 further
  poisoned seeds were clean too (236,496 calls).
- **Memory**: 3,000 voiced packets allocate nothing past the first hundred: VBR, CBR, 60 ms with loss,
  and 10 ms.

### Performance

Pinned to one core, 2.11.3 and 2.12.0 alternating in one session, three rounds each (medians):

| Benchmark | 2.11.3 | 2.12.0 | Real-time multiple |
|-----------|--------|--------|--------------------|
| `silk_encode_1sec_wb_voiced20k` (new: the comb, voiced on nearly every frame) | | 39.8 ms | ~25× |
| `silk_encode_1sec_wb_vbr20k` (coloured noise) | 27.2 ms | 29.5 ms (+8.7%) | ~34× |
| `silk_encode_1sec_wb_cbr12k` (coloured noise) | 64.0 ms | 66.2 ms (+3.5%) | ~15× |
| `silk_pitch_analysis_20ms_wb` (new) | | 89.5 µs | |
| `silk_ltp_analysis_20ms_wb` (new) | | 83.3 µs | |

The noise benches are slower because every frame with voice activity now runs the pitch estimator, as
libopus's does, even where it then decides unvoiced. Every other benchmark moved between −1.0% and
+1.1%, which is noise.

## [2.11.3] - 2026-11-17

**SILK encode, part 4: shravan encodes SILK.** The last of the four 2.11 steps adds the frame encoder
with its rate loop, the packet layer, and a public API. On every frame libopus codes unvoiced, shravan's
packets are libopus's byte for byte. libopus decodes them to audio that follows the input, at the same
6.5 ms delay as its own encoder. Frames libopus would code voiced are coded unvoiced until pitch
analysis arrives (2.12). 14,187 assertions (was 14,110).

### Added

- **`silk_encode_frame_flp`** in `src/silk_enc.cyr` (libopus `silk_encode_frame_FLP`).
  - The new samples go 5 ms ahead of the frame being coded (the encoder's lookahead), with libopus's
    single-precision anti-denormal signal.
  - Then the stages in order: the pitch analysis residual, noise shaping, the prediction filters, the
    gains.
  - Then the rate loop, over a gain multiplier: quantise and code the frame. If it misses the budget,
    or, under CBR, falls more than 5 bits short of it, scale the gains ×3/2 or ×4/5. Once there is a
    frame on each side, interpolate between them, held to the middle half of the bracket.
  - Each subframe's gain locks once more gain stops reducing its pulses. After two misses Lambda rises
    ×1.5 (to at least 1.5) and the dither goes.
  - After six tries the best frame under the budget comes back, its bytes included. Failing that,
    damage control: the last frame's gains and no pulses.
  - Snapshots of the range coder, the quantiser state, the seed and the index predictors make every
    retry start where the frame did.
- **The packet** (`_silk_encode_packet`; libopus `silk_Encode`, mono, its input at the internal rate):
  - FLOAT2INT16: the sample as a float, ×32768, clamped, NaN to −32768 as libopus's comparisons leave
    it, rounded half to even;
  - the equal-rate resampler's delay (10, 7 or 6 samples) and the mono buffer's one-sample shift;
  - the VAD, and the VAD and LBRR flags reserved in the first bits, then patched in;
  - each frame's bit target: its share of the packet, less a fiftieth of a second's worth of the
    reservoir's excess and of the packet's overshoot so far, held between 5 kbit/s and the bit rate;
  - a multi-frame packet's budget split 3/5 or 2/5 and 3/4 over its frames, CBR on the last only;
  - frames after the first coded conditionally, and the reservoir carried to the next packet.
- **Public API**:
  - `silk_encoder_new(fs_hz, packet_ms, bitrate, complexity, cbr)` takes 8, 12 or 16 kHz (the input rate
    and the coded bandwidth), 10, 20, 40 or 60 ms packets, 1 bit/s to 1 Mbit/s, and complexity 0 or 1.
    Errors: `ERR_INVALID_RATE`, `ERR_ENCODE`, and `ERR_UNSUPPORTED_FMT` for complexity 2 and up, which
    selects libopus's `silk_NSQ_del_dec`.
  - `silk_encoder_samples`: the samples one packet takes.
  - `silk_encode`: one packet's bare SILK payload, in a buffer that also caps a VBR packet. Nothing is
    written past it.
  - `silk_encode_opus` builds one Opus packet as libopus's encoder does for SILK only:
    - the TOC, configurations 0–11 by bandwidth and duration;
    - the bit rate counting the whole packet, so the payload gets a byte a packet less;
    - VBR: trailing zero bytes dropped;
    - CBR: `(bits + 4) / 8` bytes, padded as `opus_packet_pad` does (code 3);
    - libopus's "PLC" frames, which decoders conceal as lost:
      - where there is no room for a frame, the TOC alone, and the encoder does not run. That is under
        3 bytes, under 24 bits a frame, or, for 40 and 60 ms, under 300 bytes or 2.4 kbit/s a second.
      - a payload that busts its budget, sent as a single zero byte.
    - A buffer of 0 bytes is refused; one byte holds the TOC alone.
  - Scratch comes from the Opus arena, reset per packet, so a long encode allocates nothing per packet.
- `src/opus.cyr`: `ec_enc_init` (an encoder in a caller's struct), `ec_enc_patch_initial_bits` and
  `ec_enc_copy`. `src/silk_enc.cyr`: `silk_gains_id`.
- **Tools**:
  - Interop CLI `silkenc <in.f64> <rate> <bitrate> <complexity> <ms> <cbr> <out.pk> [raw [cap]]`.
  - `opusref dec` takes an output rate. `opusref silk` is libopus's own encoder forced to SILK at an
    internal rate, complexity 1: the yardstick.
  - `silkref` takes a VBR packet cap.
  - `scripts/gen-silk-vectors.py` writes libopus's whole packets for 16 configurations. Their inputs are
    integer-only (`int_signal`), so the suite builds the same samples itself, and a hash checks that it
    does. When `build/interop-cli` exists, the generator checks shravan's packets against them.
- Fuzz target `silk_encode` in `fuzz/fuzz_decode.cyr` (exit 89):
  - random parameters, valid and not, and buffers from none to plenty;
  - hostile input: NaN, infinities, overloads, silence, constants, square waves;
  - every payload and packet bounded, with nothing written past its buffer;
  - CBR sizes exact, and shravan's decoder taking the stream.
- Interop sweep group `silk`.
- Benchmarks `silk_encode_1sec_wb_vbr20k` and `silk_encode_1sec_wb_cbr12k`.

### Verified

- **`test_silk_enc_packets_vs_libopus`**: 655 packets in 16 configurations, byte for byte libopus's
  `silk_Encode`. They cover:
  - narrow-, medium- and wideband, at 10, 20, 40 and 60 ms;
  - complexity 0 and 1;
  - VBR, also in 40- and 90-byte buffers, and CBR from 1.2 to 12 kbit/s;
  - voice-inactive frames;
  - every branch of the rate loop: bracketing and interpolation, Lambda raised and floored, gains locked,
    damage control, restoring the best frame, the shares of a multi-frame budget, the 5 kbit/s floor,
    the VBR margin.
- **Beyond the suite**, on 2 to 4 s of noise at every rate, duration, complexity and mode, packets matched
  libopus's everywhere except:
  - frames libopus coded voiced, and a few frames after each, while the quantiser state re-converged;
  - about one frame in 500, where a shaping coefficient rounds the other way in shravan's doubles than in
    libopus's floats (a ±1 in `AR_Q13`), and occasionally a pulse follows. Traced to the coefficient,
    with the state in and every other input equal.
  - On a very sharp resonance (25 dB of prediction gain) Burg is ill-conditioned. libopus rounds its
    gain-scaled input to float, and its NLSFs part from shravan's within a few frames.
- **`test_silk_enc_opus_packets`**: the TOC of each bandwidth and duration; VBR packets end on a nonzero
  byte; CBR packets are exactly their size, code-3 padded when short. shravan's Opus decoder (exact
  against libopus's) turns them back into audio correlating with the input at 6.5 ms.
- **`test_silk_enc_hostile`**:
  - Bad rates, durations, bit rates, complexities and buffers are refused. A 2-byte buffer holds the TOC
    alone.
  - Below 1.2 kbit/s at 20 ms, and below 2.4 kbit/s at 60 ms under CBR, every packet is libopus's
    TOC-only frame (padded to its size under CBR), which shravan decodes as lost frames of the packets'
    length. Against libopus's own encoder the thresholds agree exactly: TOC-only at 2,399 bit/s and a
    frame at 2,400, at 10 and 40 ms, with the same CBR sizes. libopus's TOC names its initial hybrid
    mode when it has not yet coded a packet; shravan's names the SILK configuration.
  - NaN, infinities, full-scale square waves and denormals give packets that fit and decode.
  - NaN codes as −1.0 does, and infinities and overloads as full scale do, packet for packet.
  - A payload never passes its buffer.
- **The roadmap's gate, libopus decoding shravan's packets**, on real speech (the Opus test vectors) at
  12 and 24 kbit/s, 8 and 16 kHz, correlating with the input:

  | | shravan | libopus's own encoder |
  |-|---------|-----------------------|
  | 8 kHz | 0.926–0.993 | 0.947–0.997 |
  | 16 kHz | 0.850–0.979 | 0.915–0.991 |

  Both run at the same rates and the same delay. The gap is the voiced path (LTP), 2.12.
- **Interop sweep** (`scripts/interop.py --only silk`), 48 cases: pink noise and the sweep's tones, at
  every rate and duration, VBR and CBR, judged against libopus's own encoder on the same input. Every case
  has libopus's delay, correlation at least 0.8 and no lower than libopus's by more than 0.03, its bit
  rate within 10% under VBR, and its exact packet size under CBR.
  - Pink noise: shravan 0.82–0.92, libopus 0.74–0.85. libopus's Opus layer high-passes its input at
    3 Hz (`dc_reject`), and pink noise is mostly low frequencies. On it, shravan's payloads are bare
    `silk_Encode`'s, byte for byte.
  - Tones: shravan 0.957–0.990, libopus 0.877–0.976.
- **Mutations**: 43 tried, 40 caught, by 1 to 20 failures each:
  - the packet layer: the input delay, the mono shift, the reservoir, the overshoot term, the budget
    shares, the flags' reservation and patch, conditional coding, CBR on the last frame only, the
    5 kbit/s floor, the seed counter, the input buffer's history;
  - the rate loop: every branch above, the restore and its bytes, the interpolation clamp, both
    multipliers, the gain index reset, the first-frame flag;
  - the Opus packet: the TOC, trailing zeros, CBR padding, the PLC frame, each condition of the TOC-only
    rule, and its CBR padding;
  - FLOAT2INT16's NaN.

  Three were equivalent:
  - the anti-denormal signal, dropped or added in double: it changes no output, since the analysis's
    own regularisation swamps 1e-6;
  - the first slot of the mono buffer's two carried samples, which only stereo reads.
- **Fuzz**: `./fuzz/run.sh 90000`, and again with `--poison`, clean, cold starts included. A hunt over 24
  further poisoned seeds was clean too. That hunt's first run found `silk_encode_opus` refusing a
  286 bit/s CBR packet that libopus sends TOC-only, which led to the rule above.
- **Memory**: 3,000 packets allocate nothing past the first hundred: VBR, CBR, and 60 ms narrowband.

### Performance

| Benchmark | Per second of audio | Real-time multiple |
|-----------|---------------------|--------------------|
| `silk_encode_1sec_wb_vbr20k` (wideband, 20 ms packets, 20 kbit/s VBR) | 27.7 ms | ~36× |
| `silk_encode_1sec_wb_cbr12k` (the same at 12 kbit/s CBR, where the rate loop quantises each frame several times) | 64.8 ms | ~15× |

The existing benchmarks moved by −1.0% to +2.3% against a 2.11.2 build. Both builds ran in one session,
pinned to one core, six rounds each, comparing medians. No existing code path changed beyond the range
encoder's constructor.

### Not yet

- Voiced frames: pitch analysis and LTP (2.12).
- `silk_encode_opus` is a SILK-only packet at an internal rate. libopus's `opus_encode` around it also
  resamples from the API rate, high-passes the input, can add in-band FEC (LBRR) and DTX, codes stereo,
  and decides mode and bandwidth. That is the Opus encoder layer of 2.12.
- Complexity 2 and up (`silk_NSQ_del_dec`) is refused.

## [2.11.2] - 2026-11-17

**SILK encode, part 3: the noise shaping quantiser.** The third of the four 2.11 steps: `silk_NSQ`, which
turns the frame's input, prediction filters, shaping filters and gains into the excitation pulses the
bitstream carries. It is fixed point, with 32-bit wrapping where libopus wraps, and it matches libopus
bit for bit: every pulse, and the quantiser's whole state after every frame, voiced frames included.
The float-to-fixed conversion in front of it matches exactly too. The frame encoder that calls it is
2.11.3. 14,110 assertions (was 13,942).

### Added

- **`silk_nsq`** in `src/silk_enc.cyr` (libopus `silk_NSQ_c`).
  - Per subframe:
    - the input over its gain (Q10);
    - for voiced frames, the LTP history re-whitened through the new prediction filter;
    - every filter state rescaled when the gain changes.
  - Per sample (`_sk_nsq_quantizer`):
    - the short-term prediction, plus long-term for voiced frames;
    - the noise shaping feedback: shaping filter, tilt, low-frequency and harmonic shaping;
    - the residual against two candidate levels by rate and distortion (Lambda), with the dither sign of
      a pseudo-random seed. Above Lambda 2 the candidates are biased down by about Lambda/2, libopus's
      aggressive branch.
    - the decoder's reconstruction tracked, so every prediction stays in step with it.
  - The state (`silk_nsq_new`, `_SKN_`): quantised output, shaping and synthesis histories, dither
    seed, previous gain. `_sk_nsq_hash` hashes it as the reference dump does.
- **`silk_nsq_wrapper_flp`** (libopus `silk_NSQ_wrapper_FLP`). `_sk_nsq_convert` converts the frame's f64
  values to the quantiser's fixed point by `lrintf`: Q13 shaping filters, packed Q14 low-frequency
  shaping, Q14 tilt and harmonic shaping, Q12 prediction filters, Q14 LTP taps, Q16 gains, Q10 Lambda,
  the LTP scale, and the int16 input. Then it runs `silk_nsq`.
- **References**:
  - `scripts/silkref/patch.py` records each `silk_NSQ_c` call's parameters and state hash (and its full
    input state), and the wrapper's float inputs.
  - `scripts/silkref/nsqprobe.c` calls libopus's quantiser directly, on large residuals at Lambda above 2.
    The aggressive branch only changes the result when residuals are big, and no encoder run produces
    those: CBR overload raises Lambda by inflating the gains, which shrinks the residuals.
  - `scripts/gen-silk-vectors.py` writes:
    - two sequences from a fresh encoder (16 wideband and 40 narrowband frames of the vowel);
    - ten calls with Lambda above 2, each with its recorded input state: six from a starved 3 kbit/s CBR
      run, four from the probe;
    - 14 frames of the conversion.
- Benchmark `silk_nsq_20ms_wb`.

### Verified

- **`test_silk_enc_nsq_vs_libopus`**:
  - A fresh state is libopus's.
  - Over the 56 sequence frames (voiced, unvoiced and inactive), and over all 180 frames of three runs
    before they were chosen: every pulse is libopus's, and so is the whole state after every frame.
  - The ten aggressive calls: their recorded states load as libopus's, and give libopus's pulses and
    state.
  - The conversion gives libopus's fixed-point parameters exactly.
- **Mutations.** Fourteen were caught, by 2 to 115 failures each:
  - the level adjustment, the dither sign and the dither generator's increment;
  - the LTP prediction's rounding and the re-whitening start (one sample late);
  - the shaping state's rescale and the harmonic taps' packing;
  - the output's rounding and the low-frequency packing;
  - the aggressive branch: its threshold, the branch dropped, a bias off by 1.5 levels, and the sign of
    its zero region.

  Three were equivalent and changed nothing:
  - Re-whitening one sample *early*: the filtered history is only read from the filter order past its
    start.
  - Moving the aggressive bias by up to a level (−500 for −512, or none): the two candidate levels bracket
    the rate-distortion minimum, at r − Lambda/2, for any bias within a level of Lambda/2 − 512.
- **Unreachable in libopus.** The quantiser's branch for an interpolated first-half filter: libopus runs
  `silk_NSQ` only at complexity 0–1, where interpolation is off, and so does shravan.

### Performance

`silk_nsq_20ms_wb`, a voiced 20 ms wideband frame: 385 µs. The existing benchmarks moved within ±3.9%
(one session, pinned, median of three); no existing code path changed.

## [2.11.1] - 2026-11-17

**SILK encode, part 2: the VAD, noise shaping and gains.** The second of the four 2.11 steps. It
measures what the gain and shaping decisions need, and makes them:
- the VAD's speech activity, spectral tilt and band qualities;
- the LPC residual the pitch analysis starts from, and its prediction gain;
- each subframe's noise shaping filter and gain, with the low-frequency shaping, tilt and harmonic
  shaping;
- the quantised gains, quantiser offset and rate/distortion weight.

The VAD and every decision (quantiser offsets, gain indices) match libopus exactly. The float stages
match it to within libopus's own single-precision error. As in 2.11.0, nothing calls these from a public
API until the frame encoder (2.11.3). 13,942 assertions (was 13,697).

### Added

- In `src/silk_enc.cyr`:
  - **The encoder's parameters.** `silk_enc_setup_complexity` is libopus's complexity table 0–10: analysis
    orders, shaping lookahead, delayed-decision states, NLSF survivors, interpolation, warping.
    `silk_control_snr` gives the target SNR for a bit rate, from libopus's per-bandwidth tables. The
    state carries the pitch, LTP and shaping lengths of `silk_setup_fs`.
  - **The VAD**, fixed point, exact (`silk_vad_new`, `silk_vad_get_sa_q8`, `silk_encode_do_vad`):
    - three halving filter banks (`silk_ana_filt_bank_1`) into four bands, the lowest high-passed;
    - band energies against noise levels that track faster for the first 20 s;
    - from those, the speech activity, tilt and band qualities (`silk_sigm_q15`);
    - the frame's signal type (no voice activity under 5%) and VAD flag.
  - **The pitch analysis front end**, `silk_find_pitch_lags_flp`: a sine-windowed LPC analysis, its
    prediction gain, and the buffer's residual. The voicing decision after it is pitch estimation (2.12),
    so until then a frame with voice activity is unvoiced.
  - **`silk_noise_shape_analysis_flp`**, all of it:
    - coding quality from the SNR;
    - the quantiser offset from the residual's energy fluctuation;
    - per subframe, a windowed LPC analysis (warped at complexity 4 and up, with `warped_gain` and the
      monic conversion), bandwidth-expanded and limited;
    - the low-frequency shaping and tilt;
    - harmonic shaping for voiced frames;
    - smoothing over subframes.
  - **`silk_process_gains_flp`**:
    - the voiced gain reduction and the soft limit against the residual energy;
    - Q16 by truncation, as libopus's cast;
    - `silk_gains_quant`, then the voiced quantiser offset and Lambda.
  - The float helpers they use: Schur, step-up, autocorrelation (plain and warped), the sine window, the
    bandwidth expander, `silk_sigmoid` and `silk_log2`.
- **`scripts/gen-silk-vectors.py`** and `scripts/silkref/patch.py` record the new stages too.
  - A synthetic sung vowel (a vibrato pulse train through three formants, in syllables) gives voiced,
    unvoiced and inactive frames.
  - A 40 ms run gives conditionally coded frames.
  - The vectors are base64-packed 32-bit words, half the size of decimal text: the test harness has 2 MB
    for all its string data.
  - The generator refuses to write unless the sets cover what they test: active and inactive VAD
    frames; every signal type; the warped analysis; independent and conditional gains; 10 and 20 ms;
    narrow and wide band.
- Benchmarks `silk_vad_20ms_wb` and `silk_noise_shape_20ms_wb`.

### Verified

- **`test_silk_enc_vad_vs_libopus`**: 64 frames in two sequences from a fresh encoder (wideband 20 ms,
  narrowband 10 ms of the vowel). Activity, tilt, band qualities and signal type are exact in every
  frame. Before choosing them, all 300 frames of three runs were exact.
- **`test_silk_enc_shape_vs_libopus`**, 18 frames (every signal type, warped, conditional, 10 and
  20 ms, NB and WB):
  - exact: the quantiser offsets, gain indices, carried gain index and the quantised gains;
  - within float error: the qualities, low-frequency shaping, tilt and harmonic shaping (1e-5);
  - the pitch residual and prediction gain (1e-3);
  - the shaping filters and gains (3e-3);
  - the unquantised Q16 gains (2 + 1e-6 of the value), and Lambda (1e-6).
- **Where the float error comes from.** libopus computes the windows, correlations and filters in single
  precision. A formanted vowel's high prediction gain amplifies that most: the shaping filters differ by
  up to 1.5e-3 of their peak on the wideband vowel, against 3e-6 on noise. Rounding just shravan's sine
  window and autocorrelation to float cuts every one of these gaps by 2.5 to 4 times, which is what
  libopus's precision explains and a porting error would not.
- **Ten mutations, each caught** (1 to 64 failures):
  - the VAD's tilt weight, noise-tracking floor and filter-bank coefficient sign;
  - the pitch analysis's bandwidth expansion;
  - the shaping white-noise fraction and the quantiser offset threshold;
  - the voiced low-frequency shaping;
  - the warped gain's sign;
  - Q16 gains rounded instead of truncated (caught by a direct check: the tolerance could not);
  - Lambda's quantiser-offset weight.

### Performance

`silk_vad_20ms_wb` 34 µs and `silk_noise_shape_20ms_wb` 45 µs per 20 ms wideband frame;
`silk_lpc_analysis_20ms_wb` 244 µs (228 µs in 2.11.0: the benchmark now also builds the VAD state and
complexity parameters each iteration). The existing benchmarks: all 24 within ±3% (one session, pinned,
median of three); no existing code path changed.

## [2.11.0] - 2026-11-17

**SILK encode, part 1: the LPC analysis.** The first of the four 2.11 steps toward a SILK encoder that
encodes audio. It finds each frame's prediction filter (Burg's method over the gain-normalised input),
searches whether the frame's first half should interpolate from the previous frame's NLSFs, quantises
the NLSFs, and measures the residual energies the gain stage will need. Each piece is checked on
libopus's own inputs, recorded from an instrumented libopus encoder. The fixed-point stages match libopus
exactly; the float ones run in f64 where libopus runs in single precision, and match it to float
precision. Nothing calls them from a public API yet: the frame encoder is 2.11.3. 13,697 assertions (was
13,324).

### Added

- **`src/silk_enc.cyr`**, a new `[lib]` module: libopus's float SILK encoder (`silk/float/*_FLP.c` over
  the fixed-point `silk/*.c`) ported to f64.
  - `silk_burg_modified_flp`: the prediction filter of stacked subframes, with the prediction gain held
    to a limit; double precision, as libopus computes it.
  - `silk_find_lpc_flp`: Burg over the frame. With interpolation on (complexity 2 and up, 20 ms, not
    the first frame), it also tries interpolation indices 3 down to 0 for the first half and keeps the
    lowest residual energy.
  - `silk_a2nlsf_flp` and `silk_nlsf2a_flp`, over the fixed-point `silk_a2nlsf` and `silk_nlsf2a`.
  - `silk_interpolate`.
  - `silk_process_nlsfs`, fixed point, exact:
    - the NLSF weights from their spacing, the interpolated first half's added;
    - the rate weight `NLSF_mu` falling with speech activity, ×1.5 for 10 ms frames;
    - quantisation through the existing `silk_nlsf_encode`;
    - the Q12 filters for each half.
  - `silk_residual_energy_flp`; and `silk_find_pred_coefs_flp` for unvoiced frames, the stage that runs
    all of the above. Voiced frames need the LTP analysis, which comes with pitch analysis in 2.12.
  - The encoder state and control structs (`silk_enc_state_new`, `silk_enc_ctrl_new`); the side
    information sits in `silk_encode_indices_ec`'s layout.
  - `_sk_lrint`, libopus's `lrintf` (to nearest, ties to even), at every float→fixed boundary.
- **`scripts/gen-silk-vectors.py`** and `scripts/silkref/`, the reference:
  - `patch.py` instruments a libopus v1.6.1 tree with `silkdump.h`'s record writers at the encoder's stage
    boundaries, each edit anchored on libopus's source and failing loudly if it moved;
  - `silkref.c` runs `silk_Encode` (mono, at the internal rate, VAD deciding) over a 16-bit stream.

  The generator builds both, encodes deterministic coloured noise under a moving envelope (WB, MB and
  NB; 10 and 20 ms; complexity 1 and 5), and writes `src/silk_enc_vectors.cyr`: 30 frames chosen by rule
  (the first, each signal type, each interpolation index, the last), floats as their f32 bit patterns.
  It refuses to write unless the frames cover both unvoiced signal types, every interpolation index,
  10 and 20 ms frames, all three internal rates and a first frame.
- Benchmark `silk_lpc_analysis_20ms_wb`.

### Verified

- **`test_silk_enc_lpc_vs_libopus`**, the 30 frames (373 assertions), each stage fed libopus's inputs:

  | Stage | Against libopus |
  |---|---|
  | Burg: coefficients and residual energy, every call | within 1e-6 (measured: 9.2e-8, 5.2e-8) |
  | A2NLSF on libopus's own coefficients | exact |
  | `find_LPC`: NLSFs / interpolation index | within one step / exact |
  | `process_NLSFs`: quantised NLSFs, indices, filters | exact |
  | residual energies | within 1e-6 (measured: 2.1e-7) |
  | the whole `find_pred_coefs` from the raw input: indices / energies | exact / within 1e-6 |

  Over all 200 frames of the two wideband runs, before the 30 were chosen:
  - `process_NLSFs` was exact in 200 and A2NLSF in 186 of 186.
  - `find_LPC` was exact in 199. In the 200th, one NLSF is a step off (13602 against 13603): Burg's f64
    coefficient rounds across a step that libopus's float one does not.
  - The whole `find_pred_coefs` gave libopus's indices in all 200.
- **Seven mutations, each caught** (5 to 70 failures):
  - Burg's conditioning constant;
  - `lrintf` truncating;
  - the interpolation search keeping the worse candidate;
  - the 10 ms `NLSF_mu`;
  - the interpolated weights;
  - the first half's residual through the second half's filter;
  - the first frame's gain limit.

### Performance

`silk_lpc_analysis_20ms_wb`, the unvoiced prediction stage on a 20 ms wideband frame with interpolation
searched: 228 µs. Against 2.10.4 (one session, pinned, median of three), the existing benchmarks moved by
at most ±2.7%; no existing code path changed.

## [2.10.4] - 2026-11-16

**The CELT decoder is 15 times faster.** Both CELT MDCTs ran their N/4-point transform as a direct DFT,
230,400 complex multiply-adds per channel for a 20 ms frame and 95% of a CELT decode. They now run an
FFT. A 20 ms CELT frame decodes in 130 µs (2.10.3: 1.98 ms), and the 5.1 Ogg Opus benchmark in 17.3 ms
(235 ms), about 23× real time where it was 1.6×. The decoded samples are unchanged to within
1.6e-13. 13,324 assertions (was 13,320).

### Changed

- **`_celt_dft`, the transform behind `clt_mdct_backward` and `clt_mdct_forward`.** It splits
  N/4 = 15 × P (P = 4, 8, 16, 32; 60 to 480 points) by Cooley-Tukey:
  - for each of the P columns, a 15-point DFT (`_celt_dft15`): Good-Thomas over 3 × 5, five 3-point
    butterflies and three 5-point ones with no twiddles between them;
  - a twiddle, skipped where it is exactly 1;
  - for each of the 15 rows, a P-point radix-2 FFT whose first two stages are one multiply-free 4-point
    pass (their twiddles are 1 and −j).

  The twiddles come from the MDCT's own cached tables. Nothing is allocated per frame beyond the Opus
  frame arena.
- **The CELT encoder's forward MDCT** runs on the same FFT.
- **De-emphasis** multiplies by 2⁻¹⁵ instead of dividing by 32768 per sample. The two are equal for a
  power of two.
- Benchmark `opus_celt_encode_20ms`.

### Verified

- **Old against new.** A copy of the tree with 2.10.3's direct DFT restored, compared over:
  - every Opus configuration (0–31), mono and stereo;
  - loss, FEC, DTX, and mode, bandwidth, frame-size and channel switches (72 libopus-generated streams);
  - the multistream files.

  The largest sample difference is 1.6e-13 (the FFT sums in another order). Agreement with libopus is
  unchanged: 0.727 LSB at worst, before and after.
- **`test_celt_dft_sizes`** checks the FFT against the direct DFT at all four sizes on a pseudo-random
  input, to 1e-11. Four mutations were caught by it and by 32 to 48 decode assertions each against
  libopus and ffmpeg: −j for +j in the merged stage, two 15-point outputs swapped, every twiddle taken
  as 1, and the radix-2 twiddle step doubled.
- **`scripts/interop.py --only opus,encoders`**: 295 of 295 against a default libopus build, libopus's
  decodes of shravan's CELT encoder included.
- **Fuzzing**: `./fuzz/run.sh 90000` and `--poison`: 97,800 and 90,000 calls and 2,200 cold starts,
  clean; 24 further poison seeds of `fuzz_opus`, clean.

### Performance

One session, pinned to one core, median of three. The same benchmark code was built against 2.10.4
and against a copy with 2.10.3's direct DFT restored:

| Benchmark | 2.10.3 | 2.10.4 | Change |
|---|---|---|---|
| `opus_celt_decode_20ms` | 1.98 ms | 130 µs | −93.4% (15.2×) |
| `opus_celt_encode_20ms` | 2.05 ms | 220 µs | −89.3% (9.3×) |
| `opus_decode_51_multistream` (0.4 s of 5.1) | 235 ms | 17.3 ms | −92.6% (13.6×) |

Code the change did not touch moved by up to ±2.9%.
- **The decode is no longer the transform's.** Of what remains in a 5.1 decode, band decoding (PVQ
  pulses, theta splits, folding) is about 40% and the inverse MDCT about 35%; the postfilter,
  de-emphasis and energy decoding share the rest.
- **Where the gain came from.** The FFT alone took the 5.1 decode from 235 ms to 26 ms. The
  Good-Thomas 15-point kernel took it to 17.6 ms, and the merged first radix-2 stages to 17.0 ms.

## [2.10.3] - 2026-11-16

**Multichannel decode: Opus multistream and AAC past two channels.** Surround and ambisonic Ogg Opus
(RFC 7845 mapping families 1, 2 and 255) and AAC in any channel layout (ADTS or MP4, by channel
configuration or a program_config_element, up to 22.2) now decode. Both were refused with
`ERR_UNSUPPORTED_FMT` since 2.9.3, where they had decoded garbage. Channels come out in ffmpeg's order,
the order ALAC already used. Every layout ffmpeg's encoders write is matched against libopus and ffmpeg.
13,320 assertions (was 13,200).

### Added

- **Opus multistream decode** (RFC 7845 section 5.1.1):
  - **The OpusHead channel mapping table** is read and checked.
    - Families 1 (Vorbis order, at most 8 channels), 2 (ambisonics) and 255 (no defined meaning) decode.
    - Family 3 (it needs a demixing matrix) and the reserved families are `ERR_UNSUPPORTED_FMT`.
    - A table that is short, has no stream, more coupled streams than streams or more than 255 in all,
      or maps a channel past the decoded ones is `ERR_INVALID_HEADER`.
  - **Packets are split into their streams** by RFC 6716 Appendix B's self-delimited framing, as libopus
    splits them (`opus_packet_parse_impl`). Every stream but the last carries its last frame's size, or
    for CBR every frame's.
  - **Each stream has its own decoder**, CELT and SILK state and mode history included:
    - coupled streams decode as stereo, the rest as mono;
    - mapping value 255 is a silent channel;
    - a packet whose streams cannot be split is concealed from that stream on, as libopus refuses it.
  - **Channel order.** Family 1 comes out in ffmpeg's order (5.1 as FL FR FC LFE BL BR). Families 2 and
    255 stay in mapping order, as RFC 7845 and ffmpeg's own Opus decoder leave them.
  - Accessors `opus_head_streams`, `opus_head_coupled` and `opus_head_channel_map`; `opus_head_new` is
    72 bytes.
- **AAC with more than two channels** (SCE, CPE and LFE elements):
  - **Layouts** come from three places:
    - channel configurations 1–7 and 11–14 (configuration 7 is read as 7.1, as ffmpeg reads it by
      default);
    - a program_config_element in an ADTS stream (configuration 0);
    - a PCE in the MP4 AudioSpecificConfig.
  - **Elements are placed as ffmpeg places them** (`ff_aac_get_che`, ported as `_aac_che_slot`):
    - under a PCE, by instance tag;
    - under a configuration, by their order in each block, with ffmpeg's allowances for 5.1 whose LFE
      is coded as an SCE and 4.0 whose back SCE is an LFE;
    - a mono configuration whose stream opens with a CPE decodes as stereo, and a stereo one that opens
      with an SCE as two SCEs.
  - **Channels are ordered as ffmpeg orders them** (`sniff_channel_order`): each position's channels
    are assigned by layer, then sorted by channel; 22.2 takes its fixed order.
  - **Block failures follow ffmpeg:**
    - an element the layout has no place for fails its block;
    - a block with no audio element gives no output;
    - an element missing from a block repeats its channels' last output.

    Coupling channel elements are `ERR_UNSUPPORTED_FMT`.
  - **MP4:** the AudioSpecificConfig's GASpecificConfig is read. Configurations 11–14 and a PCE set the
    channel count and layout. 960-sample frames (`frameLengthFlag`) are `ERR_UNSUPPORTED_FMT` rather
    than decoded at the wrong length.
  - `aac_adts_channel_config(h)`. The ADTS header struct is 48 bytes, and `aac_parse_adts_header` accepts
    configuration 0.
- Benchmarks: `aac_decode_51_adts` and `opus_decode_51_multistream`.

### Changed

- **MP4 AAC access units go straight to the decoder** (`_aac_decode_frames`, the core behind ADTS
  too). 2.10.2 wrapped each one in an ADTS header and copied the track into a staging buffer, which
  limited an access unit to 8,184 bytes and a stream to channel configurations 1–7.
- **AAC decode writes each block into the output in place.** 2.10.2 pushed it sample by sample, so
  stereo decode is 5% faster (see Performance).
- **A block with no audio element** (only fill or END) gives no output, as in ffmpeg. 2.10.2 gave 1024
  samples of overlap, and a mono frame of them even in a stereo stream.
- **An AAC stream whose every block fails** is `ERR_DECODE`, not an empty success.
- `MP4_DX_LAYOUT` (+64) in the demux struct; `MP4_DX_SIZE` is 72.
- The 2.9.3 MP4 audit's two ADTS-bridge assertions now check the bridge's absence:
  - the access unit over 8,184 bytes decodes to its one frame;
  - the stereo stream that claims configuration 7 fails every block, as ffmpeg's do ("channel element
    1.0 is not allocated").

### Verified

- **Fixtures** (`scripts/gen-interop-fixtures.py`). Each channel carries its own tone, so a channel out
  of place shows.
  - AAC from ffmpeg's encoder, with PNS off (its noise is random, and the comparison is sample by
    sample):
    - ADTS 4.0 and 5.1 (configurations 4 and 6), and 7.1(wide) (a PCE in the stream);
    - m4a 7.1 (configuration 12), and 6.1 (a PCE in the AudioSpecificConfig);
    - a stereo stream under a mono header and a mono stream under a stereo one.
  - Ogg Opus from libopus:
    - family 1 at 3.0, 5.1 and 7.1;
    - family 255 at 5.1 (six mono streams) and family 2 at six channels (first-order ambisonics and a
      non-diegetic pair);
    - self-delimited framing codes 0–3, CBR and VBR, across 20, 40, 60 and 120 ms packets.
  - **Every piece was reverted in a scratch copy and its fixtures failed:**
    - no family-1 reorder, and the reorder applied to families 2 and 255 as well;
    - self-delimited packets parsed as plain ones;
    - the CBR size read as the total;
    - no channel sort;
    - PCE positions ignored;
    - configuration 12's back CPE misplaced;
    - the PCE comment not skipped;
    - no LFE slot.
  - `test_aac_element_mapping` checks what no encoder at hand writes against ffmpeg's source: the 5.1 and
    4.0 slips, configurations 11, 13 and 14, and 22.2's channel order.
- **`scripts/interop.py`:**
  - The AAC group's 3- and 6-channel cases (11 rates × ADTS and m4a) are compared with ffmpeg instead of
    expected to fail.
  - Every layout ffmpeg's AAC encoder writes, in both containers: 23 cases.
  - 14 multistream Ogg Opus cases: families 1, 2 and 255, 3 to 8 channels, 20 to 120 ms.
  - The whole sweep: 1,006 of 1,006 against a default libopus build (was 969).
- **The libopus wrapper's channel order.** ffmpeg's libopus wrapper reorders every 3- to 8-channel stream
  as if it were family 1. Its decode of family 2 or 255 is put back in mapping order before comparison.
  ffmpeg's native decoder agrees with shravan's order, though not exactly with libopus's values on
  hybrid. The 4-channel fixtures 2.10.3 started with could not tell (quad's reorder is the identity).
- **Fuzzing.** The new fixtures are `fuzz/fuzz_decode.cyr` seeds: ADTS with and without a PCE, MP4
  through the track API, and multistream Ogg Opus. `./fuzz/run.sh 90000` and `--poison`:
  97,800 and 90,000 calls and 2,200 cold starts, clean; 48 further poison seeds of `fuzz_decode` and 24
  of `fuzz_opus`, clean.

### Performance

One session, pinned to one core, median of three, the same benchmark code built against 2.10.2 and
2.10.3:

| Benchmark | 2.10.2 | 2.10.3 | Change |
|---|---|---|---|
| `aac_decode_1sec_stereo` | 17.2 ms | 16.2 ms | −5.4% |
| `mp4_decode_1sec_stereo` | 17.1 ms | 15.8 ms | −7.4% |
| `aac_decode_51_adts` (0.28 s of 5.1) | refused | 12.4 ms | ~22× real time |
| `opus_decode_51_multistream` (0.4 s of 5.1) | refused | 249 ms | ~1.6× real time |

Code neither release touched moved by up to ±3.7% (MP3 −3.7%; an unused function alone moves
`opus_celt_decode_20ms` by 3.4%).
- **AAC gains from writing in place.** 2.10.2 pushed each sample into the vec. The stdlib's `memcpy` is a
  byte loop, so the first version of the new path, which copied each block, was 4% *slower*.
- **MP4 also skips the ADTS staging copy** it no longer makes.
- **5.1 Opus decodes at only 1.6× real time.** That is four streams (two coupled) at CELT's speed of
  2.1 ms per mono 20 ms frame, about what that predicts. The multistream layer adds little, and surround
  Opus playback has little headroom until the CELT decoder is faster.

## [2.10.2] - 2026-11-16

**Gapless MP4 and MP3, and MP4 track selection.** An AAC file now decodes to its source's length: the
encoder's priming and the last frame's padding are trimmed, as ffmpeg and every player trims them. For
MP4 that comes from the sample table, the edit list or iTunes' `iTunSMPB`; for MP3, from the LAME tag. An
MP4 with several audio tracks decodes the one ffmpeg plays, and every track can be reached by index.
Everything is matched against ffmpeg to the sample and to its length. 13,200 assertions (was 13,119).

### Added

- **Gapless decoding**, on by default. `shravan_set_gapless(on)` returns the previous setting;
  `shravan_gapless()` reads it.
  - **MP4.** A track's decode is first cut to its media length, the sum of its `stts` durations; the last
    frame is usually shorter than a codec frame. Then one of two rules applies:
    - **`iTunSMPB`**, on an AAC track that is the moov's last (where iTunes writes it and ffmpeg reads
      it). The priming is skipped and the remainder dropped. It wins over an edit list, as in ffmpeg.
    - **Otherwise the edit list.** Each edit contributes its media range, in order and to the sample.
      Empty edits (media time −1) and dwell edits (media rate 0) contribute nothing. A leading empty edit
      delays the start, which a PCM decode cannot express, and ffmpeg fills no silence for it either.

    The edits' total is held to the one-shot ceiling (`ERR_TOO_LARGE`), so an edit list repeating its
    media cannot multiply the output.
  - **MP3.** The LAME/Xing tag, when its encoder string starts `LAME`, `Lavf` or `Lavc` (as ffmpeg
    reads it, in `libavformat/mp3dec.c`). Its delay plus 529 samples (the decoder's own lag) are dropped
    from the start, and everything from frames × spf − padding + 529 on, where frames is the tag's
    count. A file well past the size its tag records is a concatenation, so its end is kept, as ffmpeg
    keeps it.
- **MP4 track selection:**
  - `mp4_track_count(data, len)`.
  - `mp4_track_info(data, len, i)`: a FormatInfo with `FMT_AAC` or `FMT_ALAC`, rate, channels, bit
    depth and the gapless length, without decoding.
  - `mp4_decode_track` and `mp4_demux_track`.
  - `mp4_default_track`: ffmpeg's default audio stream (enabled tracks first, then the most channels,
    then the first) among the tracks shravan decodes. `mp4_decode`, `mp4_demux` and `codec_open` take it.
- `scripts/interop/cli.cyr`: `tracks <in>`, `track <in> <i> <out>`, and `raw <in> <out>` (gapless off).
- Benchmarks: `mp4_decode_1sec_stereo` and `mp3_decode_lame_stereo`.

### Fixed

- **MP3: a CRC-protected LAME file decoded its tag frame as audio.** LAME writes the Info tag right after
  the side information even when the header announces a CRC (`lame -p`). shravan looked 2 bytes further
  on, where the CRC would push it, missed the tag, and decoded the frame as 1152 samples of audio in front
  of the stream. It now looks where LAME writes and ffmpeg reads, then after the CRC.
- **`_fx_lossy`** (interop tests) indexed past the end of a short decode after its length check failed,
  and aborted the suite. It now returns.

### Changed

- **MP4 and MP3 decodes are shorter**, by what is not audio:
  - an AAC m4a by its priming (1024 samples from ffmpeg's encoder, 2112 from Apple's) and its last
    frame's padding;
  - an MP3 with a LAME tag by its delay plus 529 and its padding.

  `shravan_set_gapless(0)` gives 2.10.1's output.
- **A multi-track MP4 decodes ffmpeg's default track**, not the first audio track. A file whose first
  audio track shravan cannot decode (HE-AAC, AC-3, AAC past two channels) now decodes the next one it
  can.
- The MP4 demux struct carries its track (`MP4_DX_TRACK`; `MP4_DX_SIZE` is 64).
- The interop references are gapless, with no `-ignore_editlist` and no `-flags2 +skip_manual`:
  - **MP3:** ffmpeg's default decode.
  - **MP4:** the ISO 14496-12 cut of ffmpeg's decode without the edit list (`mp4_iso_ref`): the sample
    table's length, then each edit's media range. ffmpeg trims an MP4's last frame to its edit list only
    from 9.0; 6.1 (Ubuntu 24.04's, CI's) to 8.0 skip the priming but play the padding. On ffmpeg 9 this
    cut equals ffmpeg's own decode in all 22 AAC m4a cases, and it is the same whether the raw decode
    ends trimmed or whole.

  MP4 AAC is now checked to the exact length; before, it passed as "at least as long".
  `scripts/gen-interop-fixtures.py` uses ffmpeg's default decode, and so needs ffmpeg 9.

### Verified

- **Against ffmpeg 9.0.2, to the sample and to its length.**
  - Fixtures, each proven by reverting its piece in a scratch copy and watching only its test fail
    (gapless off fails 38 assertions):
    - `iTunSMPB` with whole-frame sample tables and Apple's 2112 priming, alone and beside an edit list;
    - an m4a without an edit list, where the sample table trims the end;
    - two audio tracks, where the enabled one is neither the first nor the one with more channels, and
      the other track decoded by index;
    - MPEG-2 mono VBR from the `lame` CLI (tag at 9 bytes), MPEG-2 stereo from libmp3lame (17), and
      CRC-protected MPEG-1 (32);
    - the existing AAC 96 kHz m4a and LAME Info fixtures, now at gapless length.
  - Two cases where ffmpeg is not the reference, with references built from ffmpeg's decode of the
    same media:
    - A **leading empty edit.** ffmpeg 9 plays 478 samples of the last frame's padding there; its end
      trim leaves out the empty edit's offset.
    - **Two edits.** ffmpeg trims later edits only to the frame. The reference is its decode without the
      edit list, cut by the ISO rule.
- **`scripts/interop.py`**:
  - The 81 MP3 cases (9 rates × mono, stereo, joint × VBR, 32k, 128k) and the 22 AAC m4a cases are now
    checked at ffmpeg's gapless length.
  - A new `gapless` group: AAC and ALAC files with three audio tracks in six enabled/disabled layouts,
    the default track and each track by index, 48 cases.
  - These groups: 260 of 260. The whole sweep: 969 of 969 against a default libopus build (was 921).
- **Fuzzing**: the new fixtures are seeds of `fuzz/fuzz_decode.cyr`, so mutations reach the `elst`,
  `stts`, `udta` and tag parsers, and a new mode drives the track API (count, default, info, decode by
  index). `./fuzz/run.sh 90000` and `--poison`: 94,760 and 90,000 calls and 2,200 cold starts, clean;
  48 further poison seeds of `fuzz_decode`, clean.
- The AAC decoder's differences from ffmpeg on ffmpeg's own m4a output (up to 7.5 LSB) are PNS noise,
  random by definition. With PNS off the same files match to 0.002 LSB.

### Performance

One session, pinned to one core, best of three, the same benchmark code built against 2.10.1 and 2.10.2:

| Benchmark | 2.10.1 | 2.10.2 | Change |
|---|---|---|---|
| `mp4_decode_1sec_stereo` | 18.1 ms | 17.7 ms | −2.4% |
| `mp3_decode_lame_stereo` (0.3 s) | 20.3 ms | 20.6 ms | +1.1% |

Both are within the run's noise: code neither release touched moved by up to ±3.3%. The track walk and
the trim cost nothing measurable; a single-range trim moves the vec's data pointer and copies nothing.

## [2.10.1] - 2026-11-16

**The AAC encoder, rebuilt.** 2.10.0's encoder coded long windows only, with TNS switched off, PNS and
intensity thresholds set as 44.1 kHz bin numbers, and no working rate control: every request from 32 to
192 kbit/s came out at about 570 kbit/s on real recordings. It is now a 3GPP TS 26.403 encoder, with
block switching, TNS and a rate loop, at all 13 AAC rates. On 5.5 minutes of real recordings its NMR is
lower than ffmpeg's native encoder from 64 kbit/s up, while spending 3–4% fewer bits. From 96 kbit/s up it
loses at most 0.1% of bands (ffmpeg 0.0%). It has less pre-echo than ffmpeg in all 12 transient
cases, and ffmpeg and faad decode it at every rate. The decoder's PNS bands were n/4 too loud, and that
is fixed too. 13,119 assertions (was 13,063).

### Added — the AAC encoder

- **Psychoacoustics** after TS 26.403, as ffmpeg's `aacpsy.c` implements it:
  - a −29 dB threshold spread at 15 dB a Bark upward and 30 downward;
  - the threshold in quiet, capped at 40 dB SPL (uncapped, its f⁴ term removes everything above about
    19 kHz at any level);
  - pre-echo control;
  - perceptual entropy;
  - the bit demand curve.
- **Tonality-adjusted SMR**, in long windows. Each band's signal-to-mask offset runs from 29 dB for a
  tone to 12 dB for noise. Tonality comes from Johnston's spectral flatness on a Hann-windowed
  2048-point FFT, computed as one 1024-point complex FFT; the MDCT's flatness is unreliable in narrow
  bands.
- **Block switching.** An attack detector compares first-difference energy over 128-sample blocks, one
  frame ahead. The window sequence runs `LONG_START` → `EIGHT_SHORT` → `LONG_STOP`, with windows exactly
  mirroring the decoder's synthesis. Short windows are grouped by energy: a new group starts at the attack
  and wherever the energy changes by more than 6 dB.
- **TNS**:
  - from 1275 Hz (2750 in short windows) to `TNS_MAX_BANDS`;
  - order 12 (7 short), gated at a prediction gain over 2;
  - 4-bit coefficients, compressed where they fit.

  The quantiser works on the filtered spectrum, and the decoder's synthesis filter raises that noise
  again by about the prediction gain. So the energies and thresholds of filtered bands move into the
  filtered domain (T′ = T·E′/E), and the rate loop's reduction rule still runs on the psychoacoustic
  values.
- **Rate control.**
  - A search over one threshold scale with TS 26.403's reduction rule, (thr^¼ + r)⁴, and hole
    avoidance, which floors protected bands at −1 dB of their energy and removes the protection band by
    band from the top only past 48 steps.
  - Each step is the exact bit count of the whole frame: scalefactors, sections chosen by dynamic
    programming over the codebooks, and Huffman codewords.
  - A bit reservoir of 6144 bits a channel that **starts full**, as in ffmpeg and FDK. A constant-rate
    decoder buffers that much before its first frame. Starting empty, the bit demand spends 30–70% of the
    average until it has refilled, and the first 30 frames of a 32 kbit/s stream came out nearly empty.
  - The search starts from the previous frame's scale.
- **Scalefactors**: the coarsest one whose noise fits the band's allowance, by bracketing and bisection.
  Every value stays within 8191, and every scalefactor within 60 of the previous coded band's.
- **M/S** per band, where mid and side cost less perceptual entropy than left and right, with both held
  to half the stricter channel's threshold.
- **PNS** at most 24 kbit/s a channel: noise-like bands (tonality under 0.3) above 4 kHz, in long and
  `LONG_STOP` frames without TNS.
  - At 24 kbit/s a channel it cuts bands lost by 10 dB or more from 3.3% to 0.9% (ffmpeg 2.0%), for
    1.3 dB of NMR; ffmpeg's PNS costs 1.4.
  - At 32 kbit/s a channel it would save 0.6% to 0.2% for 2.9 dB, so it stops there.
- **Intensity stereo** at most 48 kbit/s a channel, above 6 kHz (8 kHz at 48), where left and right
  correlate past 0.9.
- **The threshold in quiet, lowered with the rate.** As calibrated (full scale plays at 96 dB SPL) it
  dropped quiet high-band content that ffmpeg codes, and a quiet passage gets turned up. Bark bands lost
  by 10 dB or more came to 1.1% at 48 kbit/s a channel, 1.2% at 64 and 8.6% at 96, all of 16–18 kHz
  there; ffmpeg loses 0.0%. ffmpeg's own floor is negligible: its psy model uses a dB difference as a
  linear energy. From 48 kbit/s a channel the threshold sits 30 dB lower, ramping from 0 at 32. That
  gives 0.1%, 0.0% and 0.0%, for 2.8–5.5 dB of NMR, which stays 1–2 dB under ffmpeg's. 20 dB still lost
  7.8% at 96 kbit/s a channel. A level-adaptive threshold, which lowers it only in quiet frames, did
  worse for the same NMR. At 32 and below, lowering it cost 1.3 dB and saved nothing: holes there come
  from the rate.
- **Bandwidth** from ffmpeg's `AAC_CUTOFF_FROM_BITRATE` at the nominal rate. ffmpeg's twoloop feeds it
  1.15 times the rate; measured here, that cost 0.4–0.5 dB of NMR and up to 3 dB of pre-echo at 16–32
  kbit/s a channel, and changed nothing from 48 up.
- **`aac_encode_bound(frames, channels)`**: the most `aac_encode` writes, every frame at AAC-LC's 6144
  bits a channel.

### Fixed

- **AAC decoder: PNS bands were n/4 too loud**, +6 dB in a 16-line band and +9 in a 32-line one. ISO
  14496-3 4.6.13.3 sets a noise band's total energy to 2^(nrg/2). shravan set its RMS per line instead,
  which agrees only at 4 lines, the width its calibration had used. The interop fixtures had exempted
  PNS ("its noise is random"), which hid it. A new fixture checks the energy: an ffmpeg PNS stream
  framed by silence decodes to a fixed total energy, because the filterbank is orthogonal and every
  decoder normalises each noise band exactly. shravan now matches ffmpeg to 4e-8; the old level read 13%
  high.
- **AAC encoder: the bit rate was ignored** (see above). The new encoder spends 96.1 kbit/s for 96 and
  128.1 for 128. It may run ahead of the rate by one reservoir, never more.
- **AAC encoder: intensity before TNS.** Decoders apply intensity stereo before TNS, so an intensity
  band the left channel's TNS covered copied the left's filtered spectrum. Neither channel may filter an
  intensity band now. Before, only the right channel was checked.
- **AAC encoder input**: NaN is silence, and infinities and values past full scale are full scale. They
  used to reach the transform.

### Changed

- **`aac_encode` writes a priming frame and a flush frame**, as other encoders do. A decode is the input
  delayed by 1024 samples and ends one frame later. Streams from 2.10.0 decode as before.
- Removed internal 2.9.x encoder code: `_aac_psy_*`, `_tns_analyze_apply`, `_aac_write_ics_ext`,
  `_aac_pns_should_use` and their constants.

### Verified

- **NMR against ffmpeg 7.1's native encoder** on 12 real recordings: the Opus RFC 8251 test vectors'
  reference decodes, 48 kHz stereo, 5.5 minutes. The metric is noise-to-mask ratio after Brandenburg:
  half-Bark bands, Schroeder spreading, a tonality-dependent offset and the threshold in quiet, the same
  model for every encoder. "Disturbed" is the share of frames whose worst band exceeds +1.5 dB. Lower is
  better for both. Each cell is NMR · disturbed · kbit/s actually spent:

| Rate | shravan 2.10.1 | ffmpeg | ffmpeg, PNS and intensity off |
|---|---|---|---|
| 32k | −4.57 dB · 75.2% · 32.1 | −4.72 · 72.0% · 34.2 | −5.10 · 74.8% · 34.3 |
| 48k | −5.99 · 59.0% · 48.1 | −6.46 · 49.9% · 51.1 | −7.85 · 36.1% · 51.0 |
| 64k | −9.88 · 26.1% · 64.1 | −7.35 · 44.6% · 67.8 | −8.76 · 24.8% · 66.9 |
| 96k | −12.33 · 6.8% · 96.1 | −10.38 · 23.9% · 99.6 | −11.33 · 9.4% · 99.0 |
| 128k | −16.00 · 2.4% · 128.1 | −14.08 · 9.1% · 132.6 | −14.92 · 2.3% · 132.1 |
| 192k | −22.08 · 0.4% · 192.1 | −20.19 · 1.0% · 198.2 | −21.00 · 0.4% · 197.4 |

  NMR scores substituted noise as error, so the PNS rates (32 and 48) favour encoders that substitute
  less. The model penalises ffmpeg's PNS the same way. Its threshold in quiet is the one the encoder
  lowered from 96 kbit/s, so the bands that brings back score as spent bits: that cost 2.8–5.5 dB
  (−15.16, −19.65 and −27.58 before). 2.10.0 measured −20.84 dB at 572 kbit/s for every rate asked.
- **Bands lost by 10 dB or more** (below the cutoff, above 4 kHz): 0.9%, 0.6%, 0.1%, 0.0% and 0.0% at 48,
  64, 96, 128 and 192 kbit/s. ffmpeg loses 2.0%, 0.4%, 0.0%, 0.0% and 0.0% (0.0% with PNS off too, from
  96 up).
- **Pre-echo** on synthetic castanets, drums and glockenspiel: coding error in the 21 ms before each
  attack, over the attack's energy. Lower is better; ffmpeg ran with PNS and intensity off:

| | castanets | drums | glockenspiel |
|---|---|---|---|
| 64k mono | −31.4 dB (ffmpeg −29.0) | −38.6 (−34.2) | −35.1 (−26.9) |
| 128k mono | −37.8 (−37.7) | −43.9 (−42.7) | −39.9 (−39.4) |
| 64k stereo | −32.4 (−25.9) | −35.3 (−32.8) | −30.1 (−20.9) |
| 128k stereo | −33.8 (−29.8) | −37.7 (−35.0) | −36.1 (−29.2) |

  TNS accounts for 2–4 dB of that. 2.10.0 measured −15 to −17 dB on castanets and −22 to −24 dB on drums,
  at its 570 kbit/s.
- **`scripts/interop.py`**: the AAC encoder cases grow from 6 to 52. They cover 10 rates (8 to 96 kHz),
  mono and stereo, at 32, 64 and 128 kbit/s a channel. Each case must:
  - decode in ffmpeg at exactly the 1024-sample priming;
  - come within 0.02 of the correlation ffmpeg's encoder reaches on the same input, where ffmpeg spends
    no more than 10% over;
  - spend at most the rate asked, over the coded frames less one reservoir;
  - keep every frame within 6144 bits a channel;
  - decode in faad. Below 32 kHz faad assumes implicit SBR and plays the stream at twice the rate, so
    there it must return exactly twice the frames.

  The chirp is held under 6.3 kHz, inside both encoders' bandwidth. `scripts/interop.sh` passes 921 of
  921 against a default libopus build (was 841).
- **In the suite** (13,119 assertions):
  - The PNS energy fixture.
  - A −75 dBFS tone at 14.5 kHz over steeply low-passed noise, at 64 kbit/s mono, must decode within
    3 dB. The calibrated threshold drops it, to −13.1 dB.
  - At the same bit rate, correlated stereo must decode closer to its input than decorrelated (47.7
    against 46.2 dB SNR; with M/S off both are 40.3). The test used to compare stream sizes, which says
    nothing once both streams spend the rate.
  - Rate: at most the bit rate and one reservoir, and within 10% of it.
  - Block switching on clicks (band-limited, so their energy is inside the bandwidth): correlation over
    0.993, where long windows only reach 0.987 and ffmpeg 0.9996.
  - Full-band clicks at 32 kHz, which TNS filters: over 0.995. The thresholds as they were before the
    filtered-domain fix reach 0.987.
  - Each tool engages where it should and nowhere else: M/S on identical channels; PNS on noise at 24
    kbit/s but not on a tone, and not at 32; intensity on correlated stereo but not on decorrelated.
  - Hostile input.

  Each fix was proven by reverting it in a scratch copy and watching its test fail.
- **Fuzzing**: a new `aac_encode` target in `fuzz/fuzz_decode.cyr`.
  - Input: random rates (valid and not), channel counts, bit rates and lengths, over NaN, infinities,
    values past full scale, constants, silence and clicks.
  - Invalid parameters must be refused.
  - Every other stream must fit the bound, hold one frame per 1024 samples plus priming, stay within
    6144 bits a channel and decode in shravan to finite samples.

  `./fuzz/run.sh 90000` and `--poison` are clean: 92,100 and 90,000 calls and 2,200 cold starts. 48
  further poison seeds of `fuzz_decode` are clean too (a new target re-rolls every later target's inputs).

### Performance

One session, pinned to one core, best of three, against 2.10.0 built from its tag's sources:

| Benchmark | 2.10.0 | 2.10.1 | Change |
|---|---|---|---|
| `aac_encode_1sec_stereo` | 51.3 ms | 52.3 ms | +2.0% |
| `aac_decode_1sec_stereo` | 23.0 ms | 17.2 ms | −25.2% |

- **Encode** does far more per frame: tonality, block switching, TNS and a rate loop. It stays close to
  2.10.0 through three changes:
  - The tonality FFT is a 1024-point complex FFT of the real frame, not a 2048-point one, with its window
    tabulated and one logarithm per four lines. Until then it took 46% of encode time.
  - Short frames skip the tonality estimate.
  - The rate search starts from the previous frame's scale, with a per-frame cache: 6.0 quantisations
    of a frame on average, down from 8.8.
- **Decode** is unchanged code. It reads a 128 kbit/s stream now, not 2.10.0's 570 kbit/s one.
- Every other benchmark is within ±1.5%.

## [2.10.0] - 2026-11-16

**A real FLAC encoder.** Until now shravan wrote fixed-prediction FLAC only, 28% larger than `flac`'s
default on real recordings (26.5% of the WAV size against 20.7%). The encoder is rewritten after libFLAC
1.5.0, the encoder behind the `flac` tool, so its nine levels mean what `flac -0` to `flac -8` mean. At
every level shravan's output is within 0.06% of `flac`'s size, or smaller, and libFLAC and ffmpeg decode
it sample for sample. 13,063 assertions (was 12,972).

### Added — the FLAC encoder

- **`flac_encode_level(samples, rate, channels, bits, level, out)`**, levels 0 to 8. `flac_encode` is
  level 5, `flac`'s default. The presets are `flac --help`'s:

| Level | Block | Stereo | Max LPC order | Max partition order | Windows |
|---|---|---|---|---|---|
| 0 | 1152 | independent | fixed only | 3 | |
| 1 | 1152 | estimated | fixed only | 3 | |
| 2 | 1152 | exhaustive | fixed only | 3 | |
| 3 | 4096 | independent | 6 | 4 | tukey(0.5) |
| 4 | 4096 | estimated | 8 | 4 | tukey(0.5) |
| 5 | 4096 | exhaustive | 8 | 5 | tukey(0.5) |
| 6 | 4096 | exhaustive | 8 | 6 | subdivide_tukey(2) |
| 7 | 4096 | exhaustive | 12 | 6 | subdivide_tukey(2) |
| 8 | 4096 | exhaustive | 12 | 6 | subdivide_tukey(3) |

- **LPC**: per analysis window, the windowed autocorrelation, Levinson-Durbin, the order libFLAC's
  estimate prefers, and coefficients quantised with error feedback at libFLAC's default precision. For
  streams up to 16 bits (17 for the side channel) the precision is held where a decoder's prediction fits
  32-bit arithmetic, as libFLAC holds it. A candidate whose residual leaves the 32-bit range RFC 9639
  allows is dropped. `subdivide_tukey(P)` is libFLAC's: the block, its halves, then from depth 3 every
  part and the block without it (the "punchout", from the autocorrelations).
- **Fixed predictors**: the order 0 to 4 with the smallest absolute residual sum.
- **Partitioned Rice**: the partition order is chosen from partition sums, up to the level's maximum,
  and each parameter is refined by exact bit count. Streams over 16 bits may use 5-bit (RICE2) parameters.
- **Stereo decorrelation**: "exhaustive" levels encode left, right, mid and side and keep the cheapest
  pair (independent, left/side, right/side or mid/side). "Estimated" levels pick the pair from
  fixed-predictor residual sums first.
- **CONSTANT** and **VERBATIM** subframes, and **wasted bits** (trailing zero bits every sample shares,
  as in 16-bit audio in a 24-bit container).
- **Seek table**: a point every 10 seconds, at the frame holding it, as `flac` writes by default.
  `flac_decode_range` and `metaflac` read it.
- **`flac_encode_bound(frames, channels, bits)`**: the most a stream can take, every frame verbatim.
- **Incremental MD5**: `flac_md5_new`, `flac_md5_update` and `flac_md5_final`. The encoder hashes each
  frame as it writes it, and `flac_md5_compute` (used by `flac_verify`) no longer copies its input.
- **`scripts/interop/cli.cyr encode ... flac`** takes an optional level.

### Fixed

- **AAC: a heap overrun on a truncated escape** (present in 2.9.4). When the escape of a codebook-11
  pair was cut off, the decoder set its position to the band's end but stored the pair anyway, at the
  band's end and one past it. In the band that ends the frame that is coefficients 1024 and 1025, 16
  bytes past the spectrum, written into whatever the frame arena placed next. A failed escape now ends
  the band and stores nothing. The poison-mode fuzz gate found it: the new encoder target shifted the
  random inputs the AAC target sees, and a mutated ADTS stream hit it. `test_aac_esc_truncated_at_frame_end`
  builds the case bit by bit, and it fails both of its assertions with the old decoder.
- **FLAC: a heap overrun on an impossible partition order** (present in 2.9.4). A residual's first
  partition holds `(block >> order) - predictor order` values. When that was negative the decoder
  skipped the partition but still read the other `2^order - 1`, so up to `(2^order - 1) * (block >>
  order)` residuals went into a buffer of `block - predictor order`. In a mutated `flac -8` file, a
  partition order of 9 behind an order-12 predictor wrote 4,088 residuals into 4,084. The decoder now
  refuses a partition order whose first partition is smaller than the predictor, or which does not
  divide the block, as libFLAC does. The poison gate found it the same way, once the AAC fix let the
  batch run on. `test_flac_residual_partition_bounds` builds that residual section bit by bit; with the
  old reader it decodes "successfully" and writes past the buffer.
- **Memory.** The 2.9.x encoder allocated fresh buffers in every frame against a bump allocator that
  never frees: 113 MB to encode 29.5 s of stereo, about 2.3 GB for ten minutes. The new one keeps fixed
  scratch sized to one block and works frame by frame: 0.95 MB, whatever the length.
- **Streams shorter than one block** recorded their length as STREAMINFO's block size, so a stream of
  fewer than 16 samples said its maximum block was under 16. RFC 9639 requires at least 16, and ffmpeg
  refused those streams. STREAMINFO now records the configured block size, as libFLAC does.
- **Arguments FLAC cannot represent**: more than 8 channels, fewer than 4 bits a sample (libFLAC's
  minimum) or a sample rate past STREAMINFO's 20 bits produced a malformed stream. They are now refused
  (`ERR_INVALID_CHANNELS`, `ERR_ENCODE`, `ERR_INVALID_RATE`).

### Changed

- `flac_encode` writes different, smaller streams. Streams from 2.9.x decode as before.
- `flac_encode_frame` keeps its signature and encodes with level 5's analysis. The 2.9.x fixed-prediction
  helpers (`flac_encode_subframe`, `flac_best_fixed_order` and the rest) stay public and unchanged, unused
  by the encoder.

### Verified

- **Size against `flac` 1.5.0, level for level**, on 12 real recordings: the reference decodes of the
  Opus RFC 8251 test vectors, speech and music, 48 kHz stereo 16-bit, 5.5 minutes, 61.6 MB as WAV.
  `flac` ran with `--no-padding --no-seektable`; shravan's files include their seek table:

| Level | `flac` (% of WAV) | shravan (% of WAV) | Difference |
|---|---|---|---|
| 0 | 30.32 | 30.33 | +0.02% |
| 1 | 23.92 | 23.75 | −0.71% |
| 2 | 23.65 | 23.66 | +0.01% |
| 3 | 27.34 | 27.33 | −0.05% |
| 4 | 21.00 | 20.97 | −0.16% |
| 5 | 20.66 | 20.66 | −0.03% |
| 6 | 20.55 | 20.55 | −0.04% |
| 7 | 20.26 | 20.25 | −0.03% |
| 8 | 20.20 | 20.18 | −0.06% |

  2.9.4's encoder wrote 26.51% on the same files. Every one of the 108 files passes `flac -t` and
  decodes through ffmpeg to exactly its input. Level 1 is smaller than `flac -1` because its estimate
  weighs all four channel pairs with the best fixed predictor's residual sums. libFLAC 1.5.0's loose
  mode weighs only independent against mid/side, from first differences.
- **Hostile and edge-case input**: 1,125 cases, 125 a level at all nine levels. They cover 4 to 32 bits,
  1 to 8 channels and rates from 1 Hz to 1,048,575 Hz, with 0 to 8,193 samples. The signals include
  full-scale noise, square waves, alternating extremes at 32 bits (every predictor's residual leaves
  the 32-bit range, so verbatim must take over), DC, silence, wasted-bit containers and ramps.
  `flac -t` passes every case, `flac -d` returns every sample at whole-byte depths, and ffmpeg returns
  every sample at every depth. NaN, ±infinity and values past full scale are clamped, and the output
  passes `flac -t`.
- **In the suite** (13,063 assertions, was 12,972):
  - All nine levels decode exactly and carry a matching signature.
  - Edge cases: short, empty, 32-bit extremes, wasted bits, constant, 8 channels at 20 bits, the bound
    on incompressible input, MD5 in pieces.
  - The seek table and a range decode through it.
  - `flac`'s own `-8` fixtures re-encoded at level 8: within 0.5% of their frame bytes (measured
    −0.06%, 0.00%, −0.02%).
  - Each feature proven by mutation in a scratch copy, with an unmodified control passing all
    assertions. Each mutation fails its own test: no LPC, no wasted bits, independent stereo only,
    Rice partition order 0 only, seek points at the target sample, the old STREAMINFO block size, an
    MD5 that skips a byte. Both decoder fixes were proven by reverting them.
- **`scripts/interop.sh`**: 841 of 841 against this machine's deep-PLC libopus (was 816). The new cases
  are each level against `flac -L` on the sweep's signal (shravan's frames 0.00% to 0.13% smaller), 12,
  20 and 32 bits, 3 and 8 channels, and the seek table through `metaflac`. After the two decoder fixes
  the FLAC, AAC, MP4, ALAC and encoder groups ran again: 394 of 394.
- **Fuzzing**:
  - `fuzz_decode` gains the FLAC encoder: random lengths, levels, depths, channel counts and rates over
    hostile f64, decoded by shravan and compared with the quantised input.
  - `./fuzz/run.sh 90000`: 990,600 calls (fuzz_codecs 810,000, fuzz_decode 90,600, fuzz_opus 90,000) and
    2,200 cold starts, 0 crashes. The same with `--poison`, 0 redzone violations, after the two fixes
    above. Before them it found both.
  - 40 further poison batches (seeds 100 to 139): none.
- **Other gates**: `scripts/check-collisions.sh` finds no collisions. `cyrius distlib --check` passes with
  the same single platform warning (`is_err` on x86_64-windows). `cyrius vet`: 0 untrusted, 0 missing.
  Write lengths are correct, and every changed file is canonically formatted.

### Performance

`src/bench.cyr`, 2.9.4 against 2.10.0 in one session, alternating, pinned to one core, best of three.
The stereo rows are new; their 2.9.4 column is 2.9.4's `flac_encode` on the same signal:

| Benchmark | 2.9.4 | 2.10.0 | Change |
|---|---|---|---|
| flac_encode_1sec_i16 (level 5) | 10.84 ms | 5.63 ms | −48.1% |
| flac_encode_stereo_l0 (new) | 22.85 ms | 8.38 ms | −63.3% |
| flac_encode_stereo_l5 (new) | 22.95 ms | 17.70 ms | −22.8% |
| flac_encode_stereo_l8 (new) | 22.88 ms | 56.26 ms | +145.9% |
| flac_decode_1sec_i16 | 3.30 ms | 3.19 ms | −3.2% |
| flac_stream_1sec_i16 | 3.84 ms | 3.74 ms | −2.6% |
| aac_decode_1sec_stereo | 22.43 ms | 22.84 ms | +1.8% |

Levels 0 and 5 are faster than 2.9.4's encoder and write about 22% less. Level 8 does more analysis
and takes 2.5 times as long, as `flac -8` does against `flac -0`. The rest of the table is within noise.
On a 29.5 s stereo recording level 5 encodes in 0.65 s (45 times real time), level 8 in 1.9 s and level
0 in 0.31 s. `bench-history.csv` gains the rows stamped `2026-11-17T00:39:25Z`. They were recorded
before commit, so they carry 2.9.4's hash (001921b).

### Also

- The resolved `_fft_pow2` issue moved to `docs/development/issues/archived/`.

## [2.9.4] - 2026-11-16

**Namespace hygiene.** One issue, filed from dhvani: shravan's private `_fft_pow2` had abaco 2.8.0's
name and arity, so whichever bundle a consumer included second replaced the other's for every caller in
both. The roadmap scheduled nothing else for 2.9.4. shravan had no check for this class of bug, so this
release adds one, and on its first run it found a second clash, with the stdlib's mabda. No codec
output changes. 12,972 assertions (was 12,960).

### Fixed

- **`_fft_pow2` renamed `_shr_fft_pow2`**
  (`docs/development/issues/archived/2026-11-08-fft-pow2-clashes-with-abaco-2-8-0.md`). The two differed at
  `n = 1`: abaco counts 1 as a power of two and shravan does not. With abaco included first, the order
  dhvani uses, shravan's copy won, and abaco's `fft_twiddles(t, 1)` and `fft(.., 1, t, 1)` returned
  Err. abaco is the ecosystem's math library and keeps the name. Measured with the issue's repro: both
  calls now return Ok beside shravan, as with abaco alone, in either include order, and shravan's
  `fft_compute` at size 1 still returns 0 with the value unchanged. A private helper, so no API change.
- **`F64_255` deleted.** The pinned stdlib's mabda (4.2.0) declares `const F64_255 = 255.0`, so
  shravan's `var F64_255` stopped every unit that included `lib/mabda.cyr` before shravan from
  compiling (`'F64_255' is already declared as a top-level const`). shravan never read it. phylax
  deleted its copy for the same reason at 1.3.0. It was a public global, but no consumer reads it
  (tarang, jalwa, shruti, dhvani and nidhi checked).

### Added

- **`scripts/check-collisions.sh`** and a CI step. It compiles `dist/shravan.cyr` last, the order that
  did the harm, after (1) its own deps: the declared stdlib, sankoch and bayan-json, (2) every module of
  the pinned toolchain's stdlib snapshot (81 at 6.7.18), and (3) abaco at its newest release tag,
  fetched from GitHub. It fails on any duplicate or redeclaration located in shravan's bundle. cycc
  merges same-name globals that have the same value without a word, and they are harmless, so the check
  does not see them. The one accepted collision is the `detect_format` wrapper, which is public API
  until 3.0 (roadmap backlog); neither sankoch nor any consumer calls `detect_format`. Other bundles,
  as files or `REPO[@TAG]`, can be named on the command line. On the 2.9.3 bundle it fails on both
  clashes above, and on 2.9.4's it passes, as it does against abaco 2.7.0.
- **`test_fft_size_1`**: the helper's contract (`_shr_fft_pow2(1) == 0`, so size 1 takes the mixed-radix
  path) and the size-1 transform in both directions. Given abaco's semantics (`n < 1`), the suite fails
  exactly that assertion; the size-1 values still agree, so both paths give the same answer.

### Verified

- 12,972 assertions, 0 failing. `./fuzz/run.sh 90000`: 998,846 calls (fuzz_codecs 810,000,
  fuzz_decode 98,846, fuzz_opus 90,000) and 2,200 cold starts, 0 crashes. The same gate with `--poison`:
  the same counts, 0 redzone violations. `scripts/check-collisions.sh`: no collisions (deps, 81 stdlib
  modules, abaco 2.8.0). `cyrius distlib` regenerates `dist/shravan.cyr` and `--check` passes, with the
  same single platform warning as 2.9.3 (`is_err` on x86_64-windows). `cyrius vet`: 0 untrusted,
  0 missing. Write lengths correct; every changed source file canonically formatted.
- The issue's repro, built against the 2.9.4 bundle, prints 0 and 0 (it printed 1 and 1 against 2.9.3's).
- `src/bench.cyr`, 2.9.3 against 2.9.4 in one session, alternating, pinned to one core, best of three.
  Nothing moved beyond noise (other work was loading the machine), as expected of a rename and a
  deleted global:

| Benchmark | 2.9.3 | 2.9.4 | Change |
|---|---|---|---|
| wav_decode_1sec_i16 | 180.9 µs | 180.5 µs | −0.2% |
| wav_encode_1sec_i16 | 871.7 µs | 862.5 µs | −1.1% |
| wav_stream_1sec_i16 | 514.9 µs | 490.0 µs | −4.8% |
| pcm_i16_to_f64_4096 | 17.4 µs | 17.0 µs | −2.3% |
| pcm_i24_packed_4096 | 19.9 µs | 19.6 µs | −1.5% |
| pcm_u8_to_f64_4096 | 11.8 µs | 11.7 µs | −0.2% |
| pcm_f32_to_f64_4096 | 43.2 µs | 42.8 µs | −0.9% |
| fft_forward_1024 | 101.5 µs | 99.5 µs | −1.9% |
| mdct_forward_2048 | 58.9 µs | 57.2 µs | −2.9% |
| imdct_inverse_2048 | 58.2 µs | 57.5 µs | −1.4% |
| flac_encode_1sec_i16 | 11.68 ms | 11.54 ms | −1.3% |
| flac_decode_1sec_i16 | 3.53 ms | 3.50 ms | −0.7% |
| flac_stream_1sec_i16 | 4.09 ms | 4.11 ms | +0.5% |
| aac_encode_1sec_stereo | 53.53 ms | 54.15 ms | +1.2% |
| aac_decode_1sec_stereo | 24.09 ms | 24.11 ms | +0.1% |
| opus_celt_decode_20ms | 2.26 ms | 2.31 ms | +2.0% |

  `bench-history.csv` gains the rows stamped `2026-11-16T22:52:17Z`, the same pinned build run once.
  They were recorded before commit, so they carry 2.9.3's hash (cd577bb).

## [2.9.3] - 2026-11-07

**The P(-1) sweep.** No new codecs. The 2.9.2 comparison rig became `scripts/interop.sh`, a CI job that
sweeps every format across rate × depth × channels against ffmpeg, flac, mpg123 and libopus, and it was
pointed at every path no reference had reached. It found defects in Opus, MP3, AIFF-C, FLAC, the
streaming decoders, the PCM encoders and ID3v2; fuzzing found one crash; an independent audit found two
memory amplifications in MP4. Every fix below was proven by reverting it in a scratch
copy and watching a test fail, with an unmodified control passing. 12,960 assertions (was 11,938).

### Fixed — wrong output, found against a reference

| Area | What was wrong | Effect | Now |
|---|---|---|---|
| **Opus packet loss and DTX** | No concealment: lost packets and DTX frames were not concealed as libopus conceals them | Output diverged from libopus at every loss and every DTX frame | libopus's CELT PLC (pitch search, LPC extrapolation, fold) and SILK PLC + comfort noise ported. Lost packets and DTX match libopus within 3 LSB (its float32 against shravan's f64) |
| **Opus SILK FEC** | LBRR (in-band FEC) frames were never skipped | Every regular frame after one decoded from the wrong bit position | Skipped as libopus does |
| **Opus transitions** | Mode, bandwidth, frame-size and mono/stereo switches: CELT↔SILK redundancy, decoder resets, the transition PLC cross-fade and the hybrid→SILK fade were missing or wrong | Errors at every switch | Exact against libopus over streams that switch every packet |
| **Opus stereo SILK** | Mid-only frames and the side channel's restart | Errors whenever the encoder dropped the side channel | Exact |
| **Opus CELT 2.5 ms** | The second postfilter comb ran at LM 0 | Wrong output for every 2.5 ms CELT frame with the postfilter on | Exact |
| **Ogg Opus output gain** | OpusHead's output gain was ignored; RFC 7845 says a decoder MUST apply it | Wrong level whenever a file sets it | Applied, as ffmpeg does |
| **Opus multistream** | Channel mapping family 1/255 (more than two channels) was decoded as one stream | Garbage | `ERR_UNSUPPORTED_FMT`. Multistream decode is scheduled for 2.10.3 |
| **MP3 intensity stereo** | MPEG-2 LSF intensity: the scalefactor partition, illegal positions and the top-band rule were wrong, and the short-block reorder stopped before intensity-coded bands | Wrong stereo image on LSF intensity files | ISO 11172-4 / 13818-4 conformance vectors (including l3-test46, l3-he_mode) within 1 LSB |
| **MP3 Layer I/II joint stereo** | Joint-stereo subbands read the wrong allocation | Wrong output above the stereo bound | Conformance vectors l1-fl1 and l2-fl10 within 1 LSB |
| **MP3 free format** | Bitrate index 0 was rejected | Free-format files did not decode | Frame length learnt from the first two frames; l3-he_free within 1 LSB |
| **MP3 Info/Xing/VBRI frame** | Decoded as audio | About 1152 samples of silence at the start of nearly every LAME file | Skipped |
| **MP3 mixed mono/stereo** | The channel count came from the last frame | Interleaving broke when a stream changed mode | One layout for the whole stream, mono frames duplicated |
| **MP3 detection** | A stream behind leading junk was not recognised | `codec_open` refused it | Found by two matching headers within 64 KiB |
| **AIFF-C** | fl32 was decoded as integers and fl64 refused; ulaw, alaw and ima4 were read as PCM | Noise, or no audio | Float decoded; unsupported compressions `ERR_UNSUPPORTED_FMT` |
| **12-bit WAV and AIFF** | Depths other than 8/16/24/32 had no decoder | WAV returned `ERR_DECODE`; AIFF returned zero samples as success | Decoded in their whole-byte containers, as ffmpeg does |
| **FLAC 32-bit** | Sample-size code 7 (RFC 9639) refused | 32-bit files did not decode | Exact against `flac` |
| **FLAC range decode** | A seek landing in the last, shorter frame placed it with the frame's own block size | `flac_decode_range` returned nothing past that point | Placed by STREAMINFO's block size |
| **FLAC channel count** | A frame whose channels contradicted STREAMINFO was decoded anyway | Output interleaved for the wrong channel count | Refused, as libFLAC refuses it |
| **Streaming decoders** | WAV and AIFF decoded the first feed holding a complete header and ignored every later byte; FLAC re-decoded from its last seek point on each feed; `decode_reader` dropped a first feed shorter than 12 bytes. The tests only looked for a SAMPLES event | A WAV stream returned 461 of 44,100 samples; a minute of FLAC took 112 s to stream | Incremental: sample-identical to the one-shot decoders however the input is split, buffer bounded to one chunk or frame, 60 s of FLAC in 0.5 s |
| **f32 encode** | f64 → f32 truncated the fraction and flushed subnormals | Every sample not already a float pulled toward zero by up to one f32 step | Rounds to nearest even; equals ffmpeg's conversion exactly |
| **ID3v2.3 frame flags** | Never read: compressed, encrypted and grouped frames were taken as text | Compressed and encrypted frames came back as garbage text, grouped ones not at all | Compressed frames inflated (sankoch), encrypted skipped, group byte skipped |
| **ID3v2.4 compressed frames** | Skipped | Missing tags | Inflated (sankoch), unsynchronisation undone first |

### Fixed — crashes and hostile input

- **Opus SILK first packet with LBRR** (found by the new fuzzing): a fresh decoder read the LBRR iCDF before
  any table existed, a null pointer. libopus never writes such a first packet, so real streams did not hit
  it; a warm process hid it. Fixed, with a regression test that clears the tables first.
- **Lazy initialisation everywhere.** Consumers call only `alloc_init` and `shravan_init_constants`, but the
  FFT, Ogg CRC, resampler, dither, AAC rate table and FLAC CRC/MD5 tables relied on init calls the test
  harnesses made. `fuzz_decode` now calls only what consumers call.
- **MP4, from an independent bounds audit of ALAC and MP4** (the roadmap's SEC re-audit item: no
  out-of-bounds access or crash found, in code review and about 230,000 targeted fuzz runs):
  - SEC-033's fix was incomplete. A constant `stsz` size of 1 still allowed as many samples as bytes, so
    the two sample tables cost 16× the file: 142 MiB resident from 8 MiB. The bound now counts at least
    4 bytes a sample, the same 4× that an explicit table's entries already imposed.
  - Chunk offsets could point many AAC samples at the same bytes, each in bounds, and the ADTS staging
    copy was the sum of their sizes: 520 MiB from a 270 KB file. Sample sizes must now sum to at most
    the file (a real file's samples never overlap), checked before anything is staged.
  - The audio-track search ignored 64-bit box sizes and never found a `trak` that used one; ffmpeg does.
  - The per-AU ADTS bridge wrote channel configuration 7 as 0, wrapped AUs over 8184 bytes, and gave
    rates outside the ADTS table 44.1 kHz. Those tracks are now refused (ffmpeg's decoder refuses the
    explicit rate too).
  - A QuickTime v2 sample entry's f64 rate could reach `mp4_demux` negative; it now reads as unknown.
  - Also: sample counts larger than the box could hold are refused, the size and offset tables are
    allocated with overflow checks, and a table past the one-shot ceiling returns `ERR_TOO_LARGE`.
- **Streams**: a header declaring 65,535 channels can no longer make a stream buffer 2 GB for one chunk
  (chunks are held to 16 MiB), and input that never completes a header or frame is refused at a bound.
- **Compressed ID3v2 frames** are held to 256 KiB each and 1 MiB per tag: zlib expands about 1000:1.
- `fuzz_codecs` did not build since the one-shot ceiling landed (its stub lacked the new symbols).

### Changed — decisions

- **One-shot ceiling: settable, and presized.** `shravan_set_max_samples(n)` (returns the previous value)
  and `shravan_max_samples()`. The default doubles to 2^26 interleaved samples, about 12.7 minutes of
  44.1 kHz stereo. Past it every one-shot decoder returns the new **`ERR_TOO_LARGE`** (8) before
  allocating; until now a 7-minute FLAC returned `ERR_DECODE`. Decoders size their output once from the
  declared length, so peak memory is the payload, not two to four times it. Streams have no ceiling.
- **PCM encoders are the decoders' inverse.** WAV, AIFF and FLAC encoding scale by 2^(N-1), round half to
  even and clamp, where they scaled by 2^(N-1) - 1 and truncated. A decode then re-encode now hands every
  sample back; before, it moved nearly every sample by one LSB. -1.0 encodes to the minimum integer, +1.0
  clamps to the maximum.
- **sankoch has a call site**: compressed ID3v2 frames (above). Kept as a dependency on that basis.
- **One test harness.** `src/main_encoder.cyr` and `src/main_silk.cyr` are gone; `src/main.cyr` runs
  everything. The split existed for a ~3.15 MB code buffer that has been a growable 64 MiB region since
  Cyrius 6.4.49. CI checks the single summary line.
- **Stream events gain `STREAM_EVT_ERROR`** (3), carrying a packed error, after which the stream decodes
  nothing further. `decode_reader` refuses formats it cannot stream with one ERROR event instead of
  returning empty event lists forever, and its flush always ends with END.
- **AAC with more than two channels** is refused with `ERR_UNSUPPORTED_FMT`; decode is scheduled for
  2.10.3 with Opus multistream.

### Performance

| Path | Before | After |
|---|---|---|
| AAC decode, 30 s stereo, transients | 24.4 s (1.2× real time) | 0.47 s (63×) |
| AAC encode, 10 s stereo | 5.36 s | 0.49 s, identical bitstream |
| f32 → f64, 4M samples | 874 ms | 36 ms |
| i16 / i24 → f64, 4M samples | 77 / 78 ms | 21 / 28 ms |
| FLAC decode, 60 s stereo | 0.44 s | 0.39 s |
| FLAC stream, 60 s stereo | 112 s | 0.50 s |

"Before" is 2.9.2 for the PCM and stream rows. For the AAC and FLAC decode rows it is this release's tree
with that optimisation reverted, measured in the same session; 2.9.2 itself was slower still on AAC
transients, since it rebuilt the long-block cosine basis at every block switch.

- **MDCT and IMDCT through an N/4-point FFT**, on a radix-2 FFT whose twiddles and bit-reversal tables are
  cached per size. The O(N²) direct transforms remain as the fallback for other sizes and as the tests'
  reference; the fast ones agree to 1e-9 on every size AAC uses. AAC output moves by at most 2e-9 LSB.
- **AAC dequantisation tables**: |q|^(4/3) and 2^((sf-100)/4) from tables built with the same
  expressions, bit-identical to the `f64_pow` they replace.
- **PCM converters** share one layout loop that writes the output buffer directly; f32 decodes by moving
  exponent and fraction bits instead of an `f64_pow` per sample.
- **Memory**: per-frame scratch in FLAC (a buffer ring), Opus and AAC (frame arenas), so a one-shot decode
  allocates about its own output: FLAC was 3× it, AAC 8×, Opus 8×.
- `src/bench.cyr`, 2.9.2 against 2.9.3 in one session, alternating, pinned to one core, best of three
  (2.9.2's PCM rows measured its inline copies, which is what this release removed):

| Benchmark | 2.9.2 | 2.9.3 | Change |
|---|---|---|---|
| wav_decode_1sec_i16 | 884 µs | 170 µs | −80.8% |
| wav_encode_1sec_i16 | 808 µs | 812 µs | +0.5% |
| pcm_i16_to_f64_4096 | 83.1 µs | 16.0 µs | −80.7% |
| pcm_i24_packed_4096 | 78.5 µs | 18.3 µs | −76.7% |
| pcm_u8_to_f64_4096 | 51.0 µs | 11.0 µs | −78.5% |
| fft_forward_1024 | 1.26 ms | 94.2 µs | −92.5% |
| mdct_forward_2048 | 5.82 ms | 54.2 µs | −99.1% |
| flac_encode_1sec_i16 | 10.41 ms | 10.81 ms | +3.9% |
| flac_decode_1sec_i16 | 3.70 ms | 3.25 ms | −12.2% |
| aac_encode_1sec_stereo | 571 ms | 50.1 ms | −91.2% |
| aac_decode_1sec_stereo | 904 ms | 22.3 ms | −97.5% |
| opus_celt_decode_20ms | 2.09 ms | 2.10 ms | +0.3% |
| wav_stream_1sec_i16 (new) | — | 459 µs | |
| pcm_f32_to_f64_4096 (new) | — | 40.2 µs | |
| imdct_inverse_2048 (new) | — | 53.8 µs | |
| flac_stream_1sec_i16 (new) | — | 3.79 ms | |

  FLAC encode's +3.9% is plausibly the encoder now rounding where it truncated; the other two rises are
  within noise. `bench-history.csv` gains the rows stamped `2026-11-08T01:14:29Z`. They were recorded
  before commit, so they carry 2.9.2's hash (09b3632), like the 2.9.2 baseline rows taken at the start
  of this work.

### Added

- **`scripts/interop.sh`** and the **`interop` CI job**: `scripts/interop/cli.cyr` drives shravan's
  decoders, streams and encoders; `scripts/interop/opusref.c` generates libopus vectors (FEC, DTX, loss,
  every kind of switch). 816 cases, all passing (850 against a default-build libopus). Lossless paths must match exactly, and again
  through the streaming decoders.
- **New real-file fixtures** (`scripts/gen-interop-fixtures.py`, now deterministic): 18 libopus packet
  streams, Ogg Opus with an output gain, ISO MPEG conformance excerpts, a LAME Info frame, WAV with a LIST
  chunk, float, EXTENSIBLE and from a pipe, 12-bit WAV and AIFF, AIFF-C fl32 and ulaw, FLAC at 32 bits
  and with a seek table, ID3v2 tags with compressed frames (built from the specs, kept only once
  ffprobe and mutagen read them back), and six MP4s from the bounds audit, built box by box.
- **Opus concealment checked against a default libopus build.** Distributions may build libopus with deep
  PLC (DRED/OSCE), which skips the SILK PLC fade-in at 16 kHz; this machine's does. `opusref caps` tells
  the builds apart by decoder size (155 KB against 18 KB; the DRED entry points are exported as stubs
  either way, so the old symbol check could not), `scripts/interop/build-libopus.sh` makes a default
  static build, and `OPUS_PREFIX` links the interop tools and the fixture generator against it. Against
  it, wideband SILK (10/20/40/60 ms) and hybrid SWB/FB, mono and stereo, under loss and DTX, are
  sample-exact; the deep-PLC library differs from both by 25 to 3,094 LSB on the same packets. Three
  fixtures pin it in the suite (reverting to the deep-PLC behaviour fails exactly those three), the
  sweep adds 34 cases wherever its libopus is a default build, and the generator refuses a deep-PLC
  one rather than record its references.
- **Fuzzing**: `fuzz/fuzz_opus.cyr` feeds damaged libopus packets and hostile OpusHeads to the Opus
  decoder; `fuzz_decode` gains the streaming decoders (split at random), compressed ID3v2 (sankoch's
  inflate), Opus packet sequences and every new fixture, and runs AAC seeds at full rate now that AAC is
  fast. `fuzz/run.sh` runs seeded batches until each harness reaches the call count, then **cold starts**
  (one call per process, so every codec's first use precedes its tables), and takes `--poison` (allocator
  redzones; a hit exits 86) and `--bounds`. CI runs a short pass of all three and a poison pass.
- `src/bench.cyr` includes the library instead of carrying copies of it, which had drifted (they still
  scaled by 2^(N-1) - 1). New benchmarks: f32 conversion, IMDCT, WAV and FLAC streaming.

### Verified

- 12,960 assertions, 0 failing. `./fuzz/run.sh 90000`: 998,846 calls (fuzz_codecs 810,000, fuzz_decode 98,846,
  fuzz_opus 90,000) and 2,200 cold starts, 0 crashes. The same gate with `--poison`: the same
  counts, 0 redzone violations.
  Interop sweep: 816 of 816 cases against this machine's deep-PLC libopus, 850 of 850 against a default
  build (`OPUS_PREFIX`). `cyrius deps --verify`: 44 verified, 0 failed. `cyrius distlib`
  regenerates `dist/shravan.cyr` and `--check` passes, with the same single platform warning as 2.9.2
  (`is_err` on x86_64-windows). Write lengths correct; every source file canonically formatted.

### Not yet verified against a reference

See the roadmap's "Paths no reference has reached": MPEG-2.5 8 kHz mixed blocks, AAC `pulse_data`
(refused), Apple-encoded ALAC, variable-blocksize FLAC, and ID3v2 grouped frames.

## [2.9.2] - 2026-11-07

**Real files, at last.** This release set out to swap in a smaller JSON dependency and clear the
roadmap's quick items. Checking that work against ffmpeg, flac and lame found that **FLAC, ALAC, AAC
and MP3 were all decoding real files wrongly**, behind a green suite, because nearly every test
decoded what shravan itself had encoded. All four now match their references. 11,938 assertions (was
11,706; +232).

### Fixed — decoders that were wrong on real files

| Codec | What was wrong | Effect | Now |
|---|---|---|---|
| **FLAC decode** | LPC prediction rounded toward zero where the format needs an arithmetic shift | Nearly every real file was garbage (correlation 0.07). Only Fixed subframes worked, the one type shravan's encoder writes | 18 of 18 depth × channel × level files from `flac` bit-exact |
| **FLAC encode** | The mid channel had the same bug | Stereo output was lossy: `flac -t` rejected it with an MD5 mismatch | `flac -t` passes: stereo at 8/16/24-bit to 96 kHz, mono at 16/24-bit to 384 kHz |
| **ALAC** | Rice parameter, history, suffix coding, coefficient order and non-adaptive LPC were all wrong, stereo width was one bit short, and shifted low bytes were never read | Never ran on a real frame, and unreachable from any container | Rewritten. 0 differing samples against ffmpeg: mono to 7 channels, 16/24-bit, up to 96 kHz, noise, silence gaps, MP4 and QuickTime |
| **AAC rates** | One band table, the 44.1/48 kHz layout, at all 13 sample rates | Everything else was garbage, returned as success with sample values around 3.9e14 | Per-rate tables generated from libavcodec, not transcribed. All 13 rates decode |
| **AAC stereo** | With `common_window = 0`, the right channel used the left's window shape, and PNS read it with the left's layout | Errors up to 0.32 whenever transients differ between channels | 0 of 130 frames differ |
| **AAC TNS** | One filter kept of up to three, measured from `max_sfb` rather than the band table's top, no `TNS_MAX_BANDS` limit, never applied to short windows | Errors up to 0.32 on transients | 0 frames differ at every band layout, with TNS on |
| **MP3** | Count1 table B's offset was 2261, inside table 24, where the real table sits at 2773. **A test asserted the wrong value** | Every granule using it was corrupted, at every rate. LAME picks it for high-frequency bursts, so simple tones were clean and transients were not | 0 bad granules in 48 rate × bitrate × channel combinations, worst error 3e-6 |
| **MP3 8 kHz** | The short-block region boundary was fixed at 36 lines. It is 72 at 8 kHz, and now comes from the band table | The roadmap's "known edge": transients at 8 kHz correlated at 0.05, at every bitrate | Exact, as above |

AAC comparisons have PNS off where sample equality is the claim: its noise is defined only by energy,
so it can never match a reference sample for sample. That is the format, not a defect, and the roadmap
entry calling it one is gone.

### Added

- **Real encoder output in the suite.** `scripts/gen-interop-fixtures.py` captures ffmpeg, flac and lame
  output with ffmpeg's decodes into `src/interop_fixtures.cyr`, and refuses to write a fixture that does
  not exercise its path. `src/interop_tests.cyr` compares shravan against them: every sample by hash for
  lossless, sampled points for lossy. Each of 15 fixes was then reverted in turn: all 15 failed on the
  right assertion, and an unmodified control passed.
- **ALAC is live.** `mp4_decode` routes `alac` tracks to the new `alac_decode_frames`. Scratch buffers
  are reused across frames: allocating per frame would cost about 380 MB on a three-minute track.
- **`flac_verify(data, len)`** checks a stream against its STREAMINFO MD5. Returns 0 on a match, 1 if the
  file stores no signature, or a negative error. It is separate from `flac_decode` because hashing costs
  about as much again as the decode.
- **WAV `PCM_F64`**, encode and decode, bit-exact with ffmpeg both ways.
- **ID3v2 de-unsynchronisation**, whole-tag in v2.2/2.3 and per-frame in v2.4, plus v2.4's group and data
  length prefixes. Compressed or encrypted v2.4 frames are skipped rather than read as text. ffprobe
  agrees on v2.4. On v2.3 it drops the title's last character, and shravan returns the text as written.
- **MP4**: 64-bit box sizes and chunk offsets (the high word was ignored), QuickTime v1/v2 sample entries,
  `wave`-nested config, and esds/AudioSpecificConfig parsing, so rate and channels come from the stream.
  At 96 kHz the sample entry says something else entirely.
- **Hi-res round-trip tests**, 88.2–384 kHz through WAV, AIFF and FLAC.
- **Real-encoder fuzz seeds.** `fuzz_decode` previously mutated only shravan's own output, which never
  reaches FLAC LPC, ALAC, TNS, short blocks or an esds.

### Changed

- **bayan: JSON-only bundle.** `[deps.bayan]` now takes `dist/bayan-json.cyr` at tag `1.11.1` in place of
  the full stdlib `bayan`. `lib/bayan.cyr` and `lib/unicode/` are gone. `build/shravan` goes from 3,779,024
  to 3,052,408 bytes, below where it was before 2.9.1, even with the new fixtures.
- **`dist/shravan.deps` narrows to 3 leaves** (`ganita`, `assert`, `syscalls`), the rest arriving through
  `bayan-json`. **No consumer manifest needs changing:** `cyrius deps` follows shravan's own `[deps.bayan]`.
  Tested with a manifest with no bayan at all (dhvani's shape) and one keeping stdlib `bayan` (jalwa's and
  nidhi's). The latter still links the full bayan.
- **`detect_format` no longer decides `codec_open`.** sankoch defines the same name and the last definition
  wins, so with sankoch included after shravan, `codec_open` called sankoch's and **failed outright**.
  Internals now use `shravan_detect_format`, and that case passes. `detect_format` stays as a wrapper until
  3.0, so one duplicate-function warning remains. No consumer calls it.
- **`silk.cyr` duplicates removed.** `silk_bwexpander_32` and `silk_div32_varq` were each defined twice with
  different bodies. The earlier definitions were never live, so behaviour is unchanged.
- **MP4 is stricter.** HE-AAC reports `ERR_UNSUPPORTED_FMT` rather than playing its core at half rate, as
  does any track that is neither AAC nor ALAC. Both used to be fed to the AAC decoder.
- Internal ALAC helpers `alac_lg3`, `alac_rice_decode` and `alac_unfilter` are gone, and
  `alac_rice_decode_block` and `alac_decode_element` take different arguments. None of the consumer repos
  call them. `mp4_audio_info` now needs a 32-byte output buffer.
- **Roadmap rewritten.** Completed work removed, open items checked against the code and regrouped into
  v2.10.x–2.12.x with a 3.0 backlog. v2.9.3 is scheduled as a P(-1) sweep.

### Verified

- 11,938 assertions, 0 failing. `./fuzz/run.sh 90000`: 914,764 calls, 0 crashes. `cyrius vet`: 0 untrusted,
  0 missing. Write lengths correct.
- CI's `lib sync` + `deps` sequence passes on the tree as committed and from an empty `lib/`, reproducing all
  44 files and `cyrius.lock` byte for byte, then passes `test-all.sh`.
- **Benchmarks**: flat, median −0.1%, range −2.7% to +7.3% across 12 benchmarks. Taken under heavy load from
  other work (load average 26), so these are CPU time rather than wall clock, pinned to one core, best of
  five alternating runs against 2.9.1. `flac_encode` (+7.3%, with a 9.4% spread on the 2.9.1 side) and
  `flac_decode` (+3.6%) cannot be separated from noise under that load. For the same reason no row was
  added to `bench-history.csv` for this release.

### Correction to 2.9.0 and 2.9.1

Both entries say the build shows 6 warnings. It was 9: the count missed `detect_format`, which the compiler
prints on the same line as its `compile` banner. All three duplicates predate those releases, so nothing
regressed. This release takes it to 3, all `detect_format`.

### Known issues, scheduled for 2.9.3

- **One-shot decoders stop at about 6.3 minutes of stereo**: the 2^25-sample cap makes a 7-minute 44.1 kHz
  FLAC return `ERR_DECODE`.
- **PCM encoders are not the inverse of the decoders**: WAV, AIFF and FLAC encoding scale by 2^(N-1) - 1 and
  truncate, and decoders divide by 2^(N-1), so a decode then re-encode moves nearly every sample by one LSB.
- **AAC decode runs about 4× slower than real time on transients**: `f64_pow` per nonzero coefficient.

## [2.9.1] - 2026-11-07

Toolchain release. One source change, a rename in `src/serde.cyr` that bayan's deprecation requires.
11,706 assertions (unchanged).

### Changed

- **Toolchain `6.7.9` → `6.7.18`**, sankoch `2.8.3` → `2.8.9`, bayan `1.5.13` → `1.11.1`. Each is both
  what the 6.7.18 snapshot bundles and the newest upstream tag; ganita stays at `1.2.15`, also the
  newest. `lib/` re-vendored: all 51 files are byte-identical to the snapshot, whose 113 files match
  the `6.7.18` tag; sankoch resolved from its tag is byte-identical to the bundled copy.
- **New vendored leaves**, all pulled in by bayan 1.11.1: `chrono.cyr`, `hashseed.cyr`, `slice.cyr` and
  `lib/unicode/` (7 files).
- **`src/serde.cyr` moves off a deprecated bayan name.** bayan 1.11.0 deprecated
  `bayan_json_get_int`, so all seven hand-written `*_from_json` lookups now call
  `bayan_json_get_int_by_cstr(pairs, "value")`. Same lookup and null guards, without the `Str`
  allocated per call. Consumers of the bundle no longer inherit the seven warnings.
- **`dist/` regenerated.** The bundle changes by the version stamp and that rename;
  `dist/shravan.deps` is unchanged.
- **Version banners** and toolchain references updated, and the pin-bump procedure in CLAUDE.md
  corrected (see Notes).

### Verified

- **Numerics did not move.** The stdout of all three test binaries is identical to the 6.7.9 build
  apart from the version banner.
- **bayan's stricter parser cannot crash serde.** `bayan_json_parse` now returns 0 for a duplicate
  key, where it used to return a list. All 12 `*_from_json` functions, hand-written and derived,
  survive a null `pairs`.
- `./fuzz/run.sh 90000`: 903,500 calls, 0 crashes. `cyrius vet`: 0 untrusted, 0 missing. Write lengths
  correct. Warnings are back to the original 6.
- **CI's `lib sync` + `deps` sequence passes both ways**, on the tree as committed and from an empty
  `lib/`. Each reproduces all 51 files and `cyrius.lock` byte for byte, then passes `test-all.sh`.
- **Benchmarks** (same session, three alternating runs against a 6.7.9 build): median −1.2%, range
  −21.9% to +2.0% across 12 benchmarks. This session was noisy, with 3.6–10.9% run-to-run spread.
  The one outlier is `pcm_u8_to_f64_4096`, the layout-sensitive loop from 2.8.2, returning from 67 µs
  to 53 µs. That is the same code-layout effect in reverse, not a speedup, and no other delta exceeds
  the spread.

### Notes

- ⚠ **Binaries grow by about 609 KB** (`build/shravan` 3,169,704 → 3,779,024 bytes, +19%), and bayan is
  nearly all of it: linking bayan into an empty program costs 386,480 bytes on 6.7.9 and 980,080 on
  6.7.18, a 593,600-byte increase. A consumer of the bundle grows the same way (1,668,184 →
  2,214,544 bytes in a minimal test, +33%). Building that consumer with `CYRIUS_DCE=1` takes it to
  526,992 bytes.
- ⚠ **The pin-move sequence is now just `cyrius deps --relock`.** Since 6.7.13 `cyrius lib sync` refuses
  to cross a pin move while `lib/` holds files outside the declared set (`mmap`, `result`, `sankoch`),
  and its suggested `--full` would vendor the whole snapshot. `deps --relock` alone moves everything.
  Under an unchanged pin, as in CI, `lib sync` still works.
- `cyrius.lock` now retains the `2.7.14`, `2.8.1` and `2.8.3` sankoch rows beside `2.8.9`, and the
  toolchain prints a note for each. Deleting the lock would re-pin; there is no need to.
- Unchanged from 2.9.0: the distlib `is_err` warning for `x86_64-windows`, and the same nine
  non-canonical `src/*.cyr` files.

## [2.9.0] - 2026-11-07

Toolchain release, moving to the Cyrius 6.7 line. No codec source change, and none of the roadmap's
v2.9.0 codec work is in it. 11,706 assertions (unchanged).

### Changed

- **Toolchain `6.6.20` → `6.7.9`**, sankoch `2.8.1` → `2.8.3`, bayan `1.5.12` → `1.5.13` — the
  versions the 6.7.9 snapshot bundles. `lib/` re-vendored: all 41 files are byte-identical to the
  snapshot, whose 113 files match the `6.7.9` tag; sankoch resolved from its tag is byte-identical
  to the bundled copy. No vendored files added or removed.
- **`dist/` regenerated.** The only change is the version stamp; `dist/shravan.deps` is unchanged.
- **Version banners** in `src/main.cyr` and `src/bench.cyr`, and toolchain references in the README,
  CONTRIBUTING, roadmap and CLAUDE.md, updated.

### Verified

- **The 6.7 language changes do not reach shravan.** `true`, `false`, `const`, `do` and `dyn` are now
  reserved, forward calls are arity-checked, and a `for` step must end at its `)`. All three harnesses
  compile with the same 6 warnings as 6.6.20. `!` is now a real operator, where a lone `!` used to be
  dropped silently; shravan never used it as an operator (two comments and a string literal).
- **Numerics did not move.** The stdout of all three test binaries is identical to the 6.6.20 build
  apart from the version banner.
- `./fuzz/run.sh 90000`: 903,500 calls, 0 crashes. `cyrius vet`: 0 untrusted, 0 missing. Write lengths
  correct. CI's `lib sync` + `deps` sequence replayed from an empty `lib/` reproduces `lib/` and
  `cyrius.lock` byte for byte. A consumer declaring only the 7 sidecar leaves builds and runs.
- **Benchmarks** (same session, three alternating runs against a 6.6.20 build): median −0.5%, range
  −2.3% to +2.0% across 12 benchmarks. No delta exceeds the run-to-run spread, so this is neither a
  win nor a regression.

### Notes

- Binary sizes are flat: `build/shravan` 3,169,800 → 3,169,704 bytes, `build/shravan-silk` +4,000.
- `cyrius.lock` now retains the `2.7.14` and `2.8.1` sankoch commit rows beside `2.8.3` (see 2.8.2).
- Unchanged from 2.8.2: the distlib `is_err` warning for `x86_64-windows`, and the same nine
  non-canonical `src/*.cyr` files.

## [2.8.2] - 2026-11-07

Maintenance release. No codec source change. 11,706 assertions (unchanged).

### Changed

- **Toolchain `6.6.2` → `6.6.20`**, sankoch `2.7.14` → `2.8.1`, bayan `1.5.5` → `1.5.12` — the
  versions the 6.6.20 snapshot bundles. `lib/` re-vendored: all 41 files are byte-identical to the
  snapshot, whose 113 files match the `6.6.20` tag; sankoch resolved from its tag is byte-identical
  to the bundled copy. New vendored leaves `alloc_cx.cyr` and `poison.cyr`.
- **`dist/` regenerated with the 6.6.20 distlib.** The bundle now opens with a compile-verified
  `# Requires` include block, and `dist/shravan.deps` narrows from 15 stdlib leaves to the 7 it needs
  directly (`math`, `ganita`, `io`, `assert`, `str`, `bayan`, `syscalls`); the rest arrive
  transitively. A consumer declaring only those 7 builds and runs against the bundle.
- **Version banners** in `src/main.cyr` and `src/bench.cyr` now read v2.8.2; both had stayed at
  v2.8.0 through 2.8.1. Toolchain references in the README, CONTRIBUTING, roadmap and CLAUDE.md
  corrected from 6.5.35.

### Verified

- **Numerics did not move.** Upstream rewrote `f64_sin`/`f64_cos` (6.6.9) and `f64_pow` (6.6.10) for
  accuracy, so a green suite was not enough on its own: the stdout of all three test binaries,
  built on 6.6.2 and 6.6.20, is identical apart from the version banner.
- `./fuzz/run.sh 90000`: all fuzzers passed, 0 crashes. `cyrius vet`: 0 untrusted, 0 missing. Write
  lengths correct. CI's `lib sync` + `deps` sequence replayed from an empty `lib/` reproduces `lib/`
  and `cyrius.lock` byte for byte.
- **Benchmarks** (same session, three alternating runs against a 6.6.2 build): median +0.5% across
  12 benchmarks. Outliers are `fft_forward_1024` −6.9% and `pcm_u8_to_f64_4096` +25.0%.

  The `pcm_u8` jump is code layout, not a regression. The loop and its callees compile to identical
  instructions on both toolchains, and a reduced program runs at 52 µs on both. Shifting a local copy
  of the callee by a few bytes swings that loop from 33 µs to 66 µs **on 6.6.2**, and from 33 µs to
  38 µs on 6.6.20, so this benchmark says little about either toolchain on this CPU (Zen 3). It is not
  caused by page faults (173 vs 175), allocator position or heap offset; all three were measured.

### Notes

- Harness binaries grow by about 282 KB (`build/shravan` 2,888,016 → 3,169,800 bytes). The growth is
  the same in all three, so it comes from the toolchain and its libraries, not shravan's source. It
  is well inside the compiler's code buffer, a 64 MiB growable region since 6.4.49.
- ⚠ The pin-move sequence is now `cyrius lib sync`, then `cyrius deps --relock`. `lib sync` copies
  declared modules only, so plain `deps` refuses the transitive leaves still at the old pin
  (`atomic`, `boxed`, `mmap`, `result`, `sync*`) as a lock/snapshot disagreement. Relock only when
  the pin really moved; under an unchanged pin that refusal means the snapshot changed.
- `cyrius.lock` keeps the old `sankoch 2.7.14` commit row alongside `2.8.1`. Retaining old-tag rows
  is deliberate toolchain behaviour (fail-closed) since 6.6.20.
- distlib warns that `is_err` is undefined for `x86_64-windows`. `syscalls_windows.cyr` did not
  define it at 6.6.2 either, and Windows is not a shravan target.
- The same nine `src/*.cyr` files as 2.8.1 remain non-canonical under the formatter.

## [2.8.1] - 2026-09-11

### Changed

- **Toolchain `6.5.35` → `6.6.2`**, and sankoch `2.7.10` → `2.7.14`. No source
  change. Zero compiler rejections, zero fail-open sites.

  ⚠ Nine `src/*.cyr` files are not canonically formatted under the 6.6.2 formatter
  (cyrfmt began tracking paren depth at 6.5.28; this repo was pinned 6.5.35 but
  predates the reformat). No CI step runs `cyrius fmt`, so nothing is red; left
  alone rather than shipping a nine-file whitespace diff.

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
