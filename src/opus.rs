//! Opus header parsing and CELT-mode encoding via Ogg container.
//!
//! Parses `OpusHead` and `OpusTags` packets from an Ogg bitstream.
//! Provides a CELT-mode encoder for mono/stereo audio at 48 kHz.

use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

/// Parsed Opus identification header (`OpusHead`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpusHead {
    /// Version number (must be 1 for the current spec).
    pub version: u8,
    /// Number of output channels.
    pub channel_count: u8,
    /// Number of samples to discard from the beginning of the decoded stream.
    pub pre_skip: u16,
    /// Original input sample rate (informational, Opus always decodes at 48 kHz).
    pub input_sample_rate: u32,
    /// Output gain in Q7.8 dB.
    pub output_gain: i16,
    /// Channel mapping family (0 = mono/stereo, 1 = Vorbis order, 255 = unspec).
    pub channel_mapping_family: u8,
}

/// Parse an `OpusHead` identification packet.
///
/// The packet must be at least 19 bytes and begin with the `OpusHead` magic.
///
/// # Errors
///
/// Returns [`ShravanError::InvalidHeader`] for wrong magic or truncated data.
#[must_use = "parsed Opus header is returned and should not be discarded"]
pub fn parse_opus_head(packet: &[u8]) -> Result<OpusHead> {
    if packet.len() < 19 {
        return Err(ShravanError::InvalidHeader(
            "OpusHead packet too short (need >= 19 bytes)".into(),
        ));
    }
    if &packet[0..8] != b"OpusHead" {
        return Err(ShravanError::InvalidHeader("missing OpusHead magic".into()));
    }

    let version = packet[8];
    let channel_count = packet[9];
    let pre_skip = u16::from_le_bytes([packet[10], packet[11]]);
    let input_sample_rate = u32::from_le_bytes([packet[12], packet[13], packet[14], packet[15]]);
    let output_gain = i16::from_le_bytes([packet[16], packet[17]]);
    let channel_mapping_family = packet[18];

    if channel_count == 0 {
        return Err(ShravanError::InvalidChannels(0));
    }

    Ok(OpusHead {
        version,
        channel_count,
        pre_skip,
        input_sample_rate,
        output_gain,
        channel_mapping_family,
    })
}

/// Parse an `OpusTags` comment packet.
///
/// If the `tag` feature is enabled, delegates to the Vorbis Comment parser
/// in [`crate::tag`]. Otherwise returns default metadata.
///
/// # Errors
///
/// Returns [`ShravanError::InvalidHeader`] for wrong magic or truncated data.
#[must_use = "parsed Opus tags are returned and should not be discarded"]
#[cfg(feature = "tag")]
pub fn parse_opus_tags(packet: &[u8]) -> Result<crate::tag::AudioMetadata> {
    if packet.len() < 8 {
        return Err(ShravanError::InvalidHeader(
            "OpusTags packet too short".into(),
        ));
    }
    if &packet[0..8] != b"OpusTags" {
        return Err(ShravanError::InvalidHeader("missing OpusTags magic".into()));
    }

    crate::tag::read_vorbis_comment(&packet[8..])
}

/// Parse an `OpusTags` comment packet (tag feature disabled — returns default).
#[must_use = "parsed Opus tags result should not be discarded"]
#[cfg(not(feature = "tag"))]
pub fn parse_opus_tags(packet: &[u8]) -> Result<()> {
    if packet.len() < 8 {
        return Err(ShravanError::InvalidHeader(
            "OpusTags packet too short".into(),
        ));
    }
    if &packet[0..8] != b"OpusTags" {
        return Err(ShravanError::InvalidHeader("missing OpusTags magic".into()));
    }
    Ok(())
}

/// Scan backwards from the end of `data` for the last Ogg page and read
/// its granule position.
fn find_last_granule(data: &[u8]) -> Option<i64> {
    // Search backwards for OggS capture pattern
    if data.len() < 27 {
        return None;
    }
    let mut pos = data.len().saturating_sub(27);
    loop {
        if pos + 14 <= data.len() && &data[pos..pos + 4] == b"OggS" && data[pos + 4] == 0 {
            // Read granule position at offset 6
            if pos + 14 <= data.len() {
                let granule = i64::from_le_bytes([
                    data[pos + 6],
                    data[pos + 7],
                    data[pos + 8],
                    data[pos + 9],
                    data[pos + 10],
                    data[pos + 11],
                    data[pos + 12],
                    data[pos + 13],
                ]);
                return Some(granule);
            }
        }
        if pos == 0 {
            break;
        }
        pos -= 1;
    }
    None
}

