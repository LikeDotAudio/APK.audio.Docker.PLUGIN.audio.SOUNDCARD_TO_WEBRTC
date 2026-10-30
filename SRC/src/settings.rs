// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The plugin's settings: `config.ini` gives the defaults, argv and the
//! environment override them, and the config the runner publishes RETAINED is
//! the one this process actually runs with (LOOSE-soundcard-config-unread).
//!
//! Before this, the file only described the flags; the bus carried a config
//! that no code read and that could disagree with the running call.
use apk_plugin_runner::ini_to_json;
use serde_json::Value;

/// Used only when the config section does not name one.
pub const FALLBACK_ROOM: &str = "default";
pub const FALLBACK_STUN: &str = "stun:stun.l.google.com:19302";

/// What the call runs with, after every source has spoken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub room: String,
    pub stun: String,
    /// `None` is the host default device.
    pub device: Option<String>,
}

fn section_key(config: &Value, token: &str, key: &str) -> Option<String> {
    config
        .as_object()?
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(token))?
        .1
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn chosen(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned)
}

/// Resolve: argv, then (device only) `APK_SOUNDCARD_DEVICE`, then the
/// `[token]` section of `ini`, then the fallback. Empty means "not given".
pub fn resolve(
    ini: &str,
    token: &str,
    room: Option<&str>,
    stun: Option<&str>,
    device: Option<&str>,
    device_env: Option<&str>,
) -> Settings {
    let config = ini_to_json(ini);
    Settings {
        room: chosen(room)
            .or_else(|| section_key(&config, token, "room"))
            .unwrap_or_else(|| FALLBACK_ROOM.to_owned()),
        stun: chosen(stun)
            .or_else(|| section_key(&config, token, "stun"))
            .unwrap_or_else(|| FALLBACK_STUN.to_owned()),
        device: chosen(device).or_else(|| chosen(device_env)).or_else(|| section_key(&config, token, "device")),
    }
}

/// `ini` with the `[token]` section's `room`, `stun` and `device` set to the
/// running values (added at the section's end when absent), comments kept.
/// This text is what the runner publishes as `<topic>/config`.
pub fn render(ini: &str, token: &str, s: &Settings) -> String {
    let values = [("room", s.room.as_str()), ("stun", s.stun.as_str()), ("device", s.device.as_deref().unwrap_or(""))];
    let mut written = [false; 3];
    let mut out = Vec::new();
    let mut in_section = false;
    let close = |out: &mut Vec<String>, written: &mut [bool; 3]| {
        for (i, (k, v)) in values.iter().enumerate() {
            if !written[i] {
                out.push(format!("{k} = {v}"));
                written[i] = true;
            }
        }
    };
    for raw in ini.lines() {
        let line = raw.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if in_section {
                close(&mut out, &mut written);
            }
            in_section = name.trim().eq_ignore_ascii_case(token);
            out.push(raw.to_owned());
            continue;
        }
        if in_section && !line.starts_with('#') && !line.starts_with(';') {
            if let Some((key, _)) = line.split_once('=') {
                if let Some(i) = values.iter().position(|(k, _)| *k == key.trim()) {
                    out.push(format!("{} = {}", values[i].0, values[i].1));
                    written[i] = true;
                    continue;
                }
            }
        }
        out.push(raw.to_owned());
    }
    if in_section {
        close(&mut out, &mut written);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config.ini"));
    const TOKEN: &str = "soundcard_in";

    #[test]
    fn the_shipped_config_is_read() {
        let s = resolve(INI, TOKEN, None, None, None, None);
        assert_eq!(s.room, "default");
        assert_eq!(s.stun, FALLBACK_STUN);
        assert_eq!(s.device, None);
    }

    #[test]
    fn argv_then_env_then_file() {
        let ini = "[soundcard_in]\nroom = studio\ndevice = hw:1\n";
        let s = resolve(ini, TOKEN, None, None, None, Some(""));
        assert_eq!((s.room.as_str(), s.device.as_deref()), ("studio", Some("hw:1")));
        let s = resolve(ini, TOKEN, Some("booth"), None, None, Some("hw:2"));
        assert_eq!((s.room.as_str(), s.device.as_deref()), ("booth", Some("hw:2")));
        let s = resolve(ini, TOKEN, None, None, Some("hw:3"), Some("hw:2"));
        assert_eq!(s.device.as_deref(), Some("hw:3"));
    }

    #[test]
    fn the_published_config_is_the_running_one() {
        let s = Settings { room: "booth".into(), stun: "stun:x:1".into(), device: Some("hw:2".into()) };
        let text = render(INI, TOKEN, &s);
        let back = resolve(&text, TOKEN, None, None, None, None);
        assert_eq!(back, s);
        let config = ini_to_json(&text);
        assert_eq!(apk_plugin_runner::module_topic(&config, TOKEN), "APK.audio/System/Protocols/soundcard_in");
    }

    #[test]
    fn missing_keys_are_added_to_the_section() {
        let s = Settings { room: "r".into(), stun: "s".into(), device: None };
        let text = render("[soundcard_in]\ntopic = t\n[other]\nroom = x\n", TOKEN, &s);
        let config = ini_to_json(&text);
        assert_eq!(config["soundcard_in"]["room"], "r");
        assert_eq!(config["soundcard_in"]["device"], "");
        assert_eq!(config["other"]["room"], "x");
    }
}
