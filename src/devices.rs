// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! Which sound cards this host will let us record from.
//!
//! Enumeration is a READ of the live system and can fail per-device without
//! failing overall -- a card that is busy, unplugged mid-scan, or owned by
//! another process reports an error for itself alone. So a device whose config
//! cannot be read is skipped and named, never fatal: one unavailable card must
//! not hide the other five.
use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait};

pub struct InputDevice {
    pub name: String,
    pub default_rate: u32,
    pub default_channels: u16,
}

/// Every input device the default host exposes, with its default config.
pub fn list() -> Result<Vec<InputDevice>> {
    let host = cpal::default_host();
    let mut found = Vec::new();
    for device in host.input_devices().context("no input devices could be enumerated")? {
        let name = device.name().unwrap_or_else(|_| "<unnameable>".to_owned());
        match device.default_input_config() {
            Ok(cfg) => found.push(InputDevice {
                name,
                default_rate: cfg.sample_rate().0,
                default_channels: cfg.channels(),
            }),
            Err(e) => tracing::warn!(device = %name, error = %e, "skipped: no readable input config"),
        }
    }
    Ok(found)
}

/// Resolve `wanted` to a device: an exact name, else a substring, else the
/// host default when `wanted` is None.
///
/// Substring matching is a convenience for the command line, so it must not be
/// ambiguous: two matches is an error naming both, not a silent pick of the
/// first. An operator who typed half a name and got the wrong card would have
/// no way to tell from the audio.
pub fn resolve(wanted: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let Some(wanted) = wanted else {
        return host.default_input_device().ok_or_else(|| anyhow!("this host has no default input device"));
    };

    let mut exact = None;
    let mut partial = Vec::new();
    for device in host.input_devices()? {
        let Ok(name) = device.name() else { continue };
        if name == wanted {
            exact = Some(device);
            break;
        }
        if name.to_lowercase().contains(&wanted.to_lowercase()) {
            partial.push((name, device));
        }
    }
    if let Some(d) = exact {
        return Ok(d);
    }
    match partial.len() {
        1 => Ok(partial.pop().expect("length checked").1),
        0 => Err(anyhow!("no input device matches {wanted:?} -- run with --list-devices")),
        _ => {
            let names: Vec<_> = partial.iter().map(|(n, _)| n.as_str()).collect();
            Err(anyhow!("{wanted:?} matches {} devices: {}", names.len(), names.join(", ")))
        }
    }
}
