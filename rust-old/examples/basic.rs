//! Basic example: generate a sine wave, encode as WAV, decode back, verify.

fn main() {
    #[cfg(all(feature = "wav", feature = "pcm"))]
    {
        use shravan::pcm::PcmFormat;
        use shravan::wav;

        // Generate a 440 Hz sine wave, 1 second, mono, 44100 Hz
        let sample_rate = 44100u32;
        let duration_secs = 1.0f32;
        let frequency = 440.0f32;
        let num_samples = (sample_rate as f32 * duration_secs) as usize;

        let samples: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                libm::sinf(2.0 * core::f32::consts::PI * frequency * t)
            })
            .collect();

        println!("Generated {num_samples} samples of {frequency} Hz sine wave");
        println!("  Sample rate: {sample_rate} Hz, Duration: {duration_secs} s");

        // Encode as 16-bit WAV
        let wav_data =
            wav::encode(&samples, sample_rate, 1, PcmFormat::I16).expect("WAV encoding failed");
        println!("Encoded to WAV: {} bytes", wav_data.len());

        // Decode back
        let (info, decoded) = wav::decode(&wav_data).expect("WAV decoding failed");
        println!("Decoded WAV:");
        println!("  Format: {}", info.format);
        println!("  Sample rate: {} Hz", info.sample_rate);
        println!("  Channels: {}", info.channels);
        println!("  Bit depth: {}", info.bit_depth);
        println!("  Duration: {:.3} s", info.duration_secs);
        println!("  Total frames: {}", info.total_samples);
        println!("  Decoded samples: {}", decoded.len());

        // Verify roundtrip
        assert_eq!(decoded.len(), samples.len());
        let max_error: f32 = samples
            .iter()
            .zip(decoded.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);

        println!("  Max roundtrip error: {max_error:.6}");
        assert!(max_error < 0.001, "Roundtrip error too large: {max_error}");
        println!("Roundtrip verification passed.");
    }

    #[cfg(not(all(feature = "wav", feature = "pcm")))]
    {
        println!("This example requires the 'wav' and 'pcm' features.");
        println!("Run with: cargo run --example basic --features wav,pcm");
    }
}
