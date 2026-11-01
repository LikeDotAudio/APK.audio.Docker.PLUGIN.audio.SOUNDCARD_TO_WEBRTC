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
//! "running and can see nothing"; a process that dies says it too, through
//! apk-plugin-runner's last will on the same leaf, which this session does NOT
//! duplicate), `incoming/detail` says why and which devices WOULD
//! open, and the plugin retries every RETRY so a card plugged in later, or an
//! APK_SOUNDCARD_DEVICE set and the container restarted, is picked up.
//!
//! Shared: PLUGIN.SOUNDCARD_OUT takes this module from this crate (see `signal`).
use anyhow::Result;
use apkaudio_contracts::agent_state::AgentState;
use apkaudio_contracts::baremetal_io;
use apk_plugin_runner::session::Session;
use rumqttc::{AsyncClient, QoS};
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
        // ITS OWN CLIENT ID, from the runner's `Session` (role `card`): MQTT
        // drops the older of two sessions with one id, so this and the
        // runner's session in the same process must never share one.
        let options = Session::new(token, "card").to(broker, port).options();
        // NO LAST WILL HERE. apk-plugin-runner's own session already wills
        // `unavailable` on this exact leaf (RP-01: the runner owns the will);
        // a second will on a second session was two owners of one fact.
        let (client, mut eventloop) = AsyncClient::new(options, 16);
        // The event loop must be polled for anything to leave; a broker that is
        // away is retried, never fatal — the card is this process's job, not the
        // bus. Paced by the contract's broker_backoff (signal::Pacing), not a
        // flat sleep, so a refusal backs off and says why.
        tokio::spawn(async move {
            let mut pacing = crate::signal::Pacing::new("card-state");
            loop {
                match eventloop.poll().await {
                    Ok(_) => pacing.on_success(),
                    Err(e) => pacing.on_failure(&e).await,
                }
            }
        });
        Ok(Self { client, state_topic, detail_topic })
    }

    /// Say `state` and why, retained (both leaves are conclusions — baremetal_io).
    pub async fn say(&self, state: AgentState, detail: &str) {
        // Counted (ROAD-3478): a refused publish is a publish_failure, not silence.
        apk_plugin_runner::loopback::published(
            self.client.publish(&self.detail_topic, QoS::AtLeastOnce, true, detail.to_owned()).await.is_ok(),
        );
        apk_plugin_runner::loopback::published(
            self.client.publish(&self.state_topic, QoS::AtLeastOnce, true, state.as_str()).await.is_ok(),
        );
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
