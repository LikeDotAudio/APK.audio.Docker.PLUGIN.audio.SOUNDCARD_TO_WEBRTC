// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The one frame shape both ends of this bridge agree on.
//!
//! 48 kHz, stereo, 20 ms. These are not tunable constants -- they are the
//! WebRTC Opus payload's own defaults, and the SDP this crate offers names
//! 48000/2. Changing one without changing the `sdp_fmtp_line` and the encoder
//! produces audio that negotiates cleanly and then plays at the wrong speed,
//! which is far harder to diagnose than a refused offer.
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
pub const FRAME_MS: usize = 20;

/// Samples per channel in one frame (960).
pub const FRAME_SAMPLES: usize = SAMPLE_RATE as usize / 1000 * FRAME_MS;

/// Interleaved i16 values in one frame (1920).
pub const FRAME_INTERLEAVED: usize = FRAME_SAMPLES * CHANNELS;

/// Resample and re-channel an interleaved slice onto [`SAMPLE_RATE`]/[`CHANNELS`].
///
/// Linear interpolation, deliberately: this bridge carries monitoring and
/// talkback, where a fraction of a dB of image error costs nothing and a
/// dependency on a windowed-sinc resampler costs a build. It is NOT good
/// enough to master through, and nothing here should pretend otherwise.
pub fn conform(input: &[i16], in_rate: u32, in_channels: usize) -> Vec<i16> {
    if in_channels == 0 || input.is_empty() {
        return Vec::new();
    }
    let in_frames = input.len() / in_channels;
    if in_frames == 0 {
        return Vec::new();
    }
    let out_frames = ((in_frames as u64 * SAMPLE_RATE as u64) / in_rate.max(1) as u64) as usize;
    let mut out = Vec::with_capacity(out_frames * CHANNELS);

    for i in 0..out_frames {
        // Position in the input, in fractional frames.
        let position = i as f64 * in_rate as f64 / SAMPLE_RATE as f64;
        let a = position.floor() as usize;
        let b = (a + 1).min(in_frames - 1);
        let t = position - a as f64;

        for ch in 0..CHANNELS {
            // Fan a mono source out to both legs; take the matching leg of a
            // multichannel one and ignore the rest.
            let source = if in_channels == 1 { 0 } else { ch.min(in_channels - 1) };
            let sa = input[a * in_channels + source] as f64;
            let sb = input[b * in_channels + source] as f64;
            out.push((sa + (sb - sa) * t).round() as i16);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A card already at 48 kHz stereo must not be resampled at all.
    #[test]
    fn identity_at_native_rate() {
        let input: Vec<i16> = (0..FRAME_INTERLEAVED as i16).collect();
        let out = conform(&input, SAMPLE_RATE, CHANNELS);
        assert_eq!(out, input);
    }

    /// 44.1 kHz in, 48 kHz out: the frame count scales, the channel count does not.
    #[test]
    fn upsamples_44100_to_48000() {
        let in_frames = 441;
        let input: Vec<i16> = vec![0; in_frames * 2];
        let out = conform(&input, 44_100, 2);
        assert_eq!(out.len() / CHANNELS, 480, "441 frames at 44.1k is 480 at 48k");
    }

    /// A mono card must reach both legs, not just the left.
    #[test]
    fn mono_fans_out_to_stereo() {
        let input: Vec<i16> = vec![1000; 480];
        let out = conform(&input, SAMPLE_RATE, 1);
        assert_eq!(out.len(), 960, "480 mono frames become 480 stereo pairs");
        assert!(out.chunks(2).all(|p| p[0] == p[1]), "both legs carry the source");
        assert!(out.iter().all(|&s| s == 1000));
    }

    /// An empty or zero-channel buffer is not a panic.
    #[test]
    fn degenerate_input_is_empty_output() {
        assert!(conform(&[], 48_000, 2).is_empty());
        assert!(conform(&[1, 2, 3], 48_000, 0).is_empty());
    }

    /// A real tone must survive conform -> Opus -> decode with its shape intact.
    ///
    /// Opus is lossy, so this asserts ENERGY rather than samples: silence
    /// arriving where a tone was sent is the failure this catches, and it is
    /// exactly what a wrong frame size or channel count produces.
    #[test]
    fn tone_survives_the_encoder() {
        use audiopus::coder::{Decoder, Encoder};
        use audiopus::{Application, Channels, SampleRate};

        let tone: Vec<i16> = (0..FRAME_INTERLEAVED)
            .map(|i| {
                let t = (i / CHANNELS) as f32 / SAMPLE_RATE as f32;
                ((t * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16
            })
            .collect();

        let encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio).unwrap();
        let mut packet = vec![0u8; 4000];
        let n = encoder.encode(&tone, &mut packet).unwrap();
        assert!(n > 10, "a 440 Hz tone must not encode to a silence frame ({n} bytes)");

        let mut decoder = Decoder::new(SampleRate::Hz48000, Channels::Stereo).unwrap();
        let mut out = vec![0i16; FRAME_INTERLEAVED];
        let frames = decoder.decode(Some(&packet[..n]), &mut out, false).unwrap();
        assert_eq!(frames, FRAME_SAMPLES);

        let energy: f64 = out.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / out.len() as f64;
        assert!(energy > 1_000_000.0, "decoded tone is too quiet: {energy}");
    }
}
