// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! Whether this plugin has its sound card, said on the bus — not by crashing.
//!
//! Measured 2026-10-25: Plugin-SOUNDCARD_IN and _OUT restarted every ~60 s
//! with exit 1 ("device has no readable input config … unable to open slave").
//! The host has five cards; the ALSA *default* in the container is card 0, the
//! NVidia HDMI codec, which has no capture at all and whose playback opens
//! nothing without a monitor. A crash loop said none of that — the container
//! read "Restarting (1)" and the bus said nothing. Now a card that will not
//! open is a STATE: `incoming/state` = `unavailable` (the contract's word for
//! "running and can see nothing", and this client's last will, so a process
//! that dies says it too), `incoming/detail` says why and which devices WOULD
//! open, and the plugin retries every RETRY so a card plugged in later, or an
//! APK_SOUNDCARD_DEVICE set and the container restarted, is picked up.
//!
//! Twin: PLUGIN.SOUNDCARD_OUT/SRC/src/card_state.rs — keep the two identical.
use anyhow::Result;
use apkaudio_contracts::agent_state::AgentState;
use apkaudio_contracts::baremetal_io;
use rumqttc::{AsyncClient, LastWill, MqttOptions, QoS};
use std::time::Duration;

/// How long an idle plugin waits before trying the card again.
pub const RETRY: Duration = Duration::from_secs(30);

pub struct CardState {
    client: AsyncClient,
    state_topic: String,
    detail_topic: String,
}

impl CardState {
    /// Open the status session for `token` (`soundcard_in` / `soundcard_out`).
    pub async fn connect(token: &str, broker: &str, port: u16) -> Result<Self> {
        let (state_topic, _) = baremetal_io::module_incoming(token, "state")?;
        let (detail_topic, _) = baremetal_io::module_incoming(token, "detail")?;
        let mut options = MqttOptions::new(format!("apk-plugin-{token}-{}", std::process::id()), broker, port);
        options.set_keep_alive(Duration::from_secs(20));
        if let Some((username, password)) = apkaudio_contracts::broker_auth::credentials() {
            options.set_credentials(username, password);
        }
        options.set_last_will(LastWill::new(&state_topic, AgentState::Unavailable.as_str(), QoS::AtLeastOnce, true));
        let (client, mut eventloop) = AsyncClient::new(options, 16);
        // The event loop must be polled for anything to leave; a broker that is
        // away is retried, never fatal — the card is this process's job, not the bus.
        tokio::spawn(async move {
            loop {
                if eventloop.poll().await.is_err() {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        });
        Ok(Self { client, state_topic, detail_topic })
    }

    /// Say `state` and why, retained (both leaves are conclusions — baremetal_io).
    pub async fn say(&self, state: AgentState, detail: &str) {
        let _ = self.client.publish(&self.detail_topic, QoS::AtLeastOnce, true, detail.to_owned()).await;
        let _ = self.client.publish(&self.state_topic, QoS::AtLeastOnce, true, state.as_str()).await;
    }
}

/// The card-level device names worth suggesting (`default:CARD=…`), else all.
pub fn suggestions(names: &[String]) -> String {
    let cards: Vec<&str> = names.iter().map(String::as_str).filter(|n| n.starts_with("default:CARD=")).collect();
    let pick: Vec<&str> = if cards.is_empty() { names.iter().map(String::as_str).collect() } else { cards };
    if pick.is_empty() { "none — this host exposes no usable device".to_owned() } else { pick.join(", ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_level_names_are_suggested_first() {
        let names: Vec<String> = ["hw:CARD=Generic_1,DEV=0", "default:CARD=Generic_1", "default:CARD=AES67"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(suggestions(&names), "default:CARD=Generic_1, default:CARD=AES67");
    }

    #[test]
    fn no_device_says_so() {
        assert!(suggestions(&[]).starts_with("none"));
    }

    #[test]
    fn the_topics_are_the_contract_s() {
        let (state, retained) = baremetal_io::module_incoming("soundcard_in", "state").unwrap();
        assert_eq!(state, "APK.audio/System/Protocols/soundcard_in/incoming/state");
        assert!(retained);
        assert!(baremetal_io::module_incoming("soundcard_out", "detail").unwrap().1);
    }
}
