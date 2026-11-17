#!/usr/bin/env python3
"""Reference vectors for shravan's SILK encoder, from an instrumented libopus.

    scripts/gen-silk-vectors.py [--opus-src DIR]

Builds libopus (v1.6.1, cloned, or the source tree DIR) with the dump hooks of
scripts/silkref/patch.py, runs its SILK encoder (scripts/silkref/silkref.c:
silk_Encode, mono at the internal rate) over deterministic test signals, and
writes src/silk_enc_vectors.cyr: chosen frames' stage inputs and outputs, so
shravan's stages can be fed libopus's own inputs (src/silk_tests.cyr), and
(2.11.3) libopus's whole packets for inputs the suite builds itself
(int_signal), which shravan's encoder must reproduce byte for byte. With
build/interop-cli present (scripts/interop.sh builds it) the generator checks
that shravan does.

Each field holds one frame per ';'-separated segment, the frame's values packed
as little-endian 32-bit words (ints, or floats as their f32 bit patterns) in
base64: half the size of decimal text, and the test harness has 2 MB for all its
string data. Needs git (without --opus-src), cmake, a C compiler and numpy.
"""
import argparse
import base64
import os
import shutil
import struct
import subprocess
import sys
import tempfile

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "src", "silk_enc_vectors.cyr")
HERE = os.path.join(ROOT, "scripts", "silkref")


def run(*args, **kw):
    return subprocess.run(args, check=True, capture_output=True, **kw)


def require(cond, what):
    if not cond:
        sys.exit("gen-silk-vectors: " + what)


def build(src, work):
    tree = os.path.join(work, "opus")
    if src:
        shutil.copytree(src, tree)
    else:
        run("git", "clone", "--quiet", "--depth", "1", "--branch", "v1.6.1",
            "https://github.com/xiph/opus.git", tree)
    run(sys.executable, "-E", "-P", os.path.join(HERE, "patch.py"), tree)
    bld = os.path.join(work, "build")
    run("cmake", "-S", tree, "-B", bld, "-DCMAKE_BUILD_TYPE=Release", "-DOPUS_DRED=OFF", "-DOPUS_OSCE=OFF")
    run("cmake", "--build", bld, "-j%d" % (os.cpu_count() or 4))
    exe = os.path.join(work, "silkref")
    inc = []
    for d in ("include", "silk", "silk/float", "celt", ""):
        inc += ["-I", os.path.join(tree, d)]
    run("cc", "-O2", "-DOPUS_BUILD", "-DUSE_ALLOCA", *inc, "-o", exe,
        os.path.join(HERE, "silkref.c"), os.path.join(bld, "libopus.a"), "-lm")
    probe = os.path.join(work, "nsqprobe")
    run("cc", "-O2", "-DOPUS_BUILD", "-DUSE_ALLOCA", *inc, "-o", probe,
        os.path.join(HERE, "nsqprobe.c"), os.path.join(bld, "libopus.a"), "-lm")
    return exe, probe


def white(fs, seconds, seed):
    """Loud white noise: at 3 kbit/s CBR the rate loop raises Lambda past 2,
    the noise shaping quantiser's aggressive rate/distortion branch."""
    rng = np.random.default_rng(seed)
    x = rng.standard_normal(int(fs * seconds)) * 12000
    return np.clip(x, -32767, 32767).astype("<i2")


def noise(fs, seconds, seed):
    """Two-pole coloured noise under a slow envelope: unvoiced to the pitch
    analysis, with LPC and gains that move from frame to frame."""
    rng = np.random.default_rng(seed)
    n = int(fs * seconds)
    w = rng.standard_normal(n)
    y = np.zeros(n)
    for i in range(2, n):
        y[i] = w[i] + 1.2 * y[i - 1] - 0.5 * y[i - 2]
    env = 0.3 + 0.7 * (0.5 + 0.5 * np.sin(2 * np.pi * 1.3 * np.arange(n) / fs))
    y = y / np.abs(y).max() * 0.5 * env
    return np.round(y * 32767).astype("<i2")


