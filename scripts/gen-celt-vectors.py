#!/usr/bin/env python3
"""Regenerate src/celt_enc_vectors.cyr: libopus's CELT encoder on the suite's inputs.

    scripts/gen-celt-vectors.py [--opus-src DIR]

Builds libopus (v1.6.1, cloned, or the source tree DIR) as
scripts/gen-silk-vectors.py does (the portable C build, OPUS_DISABLE_INTRINSICS,
with scripts/silkref/patch.py's dump hooks), and runs scripts/silkref/celtenc.c,
libopus's celt_encode_with_ec, over the configurations below: inputs the suite
builds itself (celt_signal, mirrored by src/celt_enc_tests.cyr's _cek_signal),
every setting of the CELT encoder. For each configuration the vectors hold its
header (the input's recipe and the encoder's settings), and five hashes over its
frames, each an FNV-1a over 32-bit words: the packets (each frame's length,
bytes and final range), the decisions (transient, short blocks, the pre-filter's
on/off, pitch and gain, tf_select, spreading and tapset, the trim, dual stereo,
intensity, coded bands, balance, bytes, silence), the energies (the quantised
log energies, the energy error carried to the next frame) and the allocation
(pulses, fine bits and priorities, the time-frequency resolution, the dynalloc
boosts). The suite checks shravan's encoder against all five: the packets
prove it, the stage hashes say where it parts if it ever does. With
build/interop-cli, shravan's packets are checked here too (its celtlib command).
Needs git (without --opus-src), cmake, a C compiler and numpy.
"""
import argparse
import importlib.util
import math
import os
import struct
import subprocess
import sys
import tempfile

import numpy as np

sys.dont_write_bytecode = True
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_spec = importlib.util.spec_from_file_location("gsv", os.path.join(ROOT, "scripts", "gen-silk-vectors.py"))
gsv = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gsv)

MASK = (1 << 64) - 1
FNV0 = 1469598103934665603
PRIME = 1099511628211


def fnv_words(h, words):
    for w in words:
        h = ((h ^ (w & 0xFFFFFFFF)) * PRIME) & MASK
    return h


def fnv_bytes(h, b):
    for x in b:
        h = ((h ^ x) * PRIME) & MASK
    return h


def s32(v):
    v &= 0xFFFFFFFF
    return v - (1 << 32) if v >= 1 << 31 else v


def lcg(s):
    return (s * 1664525 + 1013904223) & 0xFFFFFFFF