/// Decode an Opus stream from pre-extracted Ogg packets.
///
/// Called by [`crate::ogg::decode`] when the first packet is identified as
/// `OpusHead`. Parses the identification and comment headers, estimates
/// duration from the last Ogg page granule position, and returns
/// [`FormatInfo`] with an empty samples vector (no audio decoding).
///
/// # Errors
///
/// Returns errors for invalid Opus headers or missing packets.
pub(crate) fn decode_from_packets(
    packets: &[Vec<u8>],
    raw_data: &[u8],
) -> Result<(FormatInfo, Vec<f32>)> {
    if packets.is_empty() {
        return Err(ShravanError::EndOfStream);
    }

    let head = parse_opus_head(&packets[0])?;

    // Try parsing tags from second packet (best-effort)
    if packets.len() >= 2 {
        let _ = parse_opus_tags(&packets[1]);
    }

    // Estimate duration from last granule position
    let duration_secs = if let Some(granule) = find_last_granule(raw_data) {
        let effective = granule.saturating_sub(i64::from(head.pre_skip));
        if effective > 0 {
            effective as f64 / 48000.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let total_samples = if duration_secs > 0.0 {
        (duration_secs * 48000.0) as u64
    } else {
        0
    };

    let info = FormatInfo {
        format: AudioFormat::Opus,
        sample_rate: 48000, // Opus always decodes at 48 kHz
        channels: u16::from(head.channel_count),
        bit_depth: 16, // Opus is typically decoded to 16-bit
        duration_secs,
        total_samples,
    };

    Ok((info, Vec::new()))
}

/// Decode an Opus stream from raw Ogg container data.
///
/// Delegates to [`crate::ogg::extract_packets`] for Ogg demuxing, then
/// parses the Opus headers. No audio decoding is performed.
///
/// # Errors
///
/// Returns errors for invalid Ogg/Opus structure.
#[must_use = "decoded audio data is returned and should not be discarded"]
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    let packets = crate::ogg::extract_packets(data)?;
    decode_from_packets(&packets, data)
}

// ---------------------------------------------------------------------------
// Opus CELT-mode encoder
// ---------------------------------------------------------------------------

/// Default pre-skip for Opus encoder (3.75 ms at 48 kHz, per RFC 7845).
const ENCODER_PRE_SKIP: u16 = 312;

/// CELT frame size: 20 ms at 48 kHz = 960 samples.
const FRAME_SIZE: usize = 960;

/// Serialize an `OpusHead` identification header packet (RFC 7845 Section 5.1).
fn serialize_opus_head(channels: u8, pre_skip: u16, input_sample_rate: u32) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(19);
    pkt.extend_from_slice(b"OpusHead");
    pkt.push(1); // version
    pkt.push(channels);
    pkt.extend_from_slice(&pre_skip.to_le_bytes());
    pkt.extend_from_slice(&input_sample_rate.to_le_bytes());
    pkt.extend_from_slice(&0i16.to_le_bytes()); // output gain
    pkt.push(0); // channel mapping family 0 (mono/stereo)
    pkt
}

/// Serialize a minimal `OpusTags` comment header packet (RFC 7845 Section 5.2).
fn serialize_opus_tags() -> Vec<u8> {
    let vendor = b"shravan";
    let mut pkt = Vec::with_capacity(8 + 4 + vendor.len() + 4);
    pkt.extend_from_slice(b"OpusTags");
    // Vendor string length + string
    pkt.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    pkt.extend_from_slice(vendor);
    // User comment list length = 0
    pkt.extend_from_slice(&0u32.to_le_bytes());
    pkt
}

// --- Range coder (RFC 6716 Section 4.1) ---

/// Range encoder state for Opus/CELT bitstream construction.
struct RangeEncoder {
    /// Output buffer.
    buf: Vec<u8>,
    /// Low end of the current range.
    low: u32,
    /// Range size.
    range: u32,
    /// Number of outstanding carry bytes.
    carry_count: u32,
    /// Cached byte waiting for carry resolution.
    cache: i32,
    /// Total bits used (for rate tracking).
    bits_used: u32,
}

impl RangeEncoder {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            low: 0,
            range: 0x8000_0000,
            carry_count: 0,
            cache: -1,
            bits_used: 0,
        }
    }

    /// Encode a symbol with probability ft/total in range [fl, fh) out of total.
    ///
    /// `total` must be > 0. `fl` must be < `fh` and `fh` must be <= `total`.
    fn encode(&mut self, fl: u32, fh: u32, total: u32) {
        debug_assert!(total > 0, "range encoder total must be > 0");
        debug_assert!(fl < fh, "range encoder fl must be < fh");
        debug_assert!(fh <= total, "range encoder fh must be <= total");
        if total == 0 {
            return;
        }
        let r = self.range / total;
        let new_low = self.low.wrapping_add(r.wrapping_mul(fl));
        if fh < total {
            self.range = r.wrapping_mul(fh - fl);
        } else {
            self.range = self.range.wrapping_sub(r.wrapping_mul(fl));
        }
        self.low = new_low;
        self.normalize();
    }

    /// Encode a single bit with equal probability.
    fn encode_bit(&mut self, val: bool) {
        self.encode(u32::from(val), u32::from(val) + 1, 2);
    }

    /// Encode a value uniformly in [0, total).
    fn encode_uint(&mut self, val: u32, total: u32) {
        if total <= 1 {
            return;
        }
        self.encode(val, val + 1, total);
    }

    fn normalize(&mut self) {
        while self.range <= 0x0080_0000 {
            self.carry_out();
            self.low <<= 8;
            self.range <<= 8;
            self.bits_used += 8;
        }
    }

    fn carry_out(&mut self) {
        let carry = (self.low >> 23) as i32;
        if carry != 0xFF {
            if self.cache >= 0 {
                self.buf
                    .push((self.cache as u32).wrapping_add((carry >> 8) as u32) as u8);
            }
            for _ in 0..self.carry_count {
                self.buf
                    .push(((carry >> 8) as u32).wrapping_add(0xFF) as u8);
            }
            self.carry_count = 0;
            self.cache = carry & 0xFF;
        } else {
            self.carry_count += 1;
        }
        self.low &= 0x007F_FFFF;
    }

    /// Finalize and return the encoded bytes.
    fn finish(mut self) -> Vec<u8> {
        // Flush remaining state
        if self.cache >= 0 {
            let carry = self.low >> 23;
            self.buf.push((self.cache as u32).wrapping_add(carry) as u8);
            for _ in 0..self.carry_count {
                self.buf
                    .push(carry.wrapping_add(0xFF).wrapping_sub(1) as u8);
            }
        }

        // Flush final bytes from low
        let nbits = if self.range > 0 {
            32u32.saturating_sub(self.range.leading_zeros()).max(1)
        } else {
            1
        };
        let nbytes = nbits.div_ceil(8);
        let shift = nbytes * 8 - 8;
        let mut val = self.low >> (23u32.saturating_sub(nbits));
        for _ in 0..nbytes {
            self.buf.push((val >> shift) as u8);
            val <<= 8;
        }

        self.buf
    }

    /// Get approximate bytes used so far.
    fn bytes_used(&self) -> usize {
        self.buf.len() + 1 + self.carry_count as usize
    }
}

