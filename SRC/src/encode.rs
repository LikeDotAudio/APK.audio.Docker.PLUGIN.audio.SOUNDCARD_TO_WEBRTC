// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! PCM frames in, Opus packets out.
//!
//! Runs on its own OS thread, between the audio callback and the async
//! runtime, and that placement is the point: encoding must not happen inside
//! cpal's callback (it allocates and takes low milliseconds) and must not
//! happen on a tokio worker (it is CPU-bound and would stall the reactor).
use crate::format::{FRAME_INTERLEAVED, SAMPLE_RATE};
use anyhow::{Context, Result};
use audiopus::coder::Encoder;
use audiopus::{Application, Channels, SampleRate};
use bytes::Bytes;
use std::sync::mpsc::Receiver;
use tokio::sync::mpsc::UnboundedSender;

/// Bounded by the largest packet Opus will emit at this frame size.
const MAX_PACKET: usize = 4000;

/// Frames between throughput lines -- 250 x 20 ms is five seconds.
const REPORT_EVERY: u64 = 250;

/// Drain `frames`, encode each, and forward the packets until either side hangs up.
pub fn run(frames: Receiver<Vec<i16>>, out: UnboundedSender<Bytes>) -> Result<()> {
    let encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio)
        .context("could not create the Opus encoder")?;
    let mut packet = vec![0u8; MAX_PACKET];
    let mut packets = 0u64;
    let mut bytes = 0u64;

    for frame in frames {
        if frame.len() != FRAME_INTERLEAVED {
            tracing::warn!(len = frame.len(), "short frame skipped");
            continue;
        }
        match encoder.encode(&frame, &mut packet) {
            Ok(n) => {
                if out.send(Bytes::copy_from_slice(&packet[..n])).is_err() {
                    return Ok(());
                }
                packets += 1;
                bytes += n as u64;
                // One line every five seconds of audio: enough to prove the
                // chain is moving, too little to fill a log with.
                if packets.is_multiple_of(REPORT_EVERY) {
                    tracing::info!(packets, bytes, "encoded");
                }
            }
            Err(e) => tracing::warn!(error = %e, "Opus encode failed, frame dropped"),
        }
    }
    Ok(())
}

/// What the SDP must say for the packets this module produces to be decodable.
pub const CLOCK_RATE: u32 = SAMPLE_RATE;