def vowel(fs, seconds, seed):
    """A sung vowel: a pulse train at 110-150 Hz with vibrato through three
    formant resonators, in syllables of 0.3 s between 0.1 s of near silence,
    with a little breath noise. libopus finds voiced, unvoiced and inactive
    frames in it."""
    rng = np.random.default_rng(seed)
    n = int(fs * seconds)
    t = np.arange(n) / fs
    f0 = 130 + 20 * np.sin(2 * np.pi * 0.7 * t) + 3 * np.sin(2 * np.pi * 5.5 * t)
    ph = np.cumsum(f0 / fs)
    src = np.zeros(n)
    src[np.nonzero(np.diff(np.floor(ph)) > 0)[0] + 1] = 1.0
    y = src + 0.02 * rng.standard_normal(n)
    for fc, bw in ((700, 110), (1220, 120), (2600, 160)):
        if fc >= fs / 2:
            continue
        r = np.exp(-np.pi * bw / fs)
        a1, a2 = -2 * r * np.cos(2 * np.pi * fc / fs), r * r
        out = np.zeros(n)
        for i in range(n):
            out[i] = y[i] - a1 * (out[i - 1] if i > 0 else 0) - a2 * (out[i - 2] if i > 1 else 0)
        y = out
    syl = (np.mod(t, 0.4) < 0.3).astype(float)
    k = int(0.01 * fs)
    env = np.convolve(syl, np.ones(k) / k, mode="same")
    y = y * (0.02 + 0.98 * env)
    y = y / np.abs(y).max() * 0.6
    return np.round(y * 32767).astype("<i2")