def celt_signal(kind, fs, n, seed, prm):
    """Integer test input that src/celt_enc_tests.cyr (_cek_signal) builds too.
    Kinds below 10 are gen-silk-vectors.py's int_signal (prm its five). The
    others draw w, four LCG draws summed, as int_signal does, each sample:
    10, a tone: s[i] = (2 prm[0] s[i-1] >> 30) - s[i-2] from s[-1] = prm[1],
    s[-2] = 0 (prm[0] = 2^30 cos w), plus w >> prm[2];
    11, bursts: every prm[0] samples a burst of w halving every prm[1]
    samples (eight halvings), over w >> prm[2];
    12, a sweep: kind 10's oscillator with its coefficient prm[0] - i prm[3]
    (falling: the pitch rises), plus w >> prm[2];
    13, -|w| >> prm[2]: never positive (the peak detector's minus side);
    14, prm[2] every prm[0] samples at phase prm[1], else silence;
    15, w >> prm[3] and w >> prm[2] in turn, prm[0] samples each, shifted by
    prm[1] (a level step just before a frame starts: the energy rises past
    what the time-domain transient analysis sees, patch_transient_decision);
    16, w >> prm[2], plus w >> prm[3] from sample prm[1] of every prm[0] (a
    moderate onset late in the frame: the hybrid's weak transients).
    17, prm[0] + (w >> prm[2]): an offset (the tone detector's LPC saturating);
    18, a sawtooth at prm[0] Hz from -prm[1] to prm[1], plus w >> prm[2];
    19, the noise's first period repeated at prm[1] Hz, prm[3] bits down, plus
    w >> prm[2], through a one-pole low-pass whose coefficient (Q14) glides
    from 0 to prm[0] and back every 2 s: periodic, its spectral tilt moving
    (as speech's and music's does: the allocation trim's borders).
    Clipped to 16 bits."""
    if kind < 10:
        return [int(v) for v in gsv.int_signal(kind, fs, n, seed, tuple(prm[:5]))]
    s = seed & 0xFFFFFFFF
    out = []
    y1, y2 = (0 if kind == 19 else prm[1]), 0
    pat = []
    for i in range(n):
        w = 0
        for _ in range(4):
            s = lcg(s)
            w += (s >> 16) - 32768
        if kind == 17:
            x = prm[0] + (w >> prm[2])
        elif kind == 18:
            x = ((i * prm[0]) % fs) * 2 * prm[1] // fs - prm[1] + (w >> prm[2])
        elif kind == 19:
            T = fs // prm[1]
            if i < T:
                pat.append(w)
            t = i % (2 * fs)
            a = prm[0] * (t if t < fs else 2 * fs - t) // fs
            y1 = (pat[i % T] >> prm[3]) + (w >> prm[2]) + ((a * y1) >> 14)
            x = y1
        elif kind == 15:
            x = w >> (prm[3] if ((i + prm[1]) // prm[0]) % 2 == 0 else prm[2])
        elif kind == 16:
            x = (w >> prm[2]) + ((w >> prm[3]) if i % prm[0] >= prm[1] else 0)
        elif kind == 13:
            x = -(abs(w) >> prm[2])
        elif kind == 14:
            x = prm[2] if i % prm[0] == prm[1] else 0
        elif kind == 10 or kind == 12:
            c = prm[0] if kind == 10 else prm[0] - i * prm[3]
            y = ((2 * c * y1) >> 30) - y2
            y2, y1 = y1, y
            x = y + (w >> prm[2])
        else:
            t = i % prm[0]
            k = t // prm[1]
            x = w >> prm[2]
            if k < 8:
                x += w >> k
        out.append(max(-32768, min(32767, x)))
    return out


def celt_stereo(kind, seed, fs, n, prm, rkind, rseed, rprm, rg, rsh):
    """Left celt_signal(kind, seed, prm), right the left times rg (Q14) plus
    celt_signal(rkind, rseed, rprm) rsh bits down; interleaved."""
    L = celt_signal(kind, fs, n, seed, prm)
    O = celt_signal(rkind, fs, n, rseed, rprm)
    x = []
    for a, b in zip(L, O):
        r = max(-32768, min(32767, ((a * rg) >> 14) + (b >> rsh)))
        x += [a, r]
    return x


def tone_prm(fs, f, amp, nshift=15):
    w = 2 * math.pi * f / fs
    return [round((1 << 30) * math.cos(w)), round(amp * math.sin(w)), nshift, 0, 0]


def sweep_prm(fs, n, f0, f1, amp):
    c0 = round((1 << 30) * math.cos(2 * math.pi * f0 / fs))
    c1 = round((1 << 30) * math.cos(2 * math.pi * f1 / fs))
    return [c0, round(amp * math.sin(2 * math.pi * f0 / fs)), 14, max(1, (c0 - c1) // n), 0]


COMB = [14000, 90, 200, 0, 0]
NONE = [0, 0, 0, 0, 0]

# The configurations: name, signal (kind, seed, prm), stereo right channel
# (rkind, rseed, rprm, rg, rsh) or None, scale (the input is the integers times
# 2^(scale - 15)), seconds, rate, channels, and celtenc's keys. Each covers what
# the dev sweep (scratch, 1,386 cases, on real speech and music as well)
# found exact: every frame size, rate, complexity, rate control, the stereo
# modes and the hybrid's start band.
def configs():
    c = []
    def add(name, sig, right, scale, secs, fs, ch, keys):
        c.append((name, sig, right, scale, secs, fs, ch, keys))
    n0 = (0, 3, NONE)
    w1 = (1, 5, NONE)
    v6 = (6, 7, COMB)
    p7 = (7, 9, [140, 4, 0, 0, 0])
    s2 = (2, 4, NONE)
    r4 = (4, 6, NONE)
    clk = (11, 8, [12000, 120, 7, 0, 0])
    # mono, 48 kHz: every frame size, complexity 10, 5 and 0, CBR
    for name, sig in (("noise", n0), ("voiced", v6), ("clicks", clk), ("periodic", p7)):
        for n in (960, 480, 240, 120):
            for cx in (10, 5, 0):
                add("m_%s_%d_c%d" % (name, n, cx), sig, None, 0, 0.4, 48000, 1,
                    ["n=%d" % n, "cx=%d" % cx, "bytes=%d" % {960: 160, 480: 100, 240: 60, 120: 40}[n]])
    # tones: the tone detector and its pre-filter path, the transient guard
    for f in (440, 60, 5100, 15000):
        add("tone%d" % f, (10, 2, tone_prm(48000, f, 11000)), None, 0, 0.5, 48000, 1, ["cx=10", "bytes=120"])
        add("tone%d_v" % f, (10, 3, tone_prm(48000, f, 9000, 9)), None, 0, 0.5, 48000, 1, ["cx=10", "vbr=1", "bitrate=48000"])
    add("sweep", (12, 5, sweep_prm(48000, 48000, 80, 9000, 9000)), None, 0, 1.0, 48000, 1, ["cx=10", "vbr=1", "bitrate=64000"])
    add("tone30", (10, 6, tone_prm(48000, 30, 12000)), None, 0, 0.6, 48000, 1, ["cx=10", "bytes=120"])
    # the tone detector's LPC at its limits: an offset, a tone near Nyquist
    add("dc", (17, 60, [9000, 0, 12, 0, 0]), None, 0, 0.4, 48000, 1, ["cx=10", "bytes=100"])
    add("dc_neg", (17, 61, [-12000, 0, 10, 0, 0]), None, 0, 0.4, 48000, 1, ["cx=10", "vbr=1", "bitrate=32000"])
    add("tone23500", (10, 7, tone_prm(48000, 23500, 9000)), None, 0, 0.4, 48000, 1, ["cx=10", "bytes=120"])
    add("tone22000_st", (10, 8, tone_prm(48000, 22000, 9000)), (0, 62, NONE, 16384, 15), 0, 0.4, 48000, 2, ["cx=10", "bytes=160"])
    # CBR with the VBR constraint off (dynalloc's halving and cap read both)
    add("cbr_nocon", clk, None, 0, 0.5, 48000, 1, ["cx=10", "vbrc=0", "bytes=150"])
    add("cbr_nocon_v", v6, None, 0, 0.5, 48000, 1, ["cx=10", "vbrc=0", "bitrate=64000"])
    # VBR, constrained and not, across rates and frame sizes
    for br in (6000, 16000, 32000, 64000, 128000, 256000):
        for vc in (1, 0):
            add("vbr%d_c%d" % (br, vc), v6, None, 0, 0.6, 48000, 1, ["cx=9", "vbr=1", "vbrc=%d" % vc, "bitrate=%d" % br])
    for n in (480, 240, 120):
        add("vbr_n%d" % n, clk, None, 0, 0.4, 48000, 1, ["n=%d" % n, "cx=9", "vbr=1", "bitrate=48000"])
    # CBR from the bit rate, and buffers too small
    for br in (8000, 24000, 96000):
        add("cbr%d" % br, n0, None, 0, 0.4, 48000, 1, ["cx=7", "bitrate=%d" % br])
    add("sizes", n0, None, 0, 0.5, 48000, 1, ["cx=10", "sizes=2,3,5,8,13,40,100,300,1275"])
    add("rates", v6, None, 0, 1.0, 48000, 1, ["cx=10", "vbr=1", "rates=6000,64000,12000,256000"])
    # every complexity
    for cx in range(11):
        add("cx%d" % cx, v6, None, 0, 0.5, 48000, 1, ["cx=%d" % cx, "vbr=1", "bitrate=32000"])
        add("cx%d_st" % cx, v6, (0, 11, NONE, 9000, 2), 0, 0.5, 48000, 2, ["cx=%d" % cx, "vbr=1", "bitrate=64000"])
    # every input rate (CELT's upsampling), each frame size
    for fs in (24000, 16000, 12000, 8000):
        for n in (fs // 50, fs // 100, fs // 200, fs // 400):
            add("fs%d_n%d" % (fs, n), v6, None, 0, 0.4, fs, 1, ["n=%d" % n, "cx=10", "vbr=1", "bitrate=24000"])
        add("fs%d_st" % fs, n0, (1, 12, NONE, 0, 3), 0, 0.4, fs, 2, ["cx=10", "vbr=1", "bitrate=48000"])
    # stereo: images from identical to independent, intensity, dual stereo,
    # mono-coded stereo and moves between them, phase inversion off
    for name, right in (("same", (0, 3, NONE, 16384, 15)), ("anti", (0, 3, NONE, -16384, 15)),
                        ("indep", (1, 13, NONE, 0, 1)), ("pan", (0, 14, NONE, 4000, 4)),
                        ("onesil", (0, 15, NONE, 0, 15))):
        for n in (960, 480, 120):
            add("st_%s_%d" % (name, n), n0, right, 0, 0.4, 48000, 2, ["n=%d" % n, "cx=10", "bytes=%d" % (200 if n > 120 else 60)])
        add("st_%s_v" % name, v6, right, 0, 0.6, 48000, 2, ["cx=8", "vbr=1", "bitrate=96000"])
    for br in (16000, 32000, 48000, 72000, 128000):
        add("st_int%d" % br, n0, (1, 17, NONE, 6000, 2), 0, 0.5, 48000, 2, ["cx=10", "vbr=1", "bitrate=%d" % br])
    add("st_mono", n0, (1, 18, NONE, 0, 1), 0, 0.5, 48000, 2, ["cx=10", "stream=1", "bytes=100"])
    add("st_switch", v6, (0, 19, NONE, 8000, 2), 0, 1.0, 48000, 2, ["cx=10", "vbr=1", "bitrate=64000", "streams=1122121"])
    add("st_inv", n0, (0, 3, NONE, -16000, 9), 0, 0.5, 48000, 2, ["cx=10", "inv=1", "bytes=80"])
    add("st_tones", (10, 2, tone_prm(48000, 440, 9000)), (10, 4, tone_prm(48000, 3100, 9000), 0, 1), 0, 0.5, 48000, 2,
        ["cx=10", "bytes=120"])
    add("st_clicks", clk, (11, 20, [12000, 60, 8, 0, 0], 8192, 1), 0, 0.5, 48000, 2, ["n=480", "cx=10", "bytes=110"])
    # prediction, loss, input depth
    for pr in (0, 1, 2):
        add("pred%d" % pr, v6, None, 0, 0.5, 48000, 1, ["pred=%d" % pr, "cx=10", "bytes=100"])
    for loss in (2, 3, 4, 5, 8, 9, 30):
        add("loss%d" % loss, v6, None, 0, 0.5, 48000, 1, ["loss=%d" % loss, "cx=10", "vbr=1", "bitrate=48000"])
    for lsb in (8, 16, 24):
        add("lsb%d" % lsb, (3, 21, NONE), None, -6, 0.4, 48000, 1, ["lsb=%d" % lsb, "cx=10", "bytes=80"])
    add("silence", s2, None, 0, 1.0, 48000, 1, ["cx=10", "vbr=1", "bitrate=32000"])
    add("silence_st", s2, (2, 22, NONE, 8000, 2), 0, 1.0, 48000, 2, ["cx=10", "bytes=120"])
    add("loud", w1, None, 2, 0.4, 48000, 1, ["cx=10", "bytes=200"])
    # the silence decision's peak: negative samples only, and one sample a
    # frame (its first, its last, a 2.5 ms frame's)
    add("negative", (13, 30, [0, 0, 6, 0, 0]), None, 0, 0.4, 48000, 1, ["cx=10", "bytes=80"])
    add("negative_st", (13, 31, [0, 0, 9, 0, 0]), (13, 32, [0, 0, 9, 0, 0], 0, 0), 0, 0.4, 48000, 2, ["cx=10", "vbr=1", "bitrate=48000"])
    # the transient decision's corners: strong bursts after near-silence (the
    # metric's top), steps just before the frame (patch_transient_decision),
    # moderate onsets in a low-rate hybrid (weak transients, the hybrid's TF)
    add("bursts", (11, 33, [9600, 50, 15, 0, 0]), None, 0, 0.6, 48000, 1, ["cx=10", "bytes=120"])
    add("bursts_n120", (11, 34, [4800, 30, 14, 0, 0]), None, 0, 0.3, 48000, 1, ["n=120", "cx=10", "bytes=50"])
    add("patch", (15, 41, [1920, 60, 2, 8, 0]), None, 0, 1.0, 48000, 1, ["cx=10", "bytes=120"])
    add("patch_st", (15, 43, [1920, 60, 2, 8, 0]), (15, 44, [1920, 60, 3, 8, 0], 0, 0), 0, 1.0, 48000, 2, ["cx=10", "bytes=200"])
    add("patch_n480", (15, 45, [960, 60, 2, 8, 0]), None, 0, 1.0, 48000, 1, ["n=480", "cx=6", "vbr=1", "bitrate=64000"])
    # steps of 6 and 12 dB: the patch's 6 dB mean rise decided near its threshold
    add("patch6", (15, 41, [1920, 60, 3, 4, 0]), None, 0, 1.0, 48000, 1, ["cx=10", "bytes=120"])
    add("patch12", (15, 55, [1920, 80, 3, 5, 0]), None, 0, 1.0, 48000, 1, ["cx=10", "bytes=120"])
    add("patch6_st", (15, 56, [1920, 60, 3, 4, 0]), (15, 57, [1920, 60, 4, 5, 0], 0, 0), 0, 1.0, 48000, 2, ["cx=10", "bytes=200"])
    add("patch12_n480", (15, 58, [960, 60, 3, 5, 0]), None, 0, 1.0, 48000, 1, ["n=480", "cx=10", "bytes=80"])
    add("hyb_weak", (16, 42, [960, 600, 5, 3, 0]), None, 0, 1.0, 48000, 1, ["start=17", "silk=7", "cx=9", "bytes=14"])
    add("hyb_weak_vbr", (16, 46, [960, 600, 5, 3, 0]), None, 0, 1.0, 48000, 1, ["start=17", "silk=8", "cx=9", "vbr=1", "bitrate=5600"])
    add("hyb_weak_st", (16, 47, [960, 600, 5, 3, 0]), (16, 48, [960, 600, 5, 3, 0], 0, 0), 0, 0.6, 48000, 2,
        ["start=17", "silk=9", "cx=9", "bytes=20"])
    # high pitches (remove_doubling's short-period thresholds), periodic
    # stereo (the pre-filter's two-channel test), one tone in both channels
    add("pitch1500", (7, 49, [1500, 5, 0, 0, 0]), None, 0, 0.5, 48000, 1, ["cx=10", "bytes=120"])
    add("pitch600", (7, 50, [600, 5, 0, 0, 0]), None, 0, 0.5, 48000, 1, ["cx=10", "vbr=1", "bitrate=48000"])
    add("pitch2400", (7, 51, [2400, 4, 0, 0, 0]), None, 0, 0.5, 48000, 1, ["cx=7", "bytes=100"])
    add("st_periodic", (7, 52, [140, 4, 0, 0, 0]), (7, 52, [140, 4, 0, 0, 0], 0, 0), 0, 0.6, 48000, 2, ["cx=10", "vbr=1", "bitrate=96000"])
    add("st_periodic_half", (7, 53, [220, 5, 0, 0, 0]), (0, 54, NONE, 8192, 4), 0, 0.6, 48000, 2, ["cx=10", "bytes=200"])
    add("st_tone_same", (10, 2, tone_prm(48000, 440, 9000)), (0, 3, NONE, 16384, 15), 0, 0.5, 48000, 2, ["cx=10", "bytes=150"])
    add("st_tone60", (10, 2, tone_prm(48000, 60, 9000)), (0, 3, NONE, 12000, 15), 0, 0.5, 48000, 2, ["cx=10", "bytes=150"])
    add("impulse0", (14, 0, [960, 0, 3000, 0, 0]), None, 0, 0.4, 48000, 1, ["cx=10", "bytes=60"])
    add("impulse_end", (14, 0, [960, 959, 3000, 0, 0]), None, 0, 0.4, 48000, 1, ["cx=10", "bytes=60"])
    add("impulse120", (14, 0, [120, 0, 800, 0, 0]), None, 0, 0.2, 48000, 1, ["n=120", "cx=10", "bytes=40"])
    add("impulse16k", (14, 0, [320, 0, 3000, 0, 0]), None, 0, 0.4, 16000, 1, ["cx=10", "vbr=1", "bitrate=24000"])
    add("clip", r4, None, 17, 0.3, 48000, 1, ["cx=10", "bytes=200"])
    # the hybrid's CELT: from band 17, after SILK's bits, with its info
    for n in (960, 480):
        # (10 ms at 16 kbit/s leaves CELT less than the pre-written bits: libopus errs)
        for br in ((16000, 32000) if n == 960 else (24000, 40000)):
            add("hyb_%d_%d" % (n, br), clk, None, 0, 0.5, 48000, 1,
                ["start=17", "n=%d" % n, "vbr=1", "bitrate=%d" % br, "silk=7", "pre=11", "cx=9", "bytes=%d" % (br // 400 + 40)])
        add("hyb_st_%d" % n, v6, (0, 23, NONE, 0, 1), 0, 0.5, 48000, 2,
            ["start=17", "n=%d" % n, "vbr=1", "bitrate=40000", "silk=3", "pre=5", "cx=9", "bytes=120"])
        add("hyb_cbr_%d" % n, n0, None, 0, 0.5, 48000, 1, ["start=17", "n=%d" % n, "silk=9", "pre=13", "cx=9", "bytes=70"])
    # the bandwidths
    for end in (13, 15, 17, 19):
        add("end%d" % end, n0, None, 0, 0.4, 48000, 1, ["end=%d" % end, "cx=10", "vbr=1", "bitrate=32000"])
    add("end17_st", n0, (1, 24, NONE, 0, 1), 0, 0.4, 48000, 2, ["end=17", "cx=10", "bytes=150"])
    # the tonality analysis's info, LFE, the surround mask, resets, frame
    # sizes changing
    for seed in (1, 2):
        add("ana%d" % seed, v6, None, 0, 0.6, 48000, 1, ["ana=%d" % seed, "cx=10", "vbr=1", "bitrate=40000"])
        add("ana%d_st" % seed, v6, (0, 25, NONE, 8000, 2), 0, 0.6, 48000, 2, ["ana=%d" % seed, "cx=10", "vbr=1", "bitrate=80000"])
        add("mask%d" % seed, n0, (1, 26, NONE, 0, 1), 0, 0.5, 48000, 2, ["mask=%d" % seed, "cx=10", "vbr=1", "bitrate=96000"])
    add("mask_m", clk, None, 0, 0.5, 48000, 1, ["mask=3", "cx=10", "vbr=1", "bitrate=48000"])
    add("lfe", n0, None, 0, 0.5, 48000, 1, ["lfe=1", "cx=10", "vbr=1", "bitrate=20000"])
    add("lfe_cbr", n0, None, 0, 0.5, 48000, 1, ["lfe=1", "cx=10", "bytes=30"])
    add("reset", v6, None, 0, 0.6, 48000, 1, ["reset=5", "cx=10", "vbr=1", "bitrate=32000"])
    add("reset_st", n0, (1, 27, NONE, 0, 1), 0, 0.5, 48000, 2, ["reset=3", "cx=10", "bytes=100"])
    add("frames", v6, None, 0, 1.0, 48000, 1, ["frames=960,120,240,480,120,960", "cx=10", "vbr=1", "bitrate=40000"])
    # corners the mutation search found unreached: CBR whose byte count rounds
    # near a step (the coder's first bit), a burst ending at a frame boundary
    # (the overlap's peak, silence after it), a peak at the silence threshold,
    # pitch jumps with the analysis's tonality (pitch_change), the hybrid at
    # complexity 0, the hybrid's transient floor at a low rate, 970 VBR frames
    # (the averaging's fixed rate after them), pre-written bits near 15 bytes
    add("cbr32160", n0, None, 0, 0.4, 48000, 1, ["cx=7", "bitrate=32160"])
    add("cbr_st40160", n0, (1, 63, NONE, 0, 1), 0, 0.4, 48000, 2, ["cx=7", "bitrate=40160"])
    add("silence_n480", s2, None, 0, 1.0, 48000, 1, ["n=480", "cx=10", "vbr=1", "bitrate=32000"])
    add("silence_edge", (14, 0, [480, 7, 1, 0, 0]), None, -1, 0.3, 48000, 1, ["n=480", "lsb=16", "cx=10", "bytes=40"])
    add("ana_jump", (6, 64, [14000, 90, 200, 0, 100]), None, 0, 1.0, 48000, 1, ["ana=4", "cx=10", "vbr=1", "bitrate=48000"])
    add("ana_jump_st", (6, 65, [14000, 90, 200, 0, 80]), (0, 66, NONE, 12000, 3), 0, 1.0, 48000, 2, ["ana=5", "cx=10", "vbr=1", "bitrate=96000"])
    add("hyb_cx0", clk, None, 0, 0.5, 48000, 1, ["start=17", "silk=10", "cx=0", "vbr=1", "bitrate=24000"])
    add("hyb_cx0_st", v6, (0, 67, NONE, 0, 1), 0, 0.5, 48000, 2, ["start=17", "silk=11", "cx=0", "bytes=80"])
    add("hyb_clicks_low", (11, 68, [9600, 50, 15, 0, 0]), None, 0, 1.0, 48000, 1, ["start=17", "silk=12", "cx=9", "vbr=1", "bitrate=6000"])
    add("hyb_pre_small", n0, None, 0, 1.0, 48000, 1, ["start=17", "silk=13", "pre=17", "cx=9", "bytes=24"])
    add("hyb_st_low", v6, (1, 69, NONE, 0, 2), 0, 0.6, 48000, 2, ["start=17", "silk=14", "cx=10", "vbr=1", "bitrate=12000"])
    add("vbr_long", v6, None, 0, 2.6, 48000, 1, ["n=120", "cx=5", "vbr=1", "bitrate=64000"])
    add("frames_st", clk, (11, 28, [12000, 60, 8, 0, 0], 8192, 1), 0, 1.0, 48000, 2, ["frames=480,120,960,240", "cx=10", "bytes=90"])
    # what a second mutation search found: decisions that real speech and
    # music (the dev sweep's) moved and the inputs above never did. An exactly
    # silent channel (the transient analysis's floor, a one-sample band), a
    # slow chirp at a low rate (the tone path's period), sawtooths in stereo
    # (the pre-filter's two-channel test at its borders, the VBR's stereo
    # saving), two tones (the cancel test, dynalloc's second channel, the
    # spreading weights, theta's bias and the noise fill under resynthesis),
    # quiet clicks at 320 kbit/s (dynalloc's noise floor, the VBR's floor
    # depth), 2.5 ms at 16 kHz (the coarse budget's clamps), identical and
    # opposite stereo (theta at its ends, the RDO's saved bytes), a periodic
    # signal whose tilt glides (the tapset's hysteresis, the VBR's tonality),
    # a tone at 15 kHz in 2.5 ms (the follower's top bands), clicks under a
    # louder right channel (temporal VBR's second channel)
    sil = (14, 0, [1, 1, 0, 0, 0])
    add("mx_onesil0", n0, sil + (0, 0), 0, 0.4, 48000, 2, ["n=120", "cx=7", "bytes=200"])
    add("mx_chirp", (12, 71, sweep_prm(48000, 96000, 100, 1500, 13000)), None, 0, 2.0, 48000, 1, ["cx=5", "bytes=20"])
    saw = (18, 73, [150, 9830, 7, 0, 0])
    add("mx_sawst", saw, sil + (13107, 0), 0, 0.4, 48000, 2, ["n=120", "cx=10", "bytes=60"])
    add("mx_sawst_loss", saw, sil + (13107, 0), 0, 0.8, 48000, 2, ["loss=25", "cx=10", "vbr=1", "bitrate=48000"])
    saw220 = (18, 93, [220, 9830, 7, 0, 0])
    add("mx_saw220", saw220, (0, 95, NONE, 13107, 8), 0, 1.0, 48000, 2, ["n=120", "cx=10", "bytes=60"])
    add("mx_saw220_loss", saw220, sil + (13107, 0), 0, 1.0, 48000, 2, ["loss=10", "cx=10", "vbr=1", "bitrate=48000"])
    t440, t5k = tone_prm(48000, 440, 13000), tone_prm(48000, 5123, 9800)
    add("mx_tonest", (10, 2, t440), (10, 4, t5k, 0, 0), 0, 0.6, 48000, 2, ["n=480", "cx=7", "bytes=200"])
    add("mx_tonest_swap", (10, 4, t5k), (10, 2, t440, 0, 0), 0, 0.8, 48000, 2, ["n=480", "cx=7", "bytes=200"])
    add("mx_tonest_n120", (10, 2, t440), (10, 4, t5k, 0, 0), 0, 0.5, 48000, 2, ["n=120", "cx=10", "vbr=1", "bitrate=128000"])
    add("mx_clicksq", (11, 72, [12000, 120, 12, 0, 0]), None, 0, 1.0, 48000, 1, ["cx=9", "vbr=1", "vbrc=0", "bitrate=320000"])
    add("mx_fs16k_st_n40", n0, (0, 74, NONE, 0, 0), 0, 0.4, 16000, 2, ["n=40", "cx=10", "vbr=1", "bitrate=48000"])
    add("mx_same", n0, sil + (16384, 0), 0, 0.4, 48000, 2, ["n=120", "cx=10", "bytes=200"])
    add("mx_anti", n0, sil + (-16384, 0), 0, 0.4, 48000, 2, ["n=480", "cx=10", "bytes=1275"])
    add("mx_pan", n0, (0, 75, NONE, 4915, 4), 0, 0.6, 48000, 2, ["n=240", "cx=9", "vbr=1", "vbrc=0", "bitrate=24000"])
    add("mx_ana_indep", n0, (0, 77, NONE, 0, 0), 0, 0.6, 48000, 2, ["ana=1", "cx=10", "vbr=1", "bitrate=80000"])
    add("mx_tilt_st", (19, 80, [14000, 150, 7, 4, 0]), (19, 81, [12000, 190, 7, 4, 0], 11469, 1), 0, 3.0, 48000, 2,
        ["cx=10", "vbr=1", "bitrate=96000"])
    add("mx_tilt", (19, 106, [15000, 180, 8, 3, 0]), None, 0, 4.0, 48000, 1, ["cx=10", "bytes=160"])
    add("mx_tilt_hf", (19, 114, [6000, 400, 6, 2, 0]), (19, 115, [5000, 300, 6, 2, 0], 0, 0), 0, 2.0, 48000, 2,
        ["n=120", "cx=10", "vbr=1", "bitrate=128000"])
    add("mx_t15k_st", (10, 110, tone_prm(48000, 15000, 9000)), (0, 111, NONE, 0, 2), 0, 1.0, 48000, 2,
        ["n=120", "cx=10", "vbr=1", "bitrate=128000"])
    add("mx_clk_right", clk, (0, 97, NONE, 8192, 0), 0, 1.5, 48000, 2, ["n=240", "cx=9", "vbr=1", "vbrc=1", "bitrate=24000"])
    add("mx_clk_right_480", clk, (0, 97, NONE, 8192, 0), 0, 1.5, 48000, 2, ["n=480", "cx=9", "vbr=1", "vbrc=1", "bitrate=24000"])
    # the analysis's bandwidth where it decides which bands are kept: stereo at 110 kbit/s (min_bw 18)
    add("mx_ana_hi_st", v6, (0, 25, NONE, 8000, 2), 0, 1.0, 48000, 2, ["ana=3", "cx=10", "vbr=1", "bitrate=110000"])
    # pitch_change where its boost shows: a strongly periodic source whose pitch jumps, the analysis's
    # tonality, VBR with room under the target's cap of twice the base (at 48 kbit/s the tonality
    # boost alone reaches it); and its gate (the analysis's validity, its tonality) at 32 kbit/s
    add("mx_pitch_jump", (6, 132, [15800, 100, 300, 0, 100]), None, 0, 2.0, 48000, 1, ["ana=2", "cx=10", "vbr=1", "vbrc=0", "bitrate=96000"])
    add("mx_pitch_jump60", (6, 141, [16000, 120, 260, 0, 60]), None, 0, 2.0, 48000, 1, ["ana=1", "cx=10", "vbr=1", "bitrate=32000"])
    return c


# A configuration's header, 51 words: kind, seed, rate, channels, samples a
# channel, frames, the input's hash (2), prm (5), the right channel's kind,
# seed, gain and shift and its prm (5) (zeros for mono), the scale, the
# encoder's settings in KEYS's order (19; -1 for bitrate max and for the
# synthetic info left off), the frames' bytes, then the packet, decision,
# energy and allocation hashes (2 each, the low half first).
KEYS = ["n", "bitrate", "vbr", "vbrc", "cx", "loss", "pred", "lsb", "inv", "start", "end", "lfe", "stream", "bytes", "reset",
        "ana", "silk", "mask", "pre"]


def key_words(keys, fs, ch):
    d = dict(k.split("=", 1) for k in keys)
    v = {"n": fs // 50, "bitrate": -1, "vbr": 0, "vbrc": 1, "cx": 5, "loss": 0, "pred": 2, "lsb": 24, "inv": 0,
         "start": 0, "end": 21, "lfe": 0, "stream": ch, "bytes": 1275, "reset": 0, "ana": -1, "silk": -1, "mask": -1, "pre": -1}
    for k in KEYS:
        if k in d:
            v[k] = -1 if d[k] == "max" else int(d[k])
    lists = {}
    for k in ("frames", "rates", "sizes"):
        lists[k] = [int(x) for x in d[k].split(",")] if k in d else []
    lists["streams"] = [int(x) for x in d["streams"]] if "streams" in d else []
    full = ["%s=%s" % (k, "max" if (k == "bitrate" and v[k] == -1) else v[k]) for k in KEYS if v[k] != -1 or k == "bitrate"]
    full += ["%s=%s" % (k, d[k]) for k in ("frames", "rates", "sizes", "streams") if k in d]
    return [v[k] for k in KEYS], lists, full


def records(path):
    """The dump's records, one dict a frame (tag -> its values, as strings)."""
    frames, cur = [], None
    for line in open(path):
        t = line.split()
        if not t:
            continue
        if t[0] == "ce.begin":
            cur = {}
            frames.append(cur)
        if cur is not None and t[0] not in cur:
            cur[t[0]] = t[1:]
    return frames


def stage_hashes(frames):
    hd, he, ha = FNV0, FNV0, FNV0
    for f in frames:
        b = [int(x) for x in f["ce.begin"]]
        LM, C, CC, start, end = b[0], b[1], b[2], b[3], b[4]
        I = lambda tag: [int(x) for x in f[tag]]
        H = lambda tag: [int(x, 16) for x in f[tag]]
        tr, pf, sp, al = I("ce.trans2"), I("ce.pf"), I("ce.spread"), I("ce.alloc")
        hd = fnv_words(hd, [tr[0], tr[1], pf[0], pf[1], pf[2], I("ce.tfsel")[0], sp[0], sp[1], I("ce.trim")[0],
                            al[3], al[2], al[0], al[1], I("ce.end")[1], b[11]])
        le = H("ce.logEq")
        he = fnv_words(he, [le[c * 21 + i] for c in range(C) for i in range(start, end)] + H("ce.eerr")[:CC * 21])
        pu, fq, fp = I("ce.pulses"), I("ce.fineq"), I("ce.finep")
        ha = fnv_words(ha, pu[start:end] + fq[start:end] + fp[start:end] + I("ce.tfcode")[:end] + I("ce.boost")[:21])
    return hd, he, ha


def packets(path):
    d = open(path, "rb").read()
    r = open(path + ".rng", "rb").read()
    out, i, k = [], 0, 0
    while i < len(d):
        n = struct.unpack_from("<I", d, i)[0]
        out.append((d[i + 4:i + 4 + n], struct.unpack_from("<I", r, 4 * k)[0]))
        i += 4 + n
        k += 1
    return out


def packet_hash(pk):
    h = FNV0
    for b, rng in pk:
        h = fnv_bytes(h, struct.pack("<I", len(b)) + b + struct.pack("<I", rng))
    return h


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--opus-src", help="a libopus v1.6.1 source tree (default: clone it)")
    args = ap.parse_args()
    cli = os.path.join(ROOT, "build", "interop-cli")
    hdrs, lists = [], {"frames": [], "rates": [], "sizes": [], "streams": []}
    checked = 0
    total_frames = 0
    with tempfile.TemporaryDirectory() as work:
        celtenc = gsv.build(args.opus_src, work)[3]
        for name, sig, right, scale, secs, fs, ch, keys in configs():
            n = int(fs * secs)
            kind, seed, prm = sig
            if right is None:
                x = celt_signal(kind, fs, n, seed, prm)
                rw = [0] * 9
            else:
                rkind, rseed, rprm, rg, rsh = right
                x = celt_stereo(kind, seed, fs, n, prm, rkind, rseed, rprm, rg, rsh)
                rw = [rkind, rseed, rg, rsh] + list(rprm)
            hx = fnv_words(FNV0, x)
            inp = os.path.join(work, name + ".f64")
            np.array([v * 2.0 ** (scale - 15) for v in x], dtype="<f8").tofile(inp)
            kw, ls, full = key_words(keys, fs, ch)
            pk = os.path.join(work, name + ".pk")
            dump = os.path.join(work, name + ".txt")
            env = dict(os.environ, CELTDUMP=dump)
            r = subprocess.run([celtenc, inp, pk, str(fs), str(ch)] + full, env=env, capture_output=True, text=True)
            if r.returncode != 0:
                sys.exit("gen-celt-vectors: celtenc %s: %s" % (name, r.stderr))
            p = packets(pk)
            fr = records(dump)
            gsv.require(len(fr) == len(p) and len(p) > 0, "%s: %d dumps for %d frames" % (name, len(fr), len(p)))
            hd, he, ha = stage_hashes(fr)
            hp = packet_hash(p)
            nbytes = sum(len(b) for b, _ in p)
            hdrs.append([s32(v) for v in [kind, seed, fs, ch, n, len(p), hx & 0xFFFFFFFF, hx >> 32] + list(prm) + rw
                         + [scale] + kw + [nbytes] + [h for v in (hp, hd, he, ha) for h in (v & 0xFFFFFFFF, v >> 32)]])
            for k in lists:
                lists[k].append(ls[k])
            total_frames += len(p)
            if os.path.exists(cli):
                sp = os.path.join(work, name + ".shr.pk")
                r2 = subprocess.run([cli, "celtlib", inp, sp, str(fs), str(ch)] + full, capture_output=True, text=True)
                gsv.require(r2.returncode == 0, "%s: celtlib failed: %s" % (name, r2.stdout + r2.stderr))
                gsv.require(packets(sp) == p, "%s: shravan's packets differ from libopus's" % name)
                checked += 1
            os.unlink(dump)
    w = gsv.Writer()
    w.parts.append("# celt_enc_vectors.cyr -- libopus's CELT encoder on the suite's inputs (scripts/gen-celt-vectors.py).\n"
                   "# Generated: do not edit. Each field one zlib stream in base64: the segment count, each segment's\n"
                   "# length (u32), then the segments, one a configuration, of little-endian 32-bit words.\n\n")
    w.int("_CEV_N", len(hdrs))
    w.field("_CEV_HDR", hdrs, lambda h: h, "i")
    w.field("_CEV_FRAMES", lists["frames"], lambda v: v, "i")
    w.field("_CEV_RATES", lists["rates"], lambda v: v, "i")
    w.field("_CEV_SIZES", lists["sizes"], lambda v: v, "i")
    w.field("_CEV_STREAMS", lists["streams"], lambda v: v, "i")
    dest = os.path.join(ROOT, "src", "celt_enc_vectors.cyr")
    open(dest, "w").write("".join(w.parts))
    print("wrote %s (%d configurations, %d frames, %d bytes)%s" % (
        os.path.relpath(dest), len(hdrs), total_frames, os.path.getsize(dest),
        "; shravan's packets identical in all %d" % checked if checked else "; no build/interop-cli, shravan's packets not checked"))


if __name__ == "__main__":
    main()
