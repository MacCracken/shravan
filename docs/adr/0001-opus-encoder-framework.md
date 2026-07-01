# ADR 0001 — Opus encoder framework

**Status:** Accepted — framework landed in v2.4.4 (2026-07-01). Features roadmapped as 2.5.x.

## Context

shravan ships a **CELT-only** Opus encoder (`src/opus.cyr`): 48 kHz, 1–2 ch
(averaged to mono), CBR, 20 ms fullband frames, **sign-only** spectral shape,
a custom (non-RFC-6716-exact) range coder, Ogg-Opus output. It is not a
bitstream-conformant Opus encoder.

The "Own the stack" goal is a full Opus encoder: SILK (speech), Hybrid,
VBR, stereo coupling, full PVQ, transient/short-block switching. That is a
large multi-release effort. v2.4.4 lands only the **framework** — the top-level
structure the feature work plugs into — without changing existing behavior.

## Opus architecture (RFC 6716) — the framework's decision surface

Opus switches, per frame, between two engines and their union:

| Mode | Engine | Bandwidths | Frame sizes | Use |
|------|--------|-----------|-------------|-----|
| SILK | LP / speech | NB, MB, WB | 10/20/40/60 ms | low-rate voice |
| Hybrid | SILK LB + CELT HB | SWB, FB | 10/20 ms | wideband voice |
| CELT | MDCT / music | NB, WB, SWB, FB | 2.5/5/10/20 ms | music, low-latency |

- **Bandwidths** (§2, Table 1): NB(8k)/MB(12k)/WB(16k)/SWB(24k)/FB(48k) — chosen
  from `min(sample-rate cap, max_bandwidth)`, then narrowed by bitrate.
- **Mode selection**: signal type (voice/music) + bitrate + frame size +
  application; libopus uses bitrate thresholds with hysteresis.
- **TOC byte** (§3.1): `(config<<3)|(stereo<<2)|code`; `config` (0–31) jointly
  encodes (mode, bandwidth, frame size) per Table 2.
- **PVQ** (§4.3.4): CELT codes each band's unit-norm shape as a pyramid vector
  (`Σ|y_i| = K` pulses), indexed combinatorially (CWRS). "Sign-only" (today) is
  the degenerate `K→N` collapse; `K=0` bands are filled by spreading/folding.
- **VBR / rate control**: not normative — a bit-reservoir state machine (libopus).
- **Stereo**: per-band mid/side coupling + intensity stereo (CELT); predictive
  stereo (SILK).
- **Transient**: a detector splits the long MDCT into 2/4/8 short blocks to avoid
  pre-echo; a `transient` flag is coded.

## Decision — v2.4.4 framework (opening work)

Additive to `src/opus.cyr`; the existing `opus_encode()` Ogg path and the test
suite are untouched. Delivered:

- **Enums**: `OpusMode{SILK,HYBRID,CELT}`, `OpusBandwidth{NB,MB,WB,SWB,FB}`,
  `OpusSignal`, `OpusApp`.
- **`OpusEncoder`** heap struct (mode, bandwidth, frame_size, bitrate, vbr,
  channels, sample_rate, signal, app) + getters.
- **`opus_encoder_new` + setters** (bitrate/vbr/bandwidth/mode), each re-running
  selection to keep state consistent.
- **Selection logic** (pure, tested): `opus_bandwidth_cap`, `opus_select_bandwidth`
  (bitrate thresholds, sample-rate clamp), `opus_select_mode`.
- **`opus_toc_config` / `opus_toc_byte`** — the full RFC 6716 config table (0–31).
- **`opus_encode_frame` dispatch** — CELT → existing `_opus_encode_celt_frame`;
  SILK/HYBRID → `ERR_UNSUPPORTED_FMT` (2.5.x seams).
- 5 test functions (+47 assertions → 610).

## Known cleanup (deferred to 2.5.x)

`_opus_encode_celt_frame` hardcodes TOC **config 30** (= CELT/FB 10 ms) but its
frames are 20 ms (= config **31**, which `opus_toc_byte` computes correctly).
Swapping the hardcode for `opus_toc_byte(...)` needs a decode round-trip guard,
so it rides with the CELT/PVQ work (2.5.1), not the additive framework.

## 2.5.x roadmap (Opus spine, deferred items interleaved)

See `docs/development/roadmap.md`. Spine dependency order:
**full PVQ → transient/short-blocks → stereo coupling → rate-control/VBR → SILK
→ Hybrid**; the 2.3.x deferrals (TNS, MP4/M4A, FLAC LPC, perf) and hi-res/DSD are
dependency-independent breathers slotted between the Opus vertebrae.

## References

RFC 6716 §2 (bandwidths), §3.1–3.2 (TOC/framing), §4.2.7 (SILK stereo),
§4.3–4.3.4 (CELT MDCT/transient/stereo/PVQ), §5 (encoder). Rate control:
libopus `opus_encoder.c`, `celt/rate.c`.