def int_signal(kind, fs, n, seed):
    """Integer-only test input, so that src/silk_tests.cyr (_skk_signal) builds
    the same samples itself: a 32-bit LCG, four draws summed. Kind 0 through a
    two-pole resonator (1.2, -0.5) under a 1.3 Hz triangle envelope (0.3 to 1);
    2 the same with every other quarter second digital silence; 4 through a
    sharp resonance (1.9, -0.98: 25 dB of prediction gain); white at three
    levels: 1 loud (its gains reach their ceiling), 5 at -6 dB, 3 at -18 dB."""
    s = seed & 0xFFFFFFFF
    y1 = y2 = 0
    P = fs * 10 // 13
    out = []
    for i in range(n):
        w = 0
        for _ in range(4):
            s = (s * 1664525 + 1013904223) & 0xFFFFFFFF
            w += (s >> 16) - 32768
        if kind == 1:
            x = (w * 5) >> 4
        elif kind == 3:
            x = (w * 5) >> 7
        elif kind == 5:
            x = (w * 5) >> 5
        elif kind == 4:
            y = w + ((31130 * y1 - 16056 * y2) >> 14)
            y2, y1 = y1, y
            x = y >> 6
        else:
            y = w + ((19661 * y1 - 8192 * y2) >> 14)
            y2, y1 = y1, y
            ph = i % P
            tri = ph if ph < P // 2 else P - ph
            env = 19661 + 45875 * 2 * tri // P
            x = (y * env) >> 20
            if kind == 2 and i % (fs // 2) >= fs // 4:
                x = 0
        out.append(max(-32768, min(32767, x)))
    return np.array(out, dtype="<i2")


def fnv_i16(x):
    """FNV-1a over the samples as 32-bit words, as _skk_signal hashes them."""
    h = 1469598103934665603
    for v in x:
        h = ((h ^ (int(v) & 0xFFFFFFFF)) * 1099511628211) & 0xFFFFFFFFFFFFFFFF
    return h


def read_packets(path):
    d = open(path, "rb").read()
    out, p = [], 0
    while p + 4 <= len(d):
        n = struct.unpack_from("<I", d, p)[0]
        out.append(d[p + 4:p + 4 + n])
        p += 4 + n
    return out


# 2.11.3: whole packets. name, kind, seed, rate, packet ms, complexity, bit rate,
# CBR, input ms, buffer (VBR's budget). Between them: every bandwidth, packet
# length and complexity, voice-inactive frames (k10, k11, k12), and every
# branch of the rate loop: k6 brackets and interpolates, k7 and k8 raise Lambda
# and lock gains, k8 falls back to damage control every frame, k9 and k15 split
# a CBR budget over frames; VBR in a small buffer (k11, k12) takes the 25%
# margin, Lambda's floor, and the 3/5, 2/5 and 3/4 shares of multi-frame
# packets; k14's reservoir pushes the target under 5 kbit/s; k13's first frame
# holds the prediction gain to 100 (only 200 ms of it: on a resonance this
# sharp Burg is ill-conditioned, and libopus's float input to it soon parts
# from shravan's doubles). The seeds are ones for which libopus codes every
# frame unvoiced (shravan has no pitch analysis yet) and its float stages
# round as shravan's double ones do (about one frame in 500 rounds a shaping
# coefficient the other way, and the pulses can follow).
PACKETS = [
    ("k0", 0, 1, 16000, 20, 1, 20000, 0, 1000, 1275),
    ("k1", 0, 1, 12000, 20, 1, 20000, 0, 1000, 1275),
    ("k2", 0, 1, 8000, 20, 0, 16000, 0, 1000, 1275),
    ("k3", 0, 1, 16000, 10, 1, 20000, 0, 1000, 1275),
    ("k4", 0, 1, 16000, 40, 1, 20000, 0, 1000, 1275),
    ("k5", 0, 1, 8000, 60, 1, 20000, 0, 1200, 1275),
    ("k6", 0, 1, 16000, 20, 1, 12000, 1, 1000, 1275),
    ("k7", 1, 2, 16000, 20, 1, 3000, 1, 1000, 1275),
    ("k8", 1, 2, 12000, 10, 0, 1200, 1, 600, 1275),
    ("k9", 0, 2, 16000, 60, 1, 8000, 1, 1200, 1275),
    ("k10", 2, 1, 16000, 20, 1, 20000, 0, 1000, 1275),
    ("k11", 0, 1, 8000, 40, 1, 16000, 0, 1000, 40),
    ("k12", 0, 1, 16000, 60, 1, 20000, 0, 1200, 90),
    ("k13", 4, 2, 16000, 20, 1, 20000, 0, 200, 1275),
    ("k14", 5, 1, 8000, 20, 1, 5000, 0, 1000, 1275),
    ("k15", 3, 1, 12000, 40, 0, 3000, 1, 1000, 1275),
]


def parse(path):
    """Frames from vad.frame on, each the records of its stages (the first of
    each name; Burg calls, which repeat, as a list)."""
    frames, cur = [], None
    for line in open(path):
        t, *v = line.split()
        if t == "vad.frame":
            cur = {"burg": [], "nsq": []}
            frames.append(cur)
        if cur is None:
            continue
        if t == "burg.hdr":
            cur["burg"].append({})
        if t == "nsq.hdr":
            cur["nsq"].append({})
        if t.startswith("burg."):
            cur["burg"][-1][t] = v
        elif t.startswith("nsq."):
            cur["nsq"][-1][t] = v
        elif t not in cur:
            cur[t] = v
    return frames


def parse_calls(path):
    """The quantiser calls of a dump with no frames around them (nsqprobe)."""
    calls = []
    for line in open(path):
        t, *v = line.split()
        if t == "nsq.hdr":
            calls.append({})
        if t.startswith("nsq.") and calls:
            calls[-1][t] = v
    return calls


def pack_i(vals):
    return base64.b64encode(struct.pack("<%di" % len(vals), *(int(v) for v in vals))).decode()


def pack_f(vals):
    return base64.b64encode(struct.pack("<%df" % len(vals), *(float(v) for v in vals))).decode()


class Writer:
    def __init__(self):
        self.parts = []

    def field(self, name, frames, get, kind):
        segs = [(pack_f if kind == "f" else pack_i)(get(f)) for f in frames]
        self.parts.append('var %s = "%s";\n' % (name, ";".join(segs)))

    def int(self, name, v):
        self.parts.append("var %s = %d;\n" % (name, v))


def pick_lpc(frames):
    """The first frame, one of each signal type and interpolation index, and the last."""
    chosen = [0]
    for key, vals in (("fpc.signalType", ("0", "1")), ("fpc.interp", ("0", "1", "2", "3", "4"))):
        for v in vals:
            for i, f in enumerate(frames):
                if f[key][0] == v and i not in chosen:
                    chosen.append(i)
                    break
    chosen.append(len(frames) - 1)
    return [frames[i] for i in sorted(set(chosen))]


def pick_shape(frames, n):
    """Frames of each signal type and coding mode (independent, conditional)."""
    chosen = []
    for sig in ("0", "1", "2"):
        hits = [i for i, f in enumerate(frames) if f["nsa.hdr"][3] == sig]
        chosen += hits[:n]
    for cond in ("0", "2"):
        hits = [i for i, f in enumerate(frames) if f["pg.hdr"][5] == cond and i not in chosen]
        chosen += hits[:1]
    return [frames[i] for i in sorted(set(chosen))]


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--opus-src", help="a libopus v1.6.1 source tree (default: clone it)")
    args = ap.parse_args()
    configs = [  # name, signal, fs, frame ms, complexity, bit rate, CBR
        ("nwb20c1", noise, 16000, 20, 1),
        ("nwb20c5", noise, 16000, 20, 5),
        ("nmb20c1", noise, 12000, 20, 1),
        ("nnb20c5", noise, 8000, 20, 5),
        ("nwb10c1", noise, 16000, 10, 1),
        ("vwb20c1", vowel, 16000, 20, 1),
        ("vnb10c1", vowel, 8000, 10, 1),
        ("vwb40c1", vowel, 16000, 40, 1),     # two frames a packet: the second coded conditionally
        ("wwb20cbr", white, 16000, 20, 1, 3000, 1),
    ]
    runs = {}
    pk_sets = []
    cli = os.path.join(ROOT, "build", "interop-cli")
    with tempfile.TemporaryDirectory() as work:
        exe, probe = build(args.opus_src, work)
        for name, kind, seed, fs, ms, cx, br, cbr, total, cap in PACKETS:
            x = int_signal(kind, fs, fs * total // 1000, seed)
            pcm = os.path.join(work, name + ".s16")
            x.tofile(pcm)
            pk = os.path.join(work, name + ".pk")
            dump = os.path.join(work, name + ".dump")
            run(exe, pcm, pk, str(fs), str(br), str(cx), str(ms), str(cbr), str(cap), env=dict(os.environ, SILKDUMP=dump))
            voiced = sum(1 for line in open(dump) if line.startswith("nsq.hdr ") and line.split()[8] == "2")
            require(voiced == 0, name + ": libopus coded %d frames voiced" % voiced)
            pks = read_packets(pk)
            require(len(pks) == total // ms, name + ": packet count")
            require(max(map(len, pks)) <= cap, name + ": a packet over its buffer")
            if os.path.exists(cli):
                f64 = os.path.join(work, name + ".f64")
                (x.astype(np.float64) / 32768).astype("<f8").tofile(f64)
                mine = os.path.join(work, name + ".shr")
                run(cli, "silkenc", f64, str(fs), str(br), str(cx), str(ms), str(cbr), mine, "raw", str(cap))
                diff = [i for i, (a, b) in enumerate(zip(pks, read_packets(mine))) if a != b]
                require(not diff, name + ": shravan's packets %s differ from libopus's" % diff[:8])
            h = fnv_i16(x)
            pk_sets.append(([kind, seed, fs, ms, cx, br, cbr, len(x), len(pks),
                             struct.unpack("<i", struct.pack("<I", h & 0xFFFFFFFF))[0],
                             struct.unpack("<i", struct.pack("<I", h >> 32))[0], cap], pks))
        if not os.path.exists(cli):
            print("note: no build/interop-cli, shravan's packets not checked")
        pdump = os.path.join(work, "probe.dump")
        run(probe, env=dict(os.environ, SILKDUMP=pdump))
        probe_calls = parse_calls(pdump)
        for name, sig, fs, ms, cx, *rate in configs:
            pcm = os.path.join(work, name + ".s16")
            sig(fs, 2.0, {noise: 7, vowel: 11, white: 3}[sig]).tofile(pcm)
            dump = os.path.join(work, name + ".dump")
            br, cbr = rate if rate else (20000, 0)
            run(exe, pcm, os.path.join(work, name + ".pk"), str(fs), str(br), str(cx), str(ms), str(cbr),
                env=dict(os.environ, SILKDUMP=dump))
            runs[name] = parse(dump)
            require(len(runs[name]) >= 50, name + ": too few frames")

    w = Writer()
    w.parts.append("# silk_enc_vectors.cyr -- libopus SILK encoder stage vectors (scripts/gen-silk-vectors.py).\n"
                   "# Generated: do not edit. One frame a ';'-separated segment, its values little-endian\n"
                   "# 32-bit words (ints, or floats as f32 bit patterns) in base64.\n\n")

    # 2.11.0: the unvoiced prediction path, from the noise runs
    sel = []
    for name in ("nwb20c1", "nwb20c5", "nmb20c1", "nnb20c5", "nwb10c1"):
        sel += pick_lpc(runs[name])
    require({f["fpc.signalType"][0] for f in sel} == {"0", "1"}, "LPC set: signal types 0 and 1")
    require({"0", "1", "2", "3", "4"} <= {f["fpc.interp"][0] for f in sel}, "LPC set: every interpolation index")
    require({f["fpc.nb_subfr"][0] for f in sel} == {"2", "4"}, "LPC set: 10 and 20 ms frames")
    require({f["fpc.fs_kHz"][0] for f in sel} == {"8", "12", "16"}, "LPC set: all three internal rates")
    require(any(f["fpc.first"][0] == "1" for f in sel), "LPC set: a first frame")
    w.int("_SKV_N", len(sel))
    w.field("_SKV_HDR", sel, lambda f: [f[k][0] for k in (
        "fpc.signalType", "fpc.nb_subfr", "fpc.subfr_length", "fpc.order", "fpc.first", "fpc.useinterp",
        "fpc.survivors", "fpc.fs_kHz", "fpc.speech_q8", "fpc.interp")] + [len(f["burg"])], "i")
    w.field("_SKV_CQ", sel, lambda f: f["fpc.coding_quality"], "f")
    w.field("_SKV_MININV", sel, lambda f: f["fpc.mininvgain"], "f")
    for key, name in (("fpc.gains", "GAINS"), ("fpc.x", "X"), ("fpc.lpcinpre", "PRE"), ("fpc.pred0", "PRED0"),
                      ("fpc.pred1", "PRED1"), ("fpc.resnrg", "RESNRG")):
        w.field("_SKV_" + name, sel, lambda f, k=key: f[k], "f")
    for key, name in (("fpc.prevnlsf", "PREV"), ("fpc.nlsf_unq", "NLSFU"), ("fpc.nlsf_q", "NLSFQ"),
                      ("fpc.nlsfidx", "NLSFIDX")):
        w.field("_SKV_" + name, sel, lambda f, k=key: f[k], "i")
    w.field("_SKV_BA", sel, lambda f: [x for b in f["burg"] for x in b["burg.A"]], "f")
    w.field("_SKV_BNRG", sel, lambda f: [b["burg.nrg"][0] for b in f["burg"]], "f")

    # 2.11.1: the VAD, as sequences from a fresh encoder
    seqs = [("vwb20c1", 24), ("vnb10c1", 40)]
    vad = []
    for name, n in seqs:
        vad += runs[name][:n]
    require({f["vad.signalType"][0] for f in vad} == {"0", "1"}, "VAD set: active and inactive frames")
    w.int("_SKA_NSEQ", len(seqs))
    w.field("_SKA_SEQ", [None], lambda f: [x for name, n in seqs for x in (
        n, runs[name][0]["vad.fs"][0], runs[name][0]["vad.fl"][0])], "i")
    w.field("_SKA_IN", vad, lambda f: f["vad.in"], "i")
    w.field("_SKA_OUT", vad, lambda f: [f["vad.speech"][0], f["vad.tilt"][0]] + f["vad.quality"]
            + [f["vad.signalType"][0]], "i")

    # 2.11.1: pitch front end, noise shaping and gains
    shp = []
    for name in ("vwb20c1", "vnb10c1"):
        shp += pick_shape(runs[name], 2)
    shp += [f for f in runs["vwb40c1"] if f["pg.hdr"][5] == "2"][:1]
    shp += pick_shape(runs["nwb20c5"], 1)[:2]
    shp += pick_shape(runs["nnb20c5"], 1)[:1]
    require({f["nsa.hdr"][3] for f in shp} == {"0", "1", "2"}, "shape set: every signal type")
    require(any(int(f["nsa.hdr"][10]) > 0 for f in shp), "shape set: the warped analysis")
    require({f["pg.hdr"][5] for f in shp} >= {"0", "2"}, "shape set: independent and conditional gains")
    require({f["nsa.hdr"][5] for f in shp} == {"2", "4"}, "shape set: 10 and 20 ms frames")
    require({f["nsa.hdr"][4] for f in shp} == {"8", "16"}, "shape set: narrow and wide band")
    w.int("_SKS_N", len(shp))
    w.field("_SKS_FPL_HDR", shp, lambda f: f["fpl.hdr"], "i")
    w.field("_SKS_FPL_X", shp, lambda f: f["fpl.x"], "f")
    w.field("_SKS_FPL_OUT", shp, lambda f: f["fpl.predgain"] + f["fpl.res"], "f")
    w.field("_SKS_NSA_HDR", shp, lambda f: f["nsa.hdr"] + f["nsa.quality"] + f["nsa.pitchL"], "i")
    w.field("_SKS_NSA_IN", shp, lambda f: f["nsa.predgain"] + f["nsa.ltpcorr"] + f["nsa.harm_smth"]
            + f["nsa.tilt_smth"], "f")
    w.field("_SKS_NSA_X", shp, lambda f: f["nsa.x"], "f")
    w.field("_SKS_NSA_RES", shp, lambda f: f["nsa.pitchres"], "f")
    w.field("_SKS_NSA_QOFF", shp, lambda f: f["nsa.qoffset"], "i")
    w.field("_SKS_NSA_OUT", shp, lambda f: f["nsa.input_quality"] + f["nsa.coding_quality"]
            + f["nsa.harm_smth_out"] + f["nsa.tilt_smth_out"] + f["nsa.gains"] + f["nsa.lfma"]
            + f["nsa.lfar"] + f["nsa.tilt"] + f["nsa.harm"], "f")
    w.field("_SKS_NSA_AR", shp, lambda f: f["nsa.AR"], "f")
    w.field("_SKS_PG_HDR", shp, lambda f: f["pg.hdr"], "i")
    w.field("_SKS_PG_IN", shp, lambda f: f["pg.ltpred"] + f["pg.input_quality"] + f["pg.coding_quality"]
            + f["pg.gains_in"] + f["pg.resnrg"], "f")
    w.field("_SKS_PG_IOUT", shp, lambda f: f["pg.gainidx"] + f["pg.lastgain"] + f["pg.gainsunq"]
            + f["pg.lastgainprev"] + f["pg.qoffset"], "i")
    w.field("_SKS_PG_FOUT", shp, lambda f: f["pg.gains_out"] + f["pg.lambda"], "f")

    # 2.11.2: the noise shaping quantiser, as sequences from a fresh encoder (one
    # call a frame in VBR), and calls in the aggressive rate/distortion branch
    # (Lambda_Q10 > 2048, inside a CBR rate loop) each from its own input state
    nseq = [("vwb20c1", 16), ("vnb10c1", 40)]
    calls = []
    for name, n in nseq:
        for f in runs[name][:n]:
            require(len(f["nsq"]) == 1, name + ": one quantiser call a frame")
            calls.append(f["nsq"][0])
    require({c["nsq.hdr"][7] for c in calls} == {"0", "1", "2"}, "NSQ set: every signal type")
    agg = [c for f in runs["wwb20cbr"] for c in f["nsq"] if int(c["nsq.hdr"][10]) > 2048]
    agg = sorted(agg, key=lambda c: -int(c["nsq.hdr"][10]))[:6]
    require(len(agg) == 6, "NSQ set: calls with Lambda_Q10 above 2048")
    # libopus's quantiser called directly (scripts/silkref/nsqprobe.c): large
    # residuals at high Lambda, which no encoder run produces
    require(len(probe_calls) == 4 and all(int(c["nsq.hdr"][10]) > 2048 for c in probe_calls), "NSQ probe calls")
    big = max(abs(int(v)) for c in probe_calls for v in c["nsq.pulses"])
    require(2 < big < 25, "NSQ probe: pulses of a few levels, unsaturated (%d)" % big)
    agg += probe_calls

    def nsq_fields(prefix, cs, full):
        w.int(prefix + "_N", len(cs))
        w.field(prefix + "_HDR", cs, lambda c: c["nsq.hdr"], "i")
        w.field(prefix + "_X16", cs, lambda c: c["nsq.x16"], "i")
        w.field(prefix + "_PAR", cs, lambda c: c["nsq.pred"] + c["nsq.ltp"] + c["nsq.harm"] + c["nsq.tilt"]
                + c["nsq.lf"] + c["nsq.gains"] + c["nsq.pitchL"], "i")
        w.field(prefix + "_AR", cs, lambda c: c["nsq.ar"], "i")
        w.field(prefix + "_PULSES", cs, lambda c: c["nsq.pulses"], "i")
        w.field(prefix + "_HASH", cs, lambda c: c["nsq.hash_in"] + c["nsq.hash_out"], "i")
        if full:
            w.field(prefix + "_STATE", cs, lambda c: c["nsq.st_xq"] + c["nsq.st_shp"] + c["nsq.st_lpc"]
                    + c["nsq.st_ar2"] + c["nsq.st_sc"], "i")

    w.int("_SKQ_NSEQ", len(nseq))
    w.field("_SKQ_SEQ", [None], lambda f: [n for name, n in nseq], "i")
    nsq_fields("_SKQ", calls, False)
    nsq_fields("_SKR", agg, True)

    # 2.11.2: the float -> fixed conversion, against the quantiser's inputs
    conv = [f for name, n in nseq for f in runs[name][:n]][::4]
    w.int("_SKW_N", len(conv))
    w.field("_SKW_HDR", conv, lambda f: f["nsw.hdr"], "i")
    w.field("_SKW_IN", conv, lambda f: f["nsw.lfar"] + f["nsw.lfma"] + f["nsw.tilt"] + f["nsw.harm"]
            + f["nsw.lambda"] + f["nsw.ltp"] + f["nsw.pred"] + f["nsw.gains"], "f")
    w.field("_SKW_AR", conv, lambda f: f["nsw.ar"], "f")
    w.field("_SKW_X", conv, lambda f: f["nsw.x"], "f")
    w.field("_SKW_HDRQ", conv, lambda f: f["nsq"][0]["nsq.hdr"], "i")
    w.field("_SKW_PAR", conv, lambda f: f["nsq"][0]["nsq.pred"] + f["nsq"][0]["nsq.ltp"] + f["nsq"][0]["nsq.harm"]
            + f["nsq"][0]["nsq.tilt"] + f["nsq"][0]["nsq.lf"] + f["nsq"][0]["nsq.gains"], "i")
    w.field("_SKW_ARQ", conv, lambda f: f["nsq"][0]["nsq.ar"], "i")
    w.field("_SKW_X16", conv, lambda f: f["nsq"][0]["nsq.x16"], "i")

    # 2.11.3: whole packets for inputs the suite builds (one config a segment:
    # its header, then its packets as u32le length and bytes, in base64)
    w.int("_SKK_N", len(pk_sets))
    w.field("_SKK_HDR", pk_sets, lambda c: c[0], "i")
    w.parts.append('var _SKK_PK = "%s";\n' % ";".join(
        base64.b64encode(b"".join(struct.pack("<I", len(p)) + p for p in c[1])).decode() for c in pk_sets))

    open(OUT, "w").write("".join(w.parts))
    print("wrote %s (LPC %d, VAD %d, shape %d frames, NSQ %d + %d calls, conversion %d, packets %d; %d bytes)" % (
        OUT, len(sel), len(vad), len(shp), len(calls), len(agg), len(conv),
        sum(len(c[1]) for c in pk_sets), os.path.getsize(OUT)))


if __name__ == "__main__":
    main()
