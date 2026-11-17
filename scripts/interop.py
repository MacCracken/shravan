#!/usr/bin/env python3
"""Sweep shravan against reference decoders and encoders, in both directions.

The in-suite fixtures (src/interop_fixtures.cyr) pin a few dozen streams. This sweep
crosses the matrix: sample rate x bit depth x channel count for every format, real
encoder output decoded by shravan and judged against ffmpeg, flac, mpg123 or libopus,
and shravan's encoders judged by what those references make of their output.

    ./scripts/interop.sh            # build the tools, run everything
    ./scripts/interop.py --only flac,opus

Lossless paths must match exactly, and WAV, AIFF and FLAC must match again through
the streaming decoders, fed in odd-sized pieces. Lossy decoders are judged by the maximum sample
error in 16-bit LSB against the reference (float rounding is well under 1 LSB);
lossy encoders by the correlation of the reference's decode with the input.

Needs ffmpeg (libmp3lame, libopus), flac, lame, twolame, mpg123, and
build/interop-cli + build/opusref (scripts/interop.sh builds both).
"""
import argparse
import array
import math
import os
import struct
import shutil
import subprocess
import sys
import tempfile
import wave

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CLI = os.path.join(ROOT, "build", "interop-cli")
OPUSREF = os.path.join(ROOT, "build", "opusref")

# Bursts at different times per channel, a sweep and noise: transients force short
# blocks and independent windows, noise keeps every band busy.
def signal(rate, ch, dur, amp=0.4, sweep=None):
    """sweep: the chirp's rise per second (default rate / 5)."""
    chans = []
    for c in range(ch):
        f = 220 * (c + 2)
        chans.append(
            f"{amp}*sin(2*PI*{f}*t)*(0.6+0.4*exp(-20*mod(t+{0.03 * c:.2f}\\,0.17)))"
            f"+0.08*sin(2*PI*(300+{rate // 5 if sweep is None else sweep}*mod(t\\,1))*t)+0.04*random({c})")
    return "aevalsrc=" + "|".join(chans) + f":s={rate}:d={dur}"


def run(*args, check=True):
    return subprocess.run(args, capture_output=True, check=check)