// --- Complex arithmetic for FFT ---

#[derive(Clone, Copy)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const ZERO: Self = Self { re: 0.0, im: 0.0 };

    #[inline]
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    #[inline]
    fn mul(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    #[inline]
    #[allow(dead_code)]
    fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }

    /// exp(i * theta)
    #[inline]
    fn from_angle(theta: f64) -> Self {
        Self {
            re: libm::cos(theta),
            im: libm::sin(theta),
        }
    }
}

// --- Mixed-radix FFT (supports factors of 2, 3, 5) ---

/// In-place mixed-radix DIT FFT.
/// `buf` length must factor entirely into 2, 3, and 5.
fn fft(buf: &mut [Complex]) {
    let n = buf.len();
    if n <= 1 {
        return;
    }

    // Find smallest factor
    let radix = if n.is_multiple_of(2) {
        2
    } else if n.is_multiple_of(3) {
        3
    } else if n.is_multiple_of(5) {
        5
    } else {
        dft_naive(buf);
        return;
    };

    let m = n / radix;

    // Decimation-in-time: reorder into `radix` sub-sequences of length `m`
    let mut tmp = vec![Complex::ZERO; n];
    for r in 0..radix {
        for j in 0..m {
            tmp[r * m + j] = buf[j * radix + r];
        }
    }

    // Recurse on each sub-FFT of length m
    for r in 0..radix {
        fft(&mut tmp[r * m..(r + 1) * m]);
    }

    // Combine: generic radix-R butterfly.
    // Output[k] = Σ_{r=0}^{R-1} tmp[r*m + (k mod m)] * W_N^{r*k}
    // where W_N = exp(-j 2π/N)
    let angle_base = -2.0 * core::f64::consts::PI / n as f64;

    for (k, out) in buf.iter_mut().enumerate() {
        let km = k % m;
        let mut sum = Complex::ZERO;
        for r in 0..radix {
            let tw = Complex::from_angle(angle_base * (r * k) as f64);
            sum = sum.add(tmp[r * m + km].mul(tw));
        }
        *out = sum;
    }
}

/// Naive DFT for small prime sizes (fallback, not expected for 240).
fn dft_naive(buf: &mut [Complex]) {
    let n = buf.len();
    let tmp: Vec<Complex> = buf.to_vec();
    let angle = -2.0 * core::f64::consts::PI / (n as f64);
    for (k, out) in buf.iter_mut().enumerate() {
        let mut sum = Complex::ZERO;
        for (j, &inp) in tmp.iter().enumerate() {
            let w = Complex::from_angle(angle * (k * j) as f64);
            sum = sum.add(inp.mul(w));
        }
        *out = sum;
    }
}

// --- MDCT via Makhoul DCT-IV + FFT ---

