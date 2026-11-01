// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The cpal input stream, reduced to a queue of 20 ms Opus-ready frames.
//!
//! THE RETURNED STREAM IS THE CAPTURE. cpal stops a stream when its handle
//! drops, and it drops silently -- no error, no end-of-stream, just a receiver
//! that never fills again. Hold the `Capture` for as long as audio is wanted.
//!
//! The device is asked for 48 kHz stereo and accepted at whatever it offers
//! instead; `format::conform` fixes the difference. The audio callback must not
//! allocate or block, so it only appends to a Vec it owns and hands whole
//! frames to a bounded channel, dropping a frame rather than waiting when the
//! consumer has stalled. A dropped frame is a click; a blocked callback is an
//! xrun that takes the whole card down.
use crate::format::{self, FRAME_INTERLEAVED};
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

pub struct Capture {
    pub frames: Receiver<Vec<i16>>,
    _stream: cpal::Stream,
    pub source_rate: u32,
    pub source_channels: u16,
}

/// Open `device` and start handing out interleaved 48 kHz stereo frames.
pub fn start(device: &cpal::Device) -> Result<Capture> {
    let default = device.default_input_config().context("device has no readable input config")?;
    let source_rate = default.sample_rate().0;
    let source_channels = default.channels();
    let config: cpal::StreamConfig = default.clone().into();

    // Bounded on purpose -- see the module note on dropping rather than blocking.
    let (tx, frames) = sync_channel::<Vec<i16>>(64);
    let sample_format = default.sample_format();
    let on_error = |e| tracing::error!(error = %e, "capture stream error");

    let mut pending: Vec<i16> = Vec::with_capacity(FRAME_INTERLEAVED * 4);
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let as_i16: Vec<i16> = data.iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect();
                drain(&as_i16, source_rate, source_channels as usize, &mut pending, &tx);
            },
            on_error,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                drain(data, source_rate, source_channels as usize, &mut pending, &tx);
            },
            on_error,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let as_i16: Vec<i16> = data.iter().map(|s| (*s as i32 - 32768) as i16).collect();
                drain(&as_i16, source_rate, source_channels as usize, &mut pending, &tx);
            },
            on_error,
            None,
        )?,
        other => anyhow::bail!("unsupported sample format {other:?}"),
    };

    stream.play().context("could not start the capture stream")?;
    Ok(Capture { frames, _stream: stream, source_rate, source_channels })
}

/// Conform one callback's worth of audio and emit every whole frame in it.
fn drain(data: &[i16], rate: u32, channels: usize, pending: &mut Vec<i16>, tx: &SyncSender<Vec<i16>>) {
    // TRAFFIC, counted for the loopback telemetry (ROAD-3478): one buffer the
    // card delivered. One relaxed atomic add — safe in the audio callback,
    // which must not block or allocate for it.
    apk_plugin_runner::loopback::packet_received();
    pending.extend_from_slice(&format::conform(data, rate, channels));
    while pending.len() >= FRAME_INTERLEAVED {
        let frame: Vec<i16> = pending.drain(..FRAME_INTERLEAVED).collect();
        if tx.try_send(frame).is_err() {
            // Consumer stalled. Losing this frame is the cheapest outcome.
            tracing::debug!("capture queue full, frame dropped");
        }
    }
}