def f64(path):
    a = array.array("d")
    with open(path, "rb") as fh:
        raw = fh.read()
    a.frombytes(raw[: len(raw) // 8 * 8])
    return a


def ff_decode(src, dst, *pre, post=()):
    run("ffmpeg", "-v", "error", "-y", *pre, "-i", src, *post, "-f", "f64le", "-c:a", "pcm_f64le", dst)
    return f64(dst)


def mp4_audio_tracks(path):
    """The audio tracks of an MP4 (ISO 14496-12), in file order: for each, the
    tkhd enabled flag, the mdhd timescale, the edit list (segment duration in the
    movie's timescale, media time, rate) and the sum of the stts durations."""
    d = open(path, "rb").read()

    def boxes(s, e):
        p = s
        while p + 8 <= e:
            size, t = struct.unpack(">I4s", d[p:p + 8])
            hdr = 8
            if size == 1:
                size = struct.unpack(">Q", d[p + 8:p + 16])[0]
                hdr = 16
            if size == 0:
                size = e - p
            yield t, p + hdr, p + size
            p += size

    def child(s, e, path):
        for t, bs, be in boxes(s, e):
            if t == path[0]:
                return (bs, be) if len(path) == 1 else child(bs, be, path[1:])
        return None

    moov = child(0, len(d), [b"moov"])
    mvhd = child(*moov, [b"mvhd"])
    movie_ts = struct.unpack(">I", d[mvhd[0] + (20 if d[mvhd[0]] == 1 else 12):][:4])[0]
    tracks = []
    for t, ts_, te in boxes(*moov):
        if t != b"trak":
            continue
        hdlr = child(ts_, te, [b"mdia", b"hdlr"])
        if d[hdlr[0] + 8:hdlr[0] + 12] != b"soun":
            continue
        tkhd = child(ts_, te, [b"tkhd"])
        mdhd = child(ts_, te, [b"mdia", b"mdhd"])
        ts = struct.unpack(">I", d[mdhd[0] + (20 if d[mdhd[0]] == 1 else 12):][:4])[0]
        stts = child(ts_, te, [b"mdia", b"minf", b"stbl", b"stts"])
        n = struct.unpack(">I", d[stts[0] + 4:stts[0] + 8])[0]
        total = sum(c * dl for c, dl in (struct.unpack(">II", d[stts[0] + 8 + i * 8:stts[0] + 16 + i * 8])
                                         for i in range(n)))
        edits = []
        elst = child(ts_, te, [b"edts", b"elst"])
        if elst:
            v = d[elst[0]]
            for i in range(struct.unpack(">I", d[elst[0] + 4:elst[0] + 8])[0]):
                q = elst[0] + 8 + i * (20 if v == 1 else 12)
                dur, mt, rate = struct.unpack(">QqI" if v == 1 else ">IiI", d[q:q + (20 if v == 1 else 12)])
                edits.append((dur, mt, rate >> 16))
        tracks.append({"enabled": d[tkhd[0] + 3] & 1, "ts": ts, "edits": edits, "stts": total})
    return movie_ts, tracks


def mp4_iso_ref(path, dst, index, ch):
    """Track `index` as ISO 14496-12 presents it: ffmpeg's decode without the edit
    list (every ffmpeg gives the same), cut to the sample table's length, then to
    each edit's media range in order (empty and dwell edits give nothing). ffmpeg
    itself trims an MP4's last frame to the edit list only from 9.0; older ones
    (Ubuntu 24.04 has 6.1) play the padding, so this cut is the reference, and
    with ffmpeg 9 it equals ffmpeg's own default decode."""
    movie_ts, tracks = mp4_audio_tracks(path)
    tk = tracks[index]
    raw = ff_decode(path, dst, "-ignore_editlist", "1", post=("-map", f"0:a:{index}"))
    media = min(len(raw) // ch, tk["stts"])
    ranges = []
    for dur, mt, rate in tk["edits"]:
        if mt >= 0 and dur > 0 and rate != 0 and mt < media:
            ranges.append((mt, min((dur * tk["ts"] + movie_ts // 2) // movie_ts, media - mt)))
    if not tk["edits"]:
        ranges = [(0, media)]
    out = array.array("d")
    for s0, n0 in ranges:
        out.extend(raw[s0 * ch:(s0 + n0) * ch])
    return out


def ffmpeg_default_track(path, channels):
    """ffmpeg's default audio stream: enabled tracks first, then the most channels,
    then the first (fftools/ffmpeg_mux_init.c, map_auto_audio)."""
    _, tracks = mp4_audio_tracks(path)
    scores = [channels[i] + 5000000 * tk["enabled"] for i, tk in enumerate(tracks)]
    return scores.index(max(scores))


def shravan_decode(src, dst):
    p = run(CLI, "decode", src, dst, check=False)
    if p.returncode != 0:
        return None, p.stdout.decode().strip()
    return f64(dst), p.stdout.decode().split()


def shravan_stream(kind, src, dst, feed):
    p = run(CLI, "stream", kind, src, dst, str(feed), check=False)
    if p.returncode != 0:
        return None, p.stdout.decode().strip()
    return f64(dst), p.stdout.decode().split()


def maxerr(a, b, scale=32768.0):
    n = min(len(a), len(b))
    return max((abs(a[i] - b[i]) for i in range(n)), default=0.0) * scale


def corr(a, b):
    n = min(len(a), len(b))
    sab = sum(a[i] * b[i] for i in range(n))
    saa = sum(a[i] * a[i] for i in range(n))
    sbb = sum(b[i] * b[i] for i in range(n))
    return sab / math.sqrt(saa * sbb) if saa > 0 and sbb > 0 else 0.0


def best_lag_corr(ref, got, ch, maxlag=4096, warm=2048, minlag=None):
    """Correlation of got against ref at the lag (in frames, either direction, or
    minlag..maxlag) that maximises it, past the first and last `warm` frames (codec
    start-up)."""
    best = (0.0, 0)
    for lag in range(-maxlag if minlag is None else minlag, maxlag + 1):
        a, b = (ref, got[lag * ch:]) if lag >= 0 else (ref[-lag * ch:], got)
        n = min(len(a), len(b)) - warm * ch
        if n <= warm * ch:
            continue
        c = corr(a[warm * ch:n], b[warm * ch:n])
        if c > best[0]:
            best = (c, lag)
    return best


class Sweep:
    def __init__(self, tmp, verbose):
        self.tmp, self.verbose = tmp, verbose
        self.results = []

    def p(self, name):
        return os.path.join(self.tmp, name)

    def record(self, group, name, ok, detail):
        self.results.append((group, name, ok, detail))
        if self.verbose or not ok:
            print(f"  {'ok  ' if ok else 'FAIL'} {group:6s} {name:44s} {detail}")

    # -- decode direction -------------------------------------------------------

    def decode_vs(self, group, name, path, ref, lossless, tol=2.0, ref_scale=32768.0):
        got, info = shravan_decode(path, self.p("got.f64"))
        if got is None:
            self.record(group, name, False, f"shravan error: {info}")
            return
        e = maxerr(ref, got, ref_scale)
        same_len = len(ref) == len(got)
        ok = same_len and (e == 0 if lossless else e <= tol)
        self.record(group, name, ok, f"n={len(got)}/{len(ref)} maxerr={e:.3f}")
        if group in ("wav", "aiff", "flac") and lossless:
            # The same file through the streaming decoder, split at an odd size so
            # pieces end mid-sample and mid-frame. Before 2.9.3 a WAV or AIFF stream
            # returned only the first piece's samples.
            got, info = shravan_stream(group, path, self.p("st.f64"), 1009)
            if got is None:
                self.record(group, name + " (stream)", False, f"shravan error: {info}")
                return
            e = maxerr(ref, got, ref_scale)
            ok = len(ref) == len(got) and e == 0
            self.record(group, name + " (stream)", ok, f"n={len(got)}/{len(ref)} maxerr={e:.3f}")

    def wav(self):
        fmts = [("u8", "pcm_u8", 8), ("s16", "pcm_s16le", 16), ("s24", "pcm_s24le", 24),
                ("s32", "pcm_s32le", 32), ("f32", "pcm_f32le", 0), ("f64", "pcm_f64le", 0)]
        for rate in (8000, 44100, 96000, 192000):
            for ch in (1, 2, 6):
                for tag, codec, bits in fmts:
                    src = self.p(f"w.wav")
                    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.1),
                        "-c:a", codec, src)
                    ref = ff_decode(src, self.p("ref.f64"))
                    self.decode_vs("wav", f"{rate} {ch}ch {tag}", src, ref, True,
                                   ref_scale=float(1 << (bits - 1)) if bits else 1 << 52)

    def aiff(self):
        fmts = [("s8", "pcm_s8"), ("s16", "pcm_s16be"), ("s24", "pcm_s24be"), ("s32", "pcm_s32be"),
                ("f32/aifc", "pcm_f32be"), ("f64/aifc", "pcm_f64be"), ("sowt/aifc", "pcm_s16le")]
        for rate in (22050, 48000, 96000):
            for ch in (1, 2, 6):
                for tag, codec in fmts:
                    src = self.p("a.aiff")
                    p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.1),
                            "-c:a", codec, src, check=False)
                    if p.returncode != 0:
                        continue
                    ref = ff_decode(src, self.p("ref.f64"))
                    self.decode_vs("aiff", f"{rate} {ch}ch {tag}", src, ref, True, ref_scale=1 << 31)

    def flac(self):
        for bits in (8, 16, 24, 32):
            for ch in (1, 2, 3, 6, 8):
                for opts in (("-0",), ("-8",), ("-8", "--lax", "-b", "16384"), ("-5", "-b", "192")):
                    if bits == 32 and "16384" in opts:
                        continue
                    wav = self.p("in.wav")
                    codec = {8: "pcm_u8", 16: "pcm_s16le", 24: "pcm_s24le", 32: "pcm_s32le"}[bits]
                    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, ch, 0.15),
                        "-c:a", codec, wav)
                    out = self.p("o.flac")
                    p = run("flac", "-s", "-f", *opts, wav, "-o", out, check=False)
                    if p.returncode != 0:
                        self.record("flac", f"{bits}b {ch}ch {' '.join(opts)}", False, "flac refused")
                        continue
                    ref = ff_decode(out, self.p("ref.f64"))
                    self.decode_vs("flac", f"{bits}b {ch}ch {' '.join(opts)}", out, ref, True,
                                   ref_scale=float(1 << (bits - 1)))
        # Wasted bits: 16-bit content in a 24-bit container.
        wav = self.p("in.wav")
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(44100, 2, 0.2),
            "-af", "aformat=s16", "-c:a", "pcm_s24le", wav)
        run("flac", "-s", "-f", "-8", wav, "-o", self.p("w.flac"))
        ref = ff_decode(self.p("w.flac"), self.p("ref.f64"))
        self.decode_vs("flac", "24b container, 16b content (wasted bits)", self.p("w.flac"), ref, True,
                       ref_scale=1 << 23)
        # ffmpeg's encoder: different choices of order, partition and stereo mode.
        for lvl in ("0", "8", "12"):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(44100, 2, 0.2),
                "-c:a", "flac", "-compression_level", lvl, "-sample_fmt", "s16", self.p("ff.flac"))
            ref = ff_decode(self.p("ff.flac"), self.p("ref.f64"))
            self.decode_vs("flac", f"ffmpeg encoder level {lvl}", self.p("ff.flac"), ref, True)

    def alac(self):
        for fmt, bits in (("s16p", 16), ("s32p", 24)):
            for ch in (1, 2, 3, 4, 5, 6, 7, 8):
                for rate in (44100, 96000):
                    src = self.p("a.m4a")
                    p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.12),
                            "-c:a", "alac", "-sample_fmt", fmt, src, check=False)
                    if p.returncode != 0:
                        continue
                    ref = ff_decode(src, self.p("ref.f64"))
                    self.decode_vs("alac", f"{rate} {ch}ch {bits}b", src, ref, True,
                                   ref_scale=float(1 << (bits - 1)))

    def mp3(self):
        for rate in (8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000):
            for ch, mode in ((1, None), (2, "0"), (2, "1")):
                for br in ("vbr", "32k", "128k"):
                    src = self.p("m.mp3")
                    args = ["-c:a", "libmp3lame"]
                    args += ["-q:a", "4"] if br == "vbr" else ["-b:a", br]
                    if mode is not None:
                        args += ["-joint_stereo", mode]
                    p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.6),
                            "-ac", str(ch), *args, src, check=False)
                    if p.returncode != 0:
                        continue
                    # Gapless (2.10.2): the LAME tag's delay and padding trimmed, as
                    # ffmpeg plays it, to the sample.
                    ref = ff_decode(src, self.p("ref.f64"))
                    js = {None: "mono", "0": "stereo", "1": "joint"}[mode]
                    self.decode_vs("mp3", f"{rate} {js} {br}", src, ref, False)
        # Layer II from twolame (incl. joint stereo with intensity), against mpg123.
        for rate in (32000, 44100, 48000, 16000, 24000):
            for mode in ("m", "s", "j"):
                wav = self.p("in.wav")
                ch = 1 if mode == "m" else 2
                run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.5),
                    "-c:a", "pcm_s16le", wav)
                mp2 = self.p("t.mp2")
                p = run("twolame", "--quiet", "-m", mode, "-b", "96" if ch == 1 else "192", wav, mp2,
                        check=False)
                if p.returncode != 0:
                    continue
                raw = run("mpg123", "-q", "-s", mp2).stdout
                ref16 = array.array("h")
                ref16.frombytes(raw[: len(raw) // 2 * 2])
                ref = array.array("d", (x / 32768.0 for x in ref16))
                self.decode_vs("mp2", f"twolame {rate} -m {mode}", mp2, ref, False)

    def aac(self):
        for rate in (8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 88200, 96000):
            for ch in (1, 2, 3, 6):
                for container in ("adts", "m4a"):
                    src = self.p("a." + ("aac" if container == "adts" else "m4a"))
                    args = ["-c:a", "aac", "-aac_pns", "0", "-b:a", f"{48 * ch}k"]
                    if container == "adts":
                        args += ["-f", "adts"]
                    p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.5),
                            *args, src, check=False)
                    if p.returncode != 0:
                        continue
                    name = f"{rate} {ch}ch {container}"
                    if ch > 2:
                        # Multichannel AAC is refused until it is implemented (roadmap).
                        got, info = shravan_decode(src, self.p("got.f64"))
                        self.record("aac", name + " refused", got is None, f"{info}")
                        continue
                    # m4a: the edit list's priming and end trim applied (2.10.2), as
                    # ISO 14496-12 cuts it (see mp4_iso_ref).
                    if container == "m4a":
                        ref = mp4_iso_ref(src, self.p("ref.f64"), 0, ch)
                    else:
                        ref = ff_decode(src, self.p("ref.f64"))
                    self.decode_vs("aac", name, src, ref, False)

    def gapless(self):
        # MP4 track selection: ffmpeg's default audio stream (enabled tracks first,
        # then the most channels, then the first) and every track by index. AAC and
        # ALAC, each to the sample and to ffmpeg's length.
        srcs = [("sine=f=440:d=0.3:sample_rate=44100", 1),
                ("aevalsrc=0.3*sin(2*PI*660*t)|0.2*sin(2*PI*990*t):s=44100:d=0.25", 2),
                ("sine=f=550:d=0.2:sample_rate=44100", 1)]
        layouts = [(1, 1, 1), (0, 1, 1), (1, 0, 1), (0, 0, 1), (0, 0, 0), (1, 1, 0)]
        for codec in ("aac", "alac"):
            for lay in layouts:
                src = self.p("t.m4a")
                args = []
                for f, _ in srcs:
                    args += ["-f", "lavfi", "-i", f]
                for i in range(len(srcs)):
                    args += ["-map", f"{i}:a"]
                for i, en in enumerate(lay):
                    args += [f"-disposition:a:{i}", "default" if en else "0"]
                cargs = ["-c:a", "aac", "-aac_pns", "0", "-b:a", "64k"] if codec == "aac" else ["-c:a", "alac"]
                run("ffmpeg", "-v", "error", "-y", *args, *cargs, src)
                name = f"{codec} tracks enabled={''.join(map(str, lay))}"
                p = run(CLI, "tracks", src, check=False)
                fields = p.stdout.decode().split()
                if p.returncode != 0 or len(fields) < 2:
                    self.record("gapless", name, False, "shravan error")
                    continue
                chans = [c for _, c in srcs]
                want = ffmpeg_default_track(src, chans)
                ref = mp4_iso_ref(src, self.p("ref.f64"), want, chans[want])
                got, info = shravan_decode(src, self.p("got.f64"))
                lossless = codec == "alac"
                e = maxerr(ref, got) if got is not None else 1e9
                ok = (fields[0] == "3" and fields[1] == str(want) and got is not None and len(got) == len(ref)
                      and (e == 0 if lossless else e <= 2.0))
                self.record("gapless", name + " default", ok,
                            f"default={fields[1]} n={len(got) if got else 0}/{len(ref)} maxerr={e:.3f}")
                for i in range(len(srcs)):
                    ref = mp4_iso_ref(src, self.p("ref.f64"), i, chans[i])
                    q = run(CLI, "track", src, str(i), self.p("got.f64"), check=False)
                    got = f64(self.p("got.f64")) if q.returncode == 0 else None
                    e = maxerr(ref, got) if got is not None else 1e9
                    ok = got is not None and len(got) == len(ref) and (e == 0 if lossless else e <= 2.0)
                    self.record("gapless", f"{name} track {i}", ok, f"n={len(got) if got else 0}/{len(ref)} maxerr={e:.3f}")

    def opus(self):
        # libopus built with deep PLC (DRED or OSCE) skips the SILK PLC glue fade at
        # 16 kHz, which a default build applies and shravan follows: wideband SILK and
        # hybrid concealment is checked only against a default build. `opusref caps`
        # tells them apart by decoder size (the DRED entry points are exported as
        # stubs either way). OPUS_PREFIX in scripts/interop.sh links a default build.
        caps = run(OPUSREF, "caps").stdout.decode()
        deep = "deep_plc 1" in caps
        print("  " + caps.strip())
        cases = [(c, ch, ()) for c in range(32) for ch in (1, 2)]
        cases += [(9, 2, ("dur=2880", "fec=20")), (13, 1, ("fec=20",)), (9, 2, ("dur=2880", "sig=1")),
                  (31, 2, ("sw=3", "chsw=4")), (1, 2, ("bwsw=5",)), (0, 2, ("fsw=4",)),
                  (31, 2, ("bwsw=4",)), (31, 2, ("sw=2", "br=10000", "dur=1920")),
                  (5, 2, ("dtx=1", "sig=3", "voip=1")), (31, 1, ("dtx=1", "sig=3")),
                  (28, 2, ("lose=5",)), (5, 2, ("lose=5",)), (31, 2, ("swmb=1", "lose=2")),
                  (1, 2, ("auto=1", "br=20000", "dur=1920")), (1, 1, ("auto=1", "voip=1", "br=12000"))]
        if not deep:
            # A default libopus build: wideband SILK and hybrid concealment, every frame
            # size and both channel counts, under loss and DTX.
            for cfg in (8, 9, 10, 11, 12, 13, 14, 15):
                for ch in (1, 2):
                    cases += [(cfg, ch, ("lose=3",)), (cfg, ch, ("dtx=1", "sig=3", "voip=1"))]
            cases += [(9, 2, ("sw=3", "lose=4")), (13, 2, ("fec=20", "lose=4"))]
        else:
            print("  deep-PLC libopus: wideband SILK/hybrid loss and DTX not checked"
                  " (OPUS_PREFIX=<default build> ./scripts/interop.sh to include them)")
        for cfg, ch, opts in cases:
            pk, ref = self.p("t.pk"), self.p("t.ref")
            out = run(OPUSREF, "gen", str(cfg), str(ch), "40", pk, ref, *opts).stdout.decode().strip()
            p = run(CLI, "opusdec", pk, str(ch), self.p("got.f64"), check=False)
            if p.returncode != 0:
                self.record("opus", f"cfg {cfg} {ch}ch {' '.join(opts)}", False, "shravan error")
                continue
            r, g = f64(ref), f64(self.p("got.f64"))
            e = maxerr(r, g)
            # Concealed CELT frames extrapolate through a 24-tap LPC filter; float32 in
            # libopus against f64 here differs by a few LSB on loud frames.
            tol = 4.0 if any(o.startswith("lose") for o in opts) else 2.0
            self.record("opus", f"cfg {cfg} {ch}ch {' '.join(opts)}",
                        len(r) == len(g) and e <= tol, f"maxerr={e:.2f} ({out.split('|')[0].strip()})")
        # Ogg Opus end to end from ffmpeg, with its pre-skip and end trimming.
        for ch, br, app, fd in ((1, "64k", "audio", "20"), (2, "96k", "audio", "10"),
                                (2, "128k", "lowdelay", "2.5"), (1, "16k", "voip", "20"),
                                (2, "24k", "voip", "40"), (2, "32k", "voip", "60")):
            src = self.p("o.opus")
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, ch, 1.0),
                "-c:a", "libopus", "-b:a", br, "-application", app, "-frame_duration", fd, src)
            ref = ff_decode(src, self.p("ref.f64"), "-c:a", "libopus")
            self.decode_vs("ogg", f"opus {ch}ch {br} {app} {fd}ms", src, ref, False)

    # -- encode direction -------------------------------------------------------

    def flac_encoder(self):
        """shravan's FLAC levels against libFLAC: flac -t passes, ffmpeg decodes every
        sample back, and the audio frames are no larger than `flac -L`'s on the same
        input (1% allowed; metadata excluded, as flac pads and shravan writes a seek
        table). Then depths and channel counts the integer-grid sweep skips."""
        def frames(path):
            b = open(path, "rb").read()
            pos = 4
            while True:
                last = b[pos] & 0x80
                pos += 4 + int.from_bytes(b[pos + 1:pos + 4], "big")
                if last:
                    return len(b) - pos

        def judge(name, out, grid, bits):
            t = run("flac", "-t", "-s", out, check=False)
            back = ff_decode(out, self.p("back.f64"))
            e = maxerr(grid, back, float(1 << (bits - 1)))
            ok = t.returncode == 0 and len(back) == len(grid) and e == 0
            return ok, f"maxerr={e:.3f} LSB n={len(back)}/{len(grid)} flac -t {'ok' if t.returncode == 0 else 'FAILED'}"

        wav = self.p("lv.wav")
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(44100, 2, 3.0),
            "-c:a", "pcm_s16le", wav)
        grid = ff_decode(wav, self.p("lv.f64"))
        for level in range(9):
            out = self.p("lv.flac")
            p = run(CLI, "encode", self.p("lv.f64"), "flac", "44100", "2", "16", out, str(level),
                    check=False)
            name = f"flac level {level} vs flac -{level}"
            if p.returncode != 0:
                self.record("encode", name, False, "shravan error " + p.stdout.decode().strip())
                continue
            ok, detail = judge(name, out, grid, 16)
            ref = self.p("ref.flac")
            run("flac", "-s", "-f", f"-{level}", "--no-padding", "--no-seektable", wav, "-o", ref)
            ours, theirs = frames(out), frames(ref)
            ok = ok and ours <= theirs * 1.01
            self.record("encode", name, ok, f"{detail} frames {ours}/{theirs} ({100.0 * (ours - theirs) / theirs:+.2f}%)")
        for bits, ch, rate in ((12, 2, 48000), (20, 2, 96000), (32, 2, 48000), (16, 8, 48000),
                               (24, 3, 44100)):
            codec = {12: "pcm_s16le", 16: "pcm_s16le", 20: "pcm_s24le", 24: "pcm_s24le",
                     32: "pcm_s32le"}[bits]
            src = self.p("dp.wav")
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.5),
                "-c:a", codec, src)
            g = ff_decode(src, self.p("dp.f64"))
            # Onto a grid of `bits` bits, so the encoder has an exact target.
            step = float(1 << (bits - 1))
            g = array.array("d", (round(v * step) / step for v in g))
            with open(self.p("dp.f64"), "wb") as fh:
                g.tofile(fh)
            for level in (0, 5, 8):
                out = self.p("dp.flac")
                p = run(CLI, "encode", self.p("dp.f64"), "flac", str(rate), str(ch), str(bits),
                        out, str(level), check=False)
                name = f"flac {bits}b {ch}ch {rate} level {level}"
                if p.returncode != 0:
                    self.record("encode", name, False, "shravan error " + p.stdout.decode().strip())
                    continue
                ok, detail = judge(name, out, g, bits)
                self.record("encode", name, ok, detail)
        # A seek point every 10 seconds, read back by metaflac.
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(8000, 1, 25.0),
            "-c:a", "pcm_s16le", self.p("long.wav"))
        ff_decode(self.p("long.wav"), self.p("long.f64"))
        run(CLI, "encode", self.p("long.f64"), "flac", "8000", "1", "16", self.p("long.flac"), "5")
        listing = run("metaflac", "--list", "--block-type=SEEKTABLE", self.p("long.flac")).stdout.decode()
        want = ["sample_number=0,", "sample_number=77824,", "sample_number=159744,"]
        ok = all(w in listing for w in want) and "seek points: 3" in listing
        self.record("encode", "flac seek table (metaflac)", ok, " ".join(l.strip() for l in listing.splitlines() if "point" in l))

    def encoders(self):
        for rate in (8000, 44100, 96000):
            for ch in (1, 2, 6):
                for kind, bits, ext in (("wav", 8, "wav"), ("wav", 16, "wav"), ("wav", 24, "wav"),
                                        ("wav", 32, "wav"), ("aiff", 16, "aiff"), ("aiff", 24, "aiff"),
                                        ("flac", 16, "flac"), ("flac", 24, "flac"), ("flac", 8, "flac")):
                    # Integer-grid input: a lossless encoder must hand every value back.
                    wav = self.p("grid.wav")
                    codec = {8: "pcm_u8", 16: "pcm_s16le", 24: "pcm_s24le", 32: "pcm_s32le"}[bits]
                    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.1),
                        "-c:a", codec, wav)
                    grid = ff_decode(wav, self.p("grid.f64"))
                    out = self.p("enc." + ext)
                    p = run(CLI, "encode", self.p("grid.f64"), kind, str(rate), str(ch), str(bits), out,
                            check=False)
                    name = f"{kind} {bits}b {rate} {ch}ch"
                    if p.returncode != 0:
                        self.record("encode", name, False, "shravan error " + p.stdout.decode().strip())
                        continue
                    back = ff_decode(out, self.p("back.f64"))
                    e = maxerr(grid, back, float(1 << (bits - 1)))
                    ok = len(back) == len(grid) and e == 0
                    detail = f"maxerr={e:.3f} LSB n={len(back)}/{len(grid)}"
                    if kind == "flac":
                        t = run("flac", "-t", "-s", out, check=False)
                        ok = ok and t.returncode == 0
                        detail += " flac -t " + ("ok" if t.returncode == 0 else "FAILED")
                    self.record("encode", name, ok, detail)
        self.flac_encoder()
        for kind in ("wavf32", "wavf64"):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, 2, 0.1),
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("x.f64"))
            x = f64(self.p("x.f64"))
            run(CLI, "encode", self.p("x.f64"), kind, "48000", "2", "0", self.p("enc.wav"))
            back = ff_decode(self.p("enc.wav"), self.p("back.f64"))
            want = x
            if kind == "wavf32":
                # The f32 samples must be ffmpeg's own rounding of the f64 input
                # (nearest, ties to even); before 2.9.3 shravan truncated.
                run("ffmpeg", "-v", "error", "-y", "-f", "f64le", "-ar", "48000", "-ac", "2",
                    "-i", self.p("x.f64"), "-c:a", "pcm_f32le", self.p("ref32.wav"))
                want = ff_decode(self.p("ref32.wav"), self.p("ref32.f64"))
            e = maxerr(want, back, 1.0)
            self.record("encode", kind, len(back) == len(want) and e == 0, f"maxerr={e:.3g}")
        # AAC: what ffmpeg and faad make of shravan's stream, at every band
        # layout and from low to high rates (the chirp, 300 + 2 sweep t Hz, held
        # under 6.3 kHz, inside both encoders' bandwidth at 32 kbit/s a channel, so
        # the comparison is of coding, not of where each draws the cutoff; at least
        # 12000 samples, past the correlation's warm-up). The stream must decode to the input at
        # exactly the encoder's 1024 samples of priming, within 0.02 of the
        # correlation ffmpeg's own encoder reaches on the same input at the same
        # rate; spend at most the rate asked (2% and 1 kbit/s for the reservoir on
        # a short clip); and keep every frame within AAC-LC's 6144 bits a channel.
        faad = shutil.which("faad")
        for rate in (8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 96000):
            for ch in (1, 2):
                dur = max(0.5, 12000 / rate)
                run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
                    signal(rate, ch, dur, 0.3, min(rate // 5, int(3000 / dur))),
                    "-f", "f64le", "-c:a", "pcm_f64le", self.p("x.f64"))
                x = f64(self.p("x.f64"))
                run("ffmpeg", "-v", "error", "-y", "-f", "f64le", "-ar", str(rate), "-ac", str(ch),
                    "-i", self.p("x.f64"), "-c:a", "pcm_s16le", self.p("x.wav"))
                for br_ch in (32000, 64000, 128000):
                    if br_ch > rate * 6:
                        continue
                    br = br_ch * ch
                    name = f"aac {rate} {ch}ch {br // 1000}k"
                    p = run(CLI, "encode", self.p("x.f64"), "aac", str(rate), str(ch), str(br),
                            self.p("enc.aac"), check=False)
                    if p.returncode != 0:
                        self.record("encode", name, False, "shravan error")
                        continue
                    data = open(self.p("enc.aac"), "rb").read()
                    sizes, pos = [], 0
                    while pos + 7 <= len(data) and data[pos] == 0xFF and data[pos + 1] & 0xF0 == 0xF0:
                        sizes.append(((data[pos + 3] & 3) << 11) | (data[pos + 4] << 3) | (data[pos + 5] >> 5))
                        pos += sizes[-1]
                    # over the coded frames (priming and flush), less the one
                    # reservoir a constant-rate stream may start with
                    coded = len(sizes) * 1024 / rate
                    kbps = (len(data) * 8 - 6144 * ch) / coded / 1000
                    frames_ok = pos == len(data) and max(sizes) - 7 <= 768 * ch
                    back = ff_decode(self.p("enc.aac"), self.p("back.f64"))
                    c, lag = best_lag_corr(x, back, ch, 1034, 2048, 1014)
                    fe = run("ffmpeg", "-v", "error", "-y", "-i", self.p("x.wav"), "-c:a", "aac",
                             "-b:a", str(br), "-f", "adts", self.p("ff.aac"), check=False)
                    cf, ffk = 0.0, 0.0
                    if fe.returncode == 0:
                        cf, _ = best_lag_corr(x, ff_decode(self.p("ff.aac"), self.p("ffb.f64")), ch, 1034, 2048, 1014)
                        ffk = (os.path.getsize(self.p("ff.aac")) * 8 - 6144 * ch) / coded / 1000
                    # ffmpeg's bar holds where it spends no more than 10% over
                    # shravan (it overshoots most at the lowest rates per sample).
                    bar = c >= cf - 0.02 or ffk > 1.1 * kbps
                    ok = lag == 1024 and c > 0.9 and bar and frames_ok and kbps <= br / 1000
                    detail = (f"corr={c:.4f} (ffmpeg's encoder {cf:.4f} at {ffk:.1f}) lag={lag} {kbps:.1f} kbit/s "
                              f"largest frame {(max(sizes) - 7) * 8} bits")
                    if faad:
                        fa = run(faad, "-q", "-o", self.p("faad.wav"), self.p("enc.aac"), check=False)
                        fok = fa.returncode == 0
                        if fok:
                            with wave.open(self.p("faad.wav")) as w:
                                frate, fn = w.getframerate(), w.getnframes()
                            if frate == rate:
                                cfa, _ = best_lag_corr(x, ff_decode(self.p("faad.wav"), self.p("faad.f64")), ch, 1034, 2048, 1014)
                                fok = cfa > c - 0.002
                                detail += f" faad corr={cfa:.4f}"
                            else:
                                # Below 32 kHz faad assumes implicit SBR and plays
                                # the stream at twice the rate: every frame, doubled.
                                fok = frate == 2 * rate and fn == 2048 * len(sizes)
                                detail += f" faad at {frate} Hz, {fn} frames"
                        else:
                            detail += " faad FAILED"
                        ok = ok and fok
                    self.record("encode", name, ok, detail)
        # CELT: what libopus makes of shravan's frames.
        for ch in (1, 2):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, ch, 0.5, 0.3),
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("x.f64"))
            x = f64(self.p("x.f64"))
            run(CLI, "celtenc", self.p("x.f64"), str(ch), str(160 * ch), self.p("c.pk"))
            run(OPUSREF, "dec", self.p("c.pk"), str(ch), self.p("back.f64"))
            back = f64(self.p("back.f64"))
            c, lag = best_lag_corr(x, back, ch, 200)
            self.record("encode", f"celt {ch}ch -> libopus", c > 0.9 and lag == 0, f"corr={c:.4f} lag={lag}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--only", help="comma-separated: wav,aiff,flac,alac,mp3,aac,gapless,opus,encoders")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()
    groups = ["wav", "aiff", "flac", "alac", "mp3", "aac", "gapless", "opus", "encoders"]
    if args.only:
        groups = [g for g in groups if g in args.only.split(",")]
    for tool in (CLI, OPUSREF):
        if not os.path.exists(tool):
            sys.exit(f"missing {tool}: run scripts/interop.sh")
    with tempfile.TemporaryDirectory() as tmp:
        sw = Sweep(tmp, args.verbose)
        for g in groups:
            print(f"== {g}")
            getattr(sw, g)()
        bad = [r for r in sw.results if not r[2]]
        print(f"\ninterop: {len(sw.results) - len(bad)} of {len(sw.results)} passed")
        sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