/// Compute forward MDCT of `input` (length N) producing N/2 spectral coefficients.
///
/// MDCT: X[k] = Σ_{n=0}^{N-1} x[n] cos(π/N (n + 0.5 + N/4)(k + 0.5))
///
/// Uses N-point FFT with pre/post twiddle:
///   z[n] = x[n] * exp(-j π n / N), Z = FFT(z),
///   then X[k] = Re(Z[k] * exp(-j π n₀ (2k+1) / N)) where n₀ = 0.5 + N/4.
///
/// The FFT gives us Σ x[n] exp(-j π n (2k+1) / N), which combined with the
/// post-twiddle produces the exact MDCT.
///
/// Complexity: O(N log N). For N=960 = 2⁶ × 3 × 5, fully factorable.
#[inline]
fn mdct_forward(input: &[f32], output: &mut [f32]) {
    let n = input.len();
    let n2 = n / 2;
    let n4 = n / 4;
    let pi = core::f64::consts::PI;
    let n0 = 0.5 + n4 as f64; // MDCT phase offset

    // We need Σ_{n=0}^{N-1} x[n] exp(-j π n (2k+1) / (2N)) for k = 0..N/2-1.
    //
    // Pre-twiddle: z[n] = x[n] * exp(-j π n / (2N))
    // Then DFT_N{z}[k] = Σ z[n] exp(-j 2π nk / N)
    //                   = Σ x[n] exp(-j π n/(2N)) exp(-j 2π nk/N)
    //                   = Σ x[n] exp(-j π n (1 + 4k) / (2N))
    //
    // We want exponent π n (2k+1) / (2N). So 4k+1 ≠ 2k+1 in general.
    //
    // Fix: use 2N-point FFT. Zero-pad x to length 2N, pre-twiddle by exp(-jπn/(2N)):
    // z[n] = x[n] * exp(-j π n / (2N))  for n < N
    // z[n] = 0                           for n >= N
    //
    // DFT_{2N}{z}[k] = Σ_{n=0}^{N-1} x[n] exp(-jπn/(2N)) exp(-j 2π nk / (2N))
    //                = Σ x[n] exp(-j π n (2k+1) / (2N))
    //
    // This is exactly what the MDCT needs.
    let nn = 2 * n; // 2N
    let mut z = vec![Complex::ZERO; nn];
    for (i, &x) in input.iter().enumerate() {
        let tw = Complex::from_angle(-pi * i as f64 / nn as f64);
        z[i] = Complex::new(f64::from(x), 0.0).mul(tw);
    }
    // z[N..2N] = 0 (already initialized)

    // 2N-point FFT (2N = 2*960 = 1920 = 2^7 * 3 * 5, fully factorable)
    fft(&mut z);

    // Z[k] = Σ x[n] exp(-j π n (2k+1) / (2N))
    // MDCT: X[k] = Re(exp(-j π n₀ (2k+1) / (2N)) * Z[k])
    for (k, out) in output.iter_mut().enumerate().take(n2) {
        let angle = -pi * n0 * (2 * k + 1) as f64 / nn as f64;
        let tw = Complex::from_angle(angle);
        *out = z[k].mul(tw).re as f32;
    }
}

/// Apply a sine window to a frame.
#[inline]
fn sine_window(frame: &mut [f32]) {
    let n = frame.len();
    for (i, sample) in frame.iter_mut().enumerate() {
        let w = libm::sinf(core::f32::consts::PI / (n as f32) * (i as f32 + 0.5));
        *sample *= w;
    }
}

// --- CELT band structure (Bark-scale critical bands at 48 kHz, 20 ms) ---

/// CELT band boundaries for 960-sample frames (480 MDCT bins) at 48 kHz.
/// These are the Opus standard band edges from the reference encoder.
/// 21 bands, boundaries in MDCT bin indices.
const CELT_BAND_EDGES: [u16; 22] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 17, 21, 26, 32, 40, 50, 62, 78, 100, 480,
];

const NUM_CELT_BANDS: usize = CELT_BAND_EDGES.len() - 1;

/// Compute the log2 energy of each CELT band from MDCT coefficients.
fn compute_band_energies(mdct: &[f32]) -> [f32; NUM_CELT_BANDS] {
    let mut energies = [0.0f32; NUM_CELT_BANDS];

    for band in 0..NUM_CELT_BANDS {
        let start = CELT_BAND_EDGES[band] as usize;
        let end = CELT_BAND_EDGES[band + 1] as usize;
        let end = end.min(mdct.len());

        let mut sum = 0.0f32;
        for &c in &mdct[start..end] {
            sum += c * c;
        }

        // Log2 energy with floor to avoid log(0)
        let band_size = (end - start).max(1) as f32;
        energies[band] = libm::log2f((sum / band_size).max(1e-10));
    }

    energies
}

/// Quantize band energies to integer values for coding.
fn quantize_band_energies(energies: &[f32; NUM_CELT_BANDS]) -> [i16; NUM_CELT_BANDS] {
    let mut quant = [0i16; NUM_CELT_BANDS];
    for (i, &e) in energies.iter().enumerate() {
        // Quantize to 1/8th dB steps (6.02 dB per bit)
        quant[i] = libm::roundf(e * 2.0) as i16;
    }
    quant
}

/// Encode quantized band energies using Laplace-like coding.
fn encode_band_energies(rc: &mut RangeEncoder, quant: &[i16; NUM_CELT_BANDS]) {
    let mut prev = 0i32;
    for &q in quant.iter() {
        // Use i32 arithmetic to avoid i16 overflow on subtraction
        let diff = i32::from(q) - prev;
        let bounded = diff.clamp(-64, 63);
        let val = (bounded + 64) as u32;
        rc.encode_uint(val, 128);
        prev = i32::from(q);
    }
}

/// Normalize MDCT coefficients per band, producing unit-norm direction vectors.
fn normalize_bands(mdct: &[f32], norms: &mut [f32]) {
    for band in 0..NUM_CELT_BANDS {
        let start = CELT_BAND_EDGES[band] as usize;
        let end = (CELT_BAND_EDGES[band + 1] as usize).min(mdct.len());

        let mut sum_sq = 0.0f32;
        for &c in &mdct[start..end] {
            sum_sq += c * c;
        }
        let norm = libm::sqrtf(sum_sq).max(1e-10);

        for (i, &c) in mdct[start..end].iter().enumerate() {
            norms[start + i] = c / norm;
        }
    }
}

