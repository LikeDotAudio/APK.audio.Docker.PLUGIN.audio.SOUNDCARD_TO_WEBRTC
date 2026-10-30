// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! SDP and ICE exchange over MQTT, because that is the bus this bench already has.
//!
//! WebRTC needs a signalling channel it does not itself provide, and adding an
//! HTTP server here would be a second control plane to secure and discover.
//! The two roles are fixed by direction: the sender OFFERS, the receiver
//! ANSWERS. Each publishes under its own role and subscribes to the other's,
//! so the same room name pairs exactly two agents and a stray third is inert.
//!
//! Nothing here is retained. A retained offer is a stale offer -- a peer that
//! reconnects would answer a session that ended, and the ICE candidates in it
//! point at ports nobody is listening on any more.
//!
//! SO THE ANSWERER ASKS. Unretained, an offer reaches only a receiver already
//! subscribed when it is said; one that starts later, or restarts after a call
//! ends, would never hear it. The answerer therefore says `hello` on its own
//! topic as soon as it is subscribed, and the offerer answers a `hello` by
//! saying its offer again -- or, if a call was already up, by ending the call
//! (the peer it had is gone), so the container restarts with a fresh offer.
use anyhow::{Result, anyhow};
use apkaudio_contracts::topics;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Signal {
    /// The answerer, freshly subscribed: "say your offer (again)".
    Hello,
    Offer { sdp: String },
    Answer { sdp: String },
    Ice { candidate: String },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Offerer,
    Answerer,
}

impl Role {
    fn leaf(self) -> &'static str {
        match self {
            Role::Offerer => "offerer",
            Role::Answerer => "answerer",
        }
    }
    fn peer(self) -> Role {
        match self {
            Role::Offerer => Role::Answerer,
            Role::Answerer => Role::Offerer,
        }
    }
}

/// The two topics one agent uses in `room`: the PEER's, which it subscribes
/// to, and its OWN, which it publishes on -- in that order.
///
/// Built through `apkaudio_contracts::topics` and never formatted here. This
/// crate used to carry its own copy of the root and `format!` the pair, which
/// made three statements of one topic -- the contract and two crates that did
/// not read it -- and let `--room` through with no validation at all:
/// `--room "a/b"` published two segments deep, the peer subscribing to
/// `{room}/{role}` never saw it, and the call did not connect with no error
/// anywhere.
pub fn topics_for(room: &str, role: Role) -> Result<(String, String)> {
    Ok((topic(room, role.peer().leaf())?, topic(room, role.leaf())?))
}

fn topic(room: &str, leaf: &str) -> Result<String> {
    topics::webrtc_signal(room, leaf).map_err(|e| {
        anyhow!(
            "{e} -- a room is ONE topic segment, [A-Za-z0-9_-]+: \
             no spaces, no slashes, no MQTT wildcards"
        )
    })
}

/// Refuse a room name before anything expensive is opened.
///
/// Called from `main` ahead of the sound card so a bad `--room` names the room
/// rather than failing four hops later as a call that silently never pairs.
pub fn check_room(room: &str, role: Role) -> Result<()> {
    topics_for(room, role).map(|_| ())
}

pub struct Signalling {
    pub sender: SignalSender,
    pub inbox: mpsc::UnboundedReceiver<Signal>,
}

/// A cloneable publish handle.
///
/// Split out from the receiving half because ICE candidates are produced in a
/// callback the peer connection owns and calls from its own task, so the thing
/// that publishes them cannot be borrowed from the struct driving the session.
#[derive(Clone)]
pub struct SignalSender {
    client: AsyncClient,
    topic: String,
}

impl SignalSender {
    pub async fn send(&self, msg: &Signal) -> Result<()> {
        let payload = serde_json::to_vec(msg)?;
        self.client.publish(&self.topic, QoS::AtLeastOnce, false, payload).await?;
        Ok(())
    }
}

/// Connect to `broker`, subscribe to the peer's half of `room`, and start pumping.
pub async fn connect(broker: &str, port: u16, room: &str, role: Role) -> Result<Signalling> {
    let client_id = format!("apk-webrtc-{}-{}", role.leaf(), std::process::id());
    let mut options = MqttOptions::new(client_id, broker, port);
    options.set_keep_alive(Duration::from_secs(20));

    let (client, eventloop) = AsyncClient::new(options, 32);
    let (listen_topic, send_topic) = topics_for(room, role)?;
    client.subscribe(&listen_topic, QoS::AtLeastOnce).await?;

    let (tx, inbox) = mpsc::unbounded_channel();
    tokio::spawn(pump(eventloop, tx));
    tracing::info!(%listen_topic, %send_topic, "signalling connected");
    Ok(Signalling { sender: SignalSender { client, topic: send_topic }, inbox })
}

impl Signalling {
    pub async fn send(&self, msg: &Signal) -> Result<()> {
        self.sender.send(msg).await
    }
}

/// Turn broker events into `Signal`s, dropping anything that is not one.
///
/// A malformed payload is logged and skipped rather than fatal: this topic is
/// public on the bench bus and one bad publish must not end a live call.
async fn pump(mut eventloop: EventLoop, tx: mpsc::UnboundedSender<Signal>) {
    loop {
        match eventloop.poll().await {
            Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(p))) => {
                match serde_json::from_slice::<Signal>(&p.payload) {
                    Ok(sig) => {
                        if tx.send(sig).is_err() {
                            return;
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "signalling payload ignored"),
                }
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, "signalling loop reconnecting");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two lists of role names with nothing comparing them is how they drift.
    /// `webrtc_signal` refuses any role outside the contract's closed pair, so
    /// a rename there would strand this crate at startup; this fails first.
    #[test]
    fn role_leaves_are_the_contract_pair() {
        assert_eq!([Role::Offerer.leaf(), Role::Answerer.leaf()], topics::WEBRTC_SIGNAL_ROLES);
    }

    /// The case the plan was carved for: a room that is not one segment is an
    /// error, not a topic nobody answers on.
    #[test]
    fn a_room_that_is_not_one_segment_is_refused() {
        assert!(check_room("a/b", Role::Offerer).is_err());
        assert!(check_room("has space", Role::Offerer).is_err());
        assert!(check_room("wild+card", Role::Answerer).is_err());
        assert!(check_room("", Role::Offerer).is_err());
        assert!(check_room("default", Role::Offerer).is_ok());
    }

    /// `hello` is the whole wire form of the answerer's request, and both
    /// crates must read it the same way (the two signal.rs are one protocol).
    #[test]
    fn hello_is_a_bare_type() {
        assert_eq!(serde_json::to_string(&Signal::Hello).unwrap(), r#"{"type":"hello"}"#);
        assert!(matches!(serde_json::from_str::<Signal>(r#"{"type":"hello"}"#).unwrap(), Signal::Hello));
    }

    /// The pairing itself: each half sends on its own role and listens on the
    /// other's, which is what makes a room hold exactly two agents.
    #[test]
    fn the_two_halves_face_each_other() {
        let (listen, send) = topics_for("studio-a", Role::Offerer).unwrap();
        assert_eq!(send, "APK.audio/Audio/WebRTC/studio-a/offerer");
        assert_eq!(listen, "APK.audio/Audio/WebRTC/studio-a/answerer");

        let (peer_listen, peer_send) = topics_for("studio-a", Role::Answerer).unwrap();
        assert_eq!(peer_send, listen);
        assert_eq!(peer_listen, send);
    }
}
