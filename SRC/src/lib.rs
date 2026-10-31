// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! System sound card in, Opus over WebRTC out.
//!
//! The chain is one direction and four hops: cpal capture -> conform to
//! 48 kHz stereo -> Opus -> a WebRTC audio track, with SDP and ICE carried
//! over MQTT because that is the bus this bench already runs.
//!
//! WHAT THIS CRATE DOES NOT DO: it does not mix, it does not resample well
//! enough to master through (see `format::conform`), and it does not own a
//! broker. One card to one peer; run one process per card.
pub mod card_state;
pub mod capture;
pub mod devices;
pub mod encode;
pub mod format;
pub mod session;
pub mod settings;
pub mod self_test;
pub mod signal;

/// This crate's `config.ini`, baked in at compile time: the settings' defaults,
/// the config the runner publishes (rendered with the running values) and what
/// the self test checks the section name against. One statement, read by
/// `main` and `self_test` alike.
pub const CONFIG_INI: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config.ini"));