/// Encode the normalized spectral shape using simplified PVQ-like coding.
/// For each band, we encode the direction as quantized angles.
fn encode_spectral_shape(rc: &mut RangeEncoder, norms: &[f32], target_bytes: usize) {
    for band in 0..NUM_CELT_BANDS {
        let start = CELT_BAND_EDGES[band] as usize;
        let end = (CELT_BAND_EDGES[band + 1] as usize).min(norms.len());
        let band_size = end - start;

        if band_size == 0 || rc.bytes_used() >= target_bytes {
            break;
        }

        // Encode each coefficient's sign + approximate magnitude
        // This is a simplified version — full Opus uses PVQ with pulse allocation
        for &coeff in &norms[start..end] {
            if rc.bytes_used() >= target_bytes {
                break;
            }
            // Encode sign
            rc.encode_bit(coeff >= 0.0);
        }
    }
}

/// Encode a single CELT frame from interleaved f32 samples.
///
/// Returns the encoded Opus packet bytes for one 20ms frame.
fn encode_celt_frame(samples: &[f32], channels: u16, target_bytes: usize) -> Vec<u8> {
    let ch = channels as usize;
    let frame_samples = FRAME_SIZE;

    // Mix to mono for MDCT if stereo (encode channels independently for quality)
    let mut mono = vec![0.0f32; frame_samples];
    for (i, m) in mono.iter_mut().enumerate().take(frame_samples) {
        let mut sum = 0.0f32;
        for c in 0..ch {
            let idx = i * ch + c;
            if idx < samples.len() {
                sum += samples[idx];
            }
        }
        *m = sum / ch as f32;
    }

    // Apply window
    sine_window(&mut mono);

    // Forward MDCT
    let mdct_size = frame_samples / 2;
    let mut mdct = vec![0.0f32; mdct_size];
    mdct_forward(&mono, &mut mdct);

    // Compute and quantize band energies
    let energies = compute_band_energies(&mdct);
    let quant_energies = quantize_band_energies(&energies);

    // Normalize bands for shape coding
    let mut norms = vec![0.0f32; mdct_size];
    normalize_bands(&mdct, &mut norms);

    // Build Opus packet with range coder
    let mut rc = RangeEncoder::new();

    // TOC byte: CELT-only, 20ms frame
    // TOC format per RFC 6716 Section 3.1: config[7:3] | s[2] | c[1:0]
    // Config 30 = CELT-only FB 20ms (fullband, 48 kHz)
    // s = 0: we always encode a mono downmix in the CELT bitstream.
    //        OpusHead carries the original channel count for the decoder.
    // c = 0: 1 frame per packet
    let toc: u8 = 30 << 3; // config=30, s=0(mono coded), c=0(1 frame)

    // The TOC byte goes first, outside the range coder
    let mut packet = Vec::with_capacity(target_bytes);
    packet.push(toc);

    // Encode band energies
    encode_band_energies(&mut rc, &quant_energies);

    // Encode spectral shape with remaining bits
    encode_spectral_shape(&mut rc, &norms, target_bytes.saturating_sub(1));

    // Finalize range coder and append to packet
    let coded = rc.finish();
    packet.extend_from_slice(&coded);

    // Pad or truncate to target size
    if packet.len() < target_bytes {
        packet.resize(target_bytes, 0);
    } else if packet.len() > target_bytes {
        packet.truncate(target_bytes);
    }

    packet
}

/// Encode audio samples as an Opus bitstream in an Ogg container.
///
/// Produces a complete Ogg/Opus file. Input must be f32 samples in \[-1.0, 1.0\],
/// interleaved for stereo. Only 48 kHz input is supported (use the `resample`
/// feature to convert other rates).
///
/// # Arguments
///
/// * `samples` — interleaved f32 audio samples
/// * `sample_rate` — must be 48000 (Opus native rate)
/// * `channels` — 1 (mono) or 2 (stereo)
/// * `bitrate` — target bitrate in bits per second (32000..=256000)
///
/// # Errors
///
/// Returns errors for invalid parameters.
#[must_use = "encoded Opus/Ogg bytes are returned and should not be discarded"]
pub fn encode(samples: &[f32], sample_rate: u32, channels: u16, bitrate: u32) -> Result<Vec<u8>> {
    if sample_rate != 48000 {
        return Err(ShravanError::InvalidSampleRate(sample_rate));
    }
    if channels == 0 || channels > 2 {
        return Err(ShravanError::InvalidChannels(channels));
    }
    if !(32000..=256000).contains(&bitrate) {
        return Err(ShravanError::EncodeError(alloc::format!(
            "bitrate must be 32000..=256000, got {bitrate}"
        )));
    }

    let ch = channels as usize;
    let total_interleaved = samples.len();
    // Bytes per CELT frame based on target bitrate
    // 20ms frames → 50 frames/sec → bytes_per_frame = bitrate / 8 / 50
    let bytes_per_frame = (bitrate / 8 / 50).max(10) as usize;

    // Generate header packets
    let opus_head = serialize_opus_head(channels as u8, ENCODER_PRE_SKIP, sample_rate);
    let opus_tags = serialize_opus_tags();

    // Encode audio frames
    let mut audio_packets: Vec<Vec<u8>> = Vec::new();
    let mut granule_positions: Vec<i64> = Vec::new();
    let mut sample_pos: usize = 0;
    let frame_interleaved = FRAME_SIZE * ch;

    // Track granule: pre_skip + sample offset
    let mut granule: i64 = i64::from(ENCODER_PRE_SKIP);

    while sample_pos < total_interleaved {
        let end = (sample_pos + frame_interleaved).min(total_interleaved);
        let frame_slice = &samples[sample_pos..end];

        // Pad short final frame with silence
        let frame_data = if frame_slice.len() < frame_interleaved {
            let mut padded = vec![0.0f32; frame_interleaved];
            padded[..frame_slice.len()].copy_from_slice(frame_slice);
            encode_celt_frame(&padded, channels, bytes_per_frame)
        } else {
            encode_celt_frame(frame_slice, channels, bytes_per_frame)
        };

        audio_packets.push(frame_data);

        let actual_samples = (end - sample_pos) / ch;
        granule += actual_samples as i64;
        granule_positions.push(granule);

        sample_pos = end;
    }

    // Handle empty input
    if audio_packets.is_empty() {
        // Encode one frame of silence
        let silence = vec![0.0f32; frame_interleaved];
        let frame_data = encode_celt_frame(&silence, channels, bytes_per_frame);
        audio_packets.push(frame_data);
        granule_positions.push(granule);
    }

    // Assemble Ogg bitstream
    // Use a simple serial number
    let serial: u32 = 0x5368_7261; // "Shra" in ASCII

    // Build pages manually for proper header/data separation (RFC 7845)
    let mut ogg_data = Vec::new();

    // Page 0: BOS + OpusHead
    let page0 = crate::ogg::build_page(
        crate::ogg::HEADER_FLAG_BOS,
        0, // granule=0 for header pages
        serial,
        0,
        &opus_head,
    );
    ogg_data.extend_from_slice(&page0);

    // Page 1: OpusTags (not BOS, not EOS)
    let page1 = crate::ogg::build_page(
        0, // no flags
        0, serial, 1, &opus_tags,
    );
    ogg_data.extend_from_slice(&page1);

    // Audio pages (2+)
    let num_audio = audio_packets.len();
    for (i, (packet, &granule_pos)) in audio_packets
        .iter()
        .zip(granule_positions.iter())
        .enumerate()
    {
        let mut flags = 0u8;
        if i == num_audio - 1 {
            flags |= crate::ogg::HEADER_FLAG_EOS;
        }

        let page = crate::ogg::build_page(flags, granule_pos, serial, (i + 2) as u32, packet);
        ogg_data.extend_from_slice(&page);
    }

    Ok(ogg_data)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Build a minimal valid OpusHead packet.
    fn make_opus_head(channels: u8, pre_skip: u16, sample_rate: u32) -> Vec<u8> {
        let mut pkt = Vec::new();
        pkt.extend_from_slice(b"OpusHead");
        pkt.push(1); // version
        pkt.push(channels);
        pkt.extend_from_slice(&pre_skip.to_le_bytes());
        pkt.extend_from_slice(&sample_rate.to_le_bytes());
        pkt.extend_from_slice(&0i16.to_le_bytes()); // output gain
        pkt.push(0); // channel mapping family
        pkt
    }

    #[test]
    fn parse_valid_opus_head() {
        let pkt = make_opus_head(2, 312, 48000);
        let head = parse_opus_head(&pkt).unwrap();

        assert_eq!(head.version, 1);
        assert_eq!(head.channel_count, 2);
        assert_eq!(head.pre_skip, 312);
        assert_eq!(head.input_sample_rate, 48000);
        assert_eq!(head.output_gain, 0);
        assert_eq!(head.channel_mapping_family, 0);
    }

    #[test]
    fn reject_wrong_magic() {
        let mut pkt = make_opus_head(2, 312, 48000);
        pkt[0..8].copy_from_slice(b"NotOpus!");
        assert!(parse_opus_head(&pkt).is_err());
    }

    #[test]
    fn reject_short_data() {
        let pkt = b"OpusHead1234567"; // only 15 bytes
        assert!(parse_opus_head(pkt).is_err());
    }

    #[test]
    fn reject_zero_channels() {
        let pkt = make_opus_head(0, 312, 48000);
        assert!(parse_opus_head(&pkt).is_err());
    }

    #[test]
    fn opus_head_serde_roundtrip() {
        let pkt = make_opus_head(2, 312, 44100);
        let head = parse_opus_head(&pkt).unwrap();
        let json = serde_json::to_string(&head).unwrap();
        let head2: OpusHead = serde_json::from_str(&json).unwrap();
        assert_eq!(head, head2);
    }

    #[test]
    fn find_last_granule_none_on_empty() {
        assert_eq!(find_last_granule(&[]), None);
    }

    #[test]
    fn find_last_granule_finds_page() {
        // Build a fake Ogg page header with known granule
        let mut data = Vec::new();
        data.extend_from_slice(b"OggS");
        data.push(0); // version
        data.push(0x04); // header type (EOS)
        let granule: i64 = 96000;
        data.extend_from_slice(&granule.to_le_bytes());
        // serial, page_seq, crc, segments... pad to make it valid-looking
        data.resize(40, 0);

        assert_eq!(find_last_granule(&data), Some(96000));
    }

    #[test]
    fn opus_tags_reject_short() {
        let pkt = b"Opus";
        assert!(parse_opus_tags(pkt).is_err());
    }

    #[test]
    fn opus_tags_reject_wrong_magic() {
        let pkt = b"NotOpusTags_data";
        assert!(parse_opus_tags(pkt).is_err());
    }

    #[cfg(not(feature = "tag"))]
    #[test]
    fn opus_tags_no_tag_feature() {
        let mut pkt = Vec::new();
        pkt.extend_from_slice(b"OpusTags");
        pkt.extend_from_slice(&[0; 20]);
        assert!(parse_opus_tags(&pkt).is_ok());
    }

    // --- Encoder tests ---

    #[test]
    fn serialize_opus_head_roundtrip() {
        let serialized = serialize_opus_head(2, 312, 48000);
        let parsed = parse_opus_head(&serialized).unwrap();

        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.channel_count, 2);
        assert_eq!(parsed.pre_skip, 312);
        assert_eq!(parsed.input_sample_rate, 48000);
        assert_eq!(parsed.output_gain, 0);
        assert_eq!(parsed.channel_mapping_family, 0);
    }

    #[test]
    fn serialize_opus_tags_valid() {
        let tags = serialize_opus_tags();
        assert!(tags.starts_with(b"OpusTags"));
        // Should be parseable by our existing parser
        assert!(parse_opus_tags(&tags).is_ok());
    }

    #[test]
    fn encode_rejects_wrong_sample_rate() {
        let samples = vec![0.0f32; 960];
        assert!(encode(&samples, 44100, 1, 64000).is_err());
    }

    #[test]
    fn encode_rejects_zero_channels() {
        let samples = vec![0.0f32; 960];
        assert!(encode(&samples, 48000, 0, 64000).is_err());
    }

    #[test]
    fn encode_rejects_too_many_channels() {
        let samples = vec![0.0f32; 960 * 3];
        assert!(encode(&samples, 48000, 3, 64000).is_err());
    }

    #[test]
    fn encode_rejects_invalid_bitrate() {
        let samples = vec![0.0f32; 960];
        assert!(encode(&samples, 48000, 1, 1000).is_err());
        assert!(encode(&samples, 48000, 1, 500000).is_err());
    }

    #[test]
    fn encode_silence_mono() {
        let samples = vec![0.0f32; 48000]; // 1 second mono
        let ogg_data = encode(&samples, 48000, 1, 64000).unwrap();

        // Should start with OggS
        assert!(ogg_data.starts_with(b"OggS"));

        // Should be parseable as Ogg
        let packets = crate::ogg::extract_packets(&ogg_data).unwrap();
        assert!(packets.len() >= 3); // OpusHead + OpusTags + audio

        // First packet should be OpusHead
        let head = parse_opus_head(&packets[0]).unwrap();
        assert_eq!(head.channel_count, 1);
        assert_eq!(head.pre_skip, ENCODER_PRE_SKIP);
        assert_eq!(head.input_sample_rate, 48000);

        // Second packet should be OpusTags
        assert!(packets[1].starts_with(b"OpusTags"));
    }

    #[test]
    fn encode_silence_stereo() {
        let samples = vec![0.0f32; 48000 * 2]; // 1 second stereo
        let ogg_data = encode(&samples, 48000, 2, 128000).unwrap();

        let packets = crate::ogg::extract_packets(&ogg_data).unwrap();
        let head = parse_opus_head(&packets[0]).unwrap();
        assert_eq!(head.channel_count, 2);
    }

    #[test]
    fn encode_sine_wave() {
        // Generate a 440 Hz sine wave, 1 second at 48 kHz mono
        let samples: Vec<f32> = (0..48000)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 48000.0))
            .collect();

        let ogg_data = encode(&samples, 48000, 1, 96000).unwrap();
        assert!(ogg_data.starts_with(b"OggS"));
        assert!(ogg_data.len() > 100); // Should have meaningful content
    }

    #[test]
    fn encode_empty_input() {
        let samples: Vec<f32> = Vec::new();
        let ogg_data = encode(&samples, 48000, 1, 64000).unwrap();

        // Should still produce valid Ogg with headers + 1 silence frame
        let packets = crate::ogg::extract_packets(&ogg_data).unwrap();
        assert!(packets.len() >= 3);
    }

    #[test]
    fn encode_short_input_padded() {
        // Less than one frame (960 samples)
        let samples = vec![0.5f32; 100];
        let ogg_data = encode(&samples, 48000, 1, 64000).unwrap();

        let packets = crate::ogg::extract_packets(&ogg_data).unwrap();
        assert!(packets.len() >= 3);
    }

    #[test]
    fn encode_decode_headers_match() {
        let samples = vec![0.0f32; 9600]; // 200ms
        let ogg_data = encode(&samples, 48000, 1, 64000).unwrap();

        // Our decode() should parse the headers correctly
        let (info, _) = decode(&ogg_data).unwrap();
        assert_eq!(info.format, AudioFormat::Opus);
        assert_eq!(info.sample_rate, 48000);
        assert_eq!(info.channels, 1);
    }

    #[test]
    fn encode_granule_positions_increase() {
        let samples = vec![0.0f32; 48000 * 2]; // 2 seconds
        let ogg_data = encode(&samples, 48000, 1, 64000).unwrap();

        // Verify granule from last page (via find_last_granule)
        let granule = find_last_granule(&ogg_data);
        assert!(granule.is_some());
        let g = granule.unwrap();
        // Should be pre_skip + ~96000 samples
        assert!(g > 48000, "granule should be > 48000, got {g}");
    }

    #[test]
    fn range_encoder_basic() {
        let mut rc = RangeEncoder::new();
        rc.encode_bit(true);
        rc.encode_bit(false);
        rc.encode_uint(3, 8);
        let bytes = rc.finish();
        assert!(!bytes.is_empty());
    }

    /// Naive O(N²) MDCT for correctness validation.
    fn mdct_naive(input: &[f32], output: &mut [f32]) {
        let n = input.len();
        let n2 = n / 2;
        for (k, out) in output.iter_mut().enumerate().take(n2) {
            let mut sum = 0.0f64;
            for (i, &inp) in input.iter().enumerate().take(n) {
                let phase = core::f64::consts::PI / (n as f64)
                    * (f64::from(i as u32) + 0.5 + (n as f64) / 4.0)
                    * (f64::from(k as u32) + 0.5);
                sum += f64::from(inp) * libm::cos(phase);
            }
            *out = sum as f32;
        }
    }

    #[test]
    fn mdct_forward_produces_output() {
        let input = vec![1.0f32; 960];
        let mut output = vec![0.0f32; 480];
        mdct_forward(&input, &mut output);
        // At least some coefficients should be non-zero for constant input
        let energy: f32 = output.iter().map(|x| x * x).sum();
        assert!(energy > 0.0, "MDCT of constant input produced all zeros");
    }

    #[test]
    fn mdct_fft_matches_naive() {
        // Generate a test signal: sine wave at 440 Hz sampled at 48 kHz
        let input: Vec<f32> = (0..960)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 48000.0))
            .collect();

        let mut fft_output = vec![0.0f32; 480];
        let mut naive_output = vec![0.0f32; 480];

        mdct_forward(&input, &mut fft_output);
        mdct_naive(&input, &mut naive_output);

        // Compare: allow small floating-point differences
        let mut max_diff = 0.0f32;
        for (f, n) in fft_output.iter().zip(naive_output.iter()) {
            let diff = (f - n).abs();
            if diff > max_diff {
                max_diff = diff;
            }
        }
        assert!(
            max_diff < 0.01,
            "FFT-based MDCT diverges from naive: max_diff={max_diff}"
        );
    }

    #[test]
    fn mdct_small_matches_naive() {
        // Small N=16 test to isolate FFT issues from large-N effects
        let input: Vec<f32> = (0..16).map(|i| (i as f32) / 16.0).collect();
        let n2 = 8;
        let mut fft_out = vec![0.0f32; n2];
        let mut naive_out = vec![0.0f32; n2];

        mdct_forward(&input, &mut fft_out);
        mdct_naive(&input, &mut naive_out);

        let mut max_diff = 0.0f32;
        for (f, n) in fft_out.iter().zip(naive_out.iter()) {
            let diff = (f - n).abs();
            if diff > max_diff {
                max_diff = diff;
            }
        }
        assert!(
            max_diff < 0.01,
            "Small MDCT: FFT vs naive max_diff={max_diff}\nFFT:   {fft_out:?}\nNaive: {naive_out:?}"
        );
    }

    #[test]
    fn fft_size_6() {
        // DFT of [1, 0, 0, 0, 0, 0] for size 6 (= 2*3) should give [1, 1, 1, 1, 1, 1]
        let mut buf = vec![Complex::ZERO; 6];
        buf[0] = Complex::new(1.0, 0.0);
        fft(&mut buf);
        for (i, z) in buf.iter().enumerate() {
            assert!(
                (z.re - 1.0).abs() < 1e-10,
                "FFT size 6 bin {i}: re={}",
                z.re
            );
            assert!(z.im.abs() < 1e-10, "FFT size 6 bin {i}: im={}", z.im);
        }
    }

    #[test]
    fn fft_basic_correctness() {
        // FFT of [1, 0, 0, 0] should give [1, 1, 1, 1]
        let mut buf = vec![
            Complex::new(1.0, 0.0),
            Complex::ZERO,
            Complex::ZERO,
            Complex::ZERO,
        ];
        fft(&mut buf);
        for z in &buf {
            assert!((z.re - 1.0).abs() < 1e-10);
            assert!(z.im.abs() < 1e-10);
        }
    }

    #[test]
    fn fft_vs_dft_size_30() {
        // Compare FFT against naive DFT for size 30 = 2 * 3 * 5
        let n = 30;
        let mut buf_fft = Vec::with_capacity(n);
        let mut buf_dft = Vec::with_capacity(n);
        for i in 0..n {
            let val = Complex::new(libm::sin(i as f64 * 0.7), libm::cos(i as f64 * 1.3));
            buf_fft.push(val);
            buf_dft.push(val);
        }

        fft(&mut buf_fft);
        dft_naive(&mut buf_dft);

        let mut max_diff = 0.0f64;
        for (a, b) in buf_fft.iter().zip(buf_dft.iter()) {
            let dr = (a.re - b.re).abs();
            let di = (a.im - b.im).abs();
            max_diff = max_diff.max(dr).max(di);
        }
        assert!(max_diff < 1e-6, "FFT vs DFT size 30 max_diff={max_diff}");
    }

    #[test]
    fn fft_size_240() {
        // Verify FFT works for N=240 (the size used by MDCT with N=960)
        let mut buf = vec![Complex::ZERO; 240];
        buf[0] = Complex::new(1.0, 0.0);
        fft(&mut buf);
        // DC bin should be 1.0
        assert!((buf[0].re - 1.0).abs() < 1e-10);
    }
}
