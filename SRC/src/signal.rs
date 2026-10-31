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
//!
//! ONE COPY, BOTH HALVES (audit 2026-10-30). This module, `card_state` and
//! `settings` are THE WebRTC link: PLUGIN.SOUNDCARD_OUT takes them from this
//! crate (`apkaudio-soundcard-to-webrtc`, by git rev; the engine `[patch]`
//! points it back here in the monorepo) instead of keeping a copy "identical by
//! hand". A change to `Signal`, the roles or the hello rule is made once.
//!
//! THE WIRE FORM — the WebRTC signalling Published Language, version
//! [`SIGNAL_VERSION`]: one JSON object per message, unretained, on
//! `APK.audio/Audio/WebRTC/{room}/{offerer|answerer}`:
//!
//! ```text
//! {"v":1,"type":"hello"}                     answerer -> offerer: say your offer (again)
//! {"v":1,"type":"offer","sdp":"…"}           offerer  -> answerer
//! {"v":1,"type":"answer","sdp":"…"}          answerer -> offerer
//! {"v":1,"type":"ice","candidate":"…"}       either way, trickled
//! ```
//!
//! A message with no `v` is version 1 (what every sender said before the field
//! existed); a message with a GREATER `v` than this build speaks is ignored and
//! logged, never half-read — see [`decode`].
use anyhow::{Result, anyhow, bail};
use apk_plugin_runner::session::Session;
use apkaudio_contracts::{broker_backoff, topics};
use rumqttc::{AsyncClient, EventLoop, QoS};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// The signalling version this build writes and the highest it reads.
pub const SIGNAL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Signal {
    /// The answerer, freshly subscribed: "say your offer (again)".
    Hello,
    Offer { sdp: String },
    Answer { sdp: String },
    Ice { candidate: String },
}

impl Signal {
    /// The `type` word, for logs and transcripts.
    pub fn kind(&self) -> &'static str {
        match self {
            Signal::Hello => "hello",
            Signal::Offer { .. } => "offer",
            Signal::Answer { .. } => "answer",
            Signal::Ice { .. } => "ice",
        }
    }
}

/// The versioned envelope: `v` beside the `type`-tagged message.
#[derive(Serialize, Deserialize)]
struct Envelope {
    #[serde(default = "first_version")]
    v: u32,
    #[serde(flatten)]
    signal: Signal,
}

fn first_version() -> u32 {
    1
}

/// One message in its wire form (`{"v":1,"type":…}`).
pub fn encode(msg: &Signal) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&Envelope { v: SIGNAL_VERSION, signal: msg.clone() })?)
}

/// Read one wire message. No `v` is version 1; a newer version is refused,
/// because a field this build does not know may change what the rest means.
pub fn decode(payload: &[u8]) -> Result<Signal> {
    let envelope: Envelope = serde_json::from_slice(payload)?;
    if envelope.v > SIGNAL_VERSION {
        bail!("signalling version {} is newer than this build's {SIGNAL_VERSION}", envelope.v);
    }
    Ok(envelope.signal)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Offerer,
    Answerer,
}

impl Role {
    pub fn leaf(self) -> &'static str {
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
    outlet: Outlet,
}

#[derive(Clone)]
enum Outlet {
    /// The broker: `topic` is this agent's own role leaf in the room.
    Bus { client: AsyncClient, topic: String },
    /// In process, to the peer's inbox — [`loopback`].
    Peer { role: Role, peer: mpsc::UnboundedSender<Signal>, transcript: Transcript },
}

impl SignalSender {
    pub async fn send(&self, msg: &Signal) -> Result<()> {
        let payload = encode(msg)?;
        match &self.outlet {
            Outlet::Bus { client, topic } => {
                client.publish(topic, QoS::AtLeastOnce, false, payload).await?;
            }
            Outlet::Peer { role, peer, transcript } => {
                // Through the wire form, so the loopback proves what the bus carries.
                let back = decode(&payload)?;
                transcript.lock().expect("transcript lock").push((*role, back.kind()));
                peer.send(back).map_err(|_| anyhow!("the peer's inbox is gone"))?;
            }
        }
        Ok(())
    }
}

/// Who said what, in order, over a [`loopback`] pair.
pub type Transcript = Arc<Mutex<Vec<(Role, &'static str)>>>;

/// An offerer and an answerer joined in process, every message passed through
/// [`encode`]/[`decode`]: the contract test's channel (SOUNDCARD_OUT's
/// `tests/offer_answer.rs` runs this crate's offerer against its answerer).
pub fn loopback() -> (Signalling, Signalling, Transcript) {
    let transcript: Transcript = Arc::default();
    let (to_answerer, answerer_inbox) = mpsc::unbounded_channel();
    let (to_offerer, offerer_inbox) = mpsc::unbounded_channel();
    let side = |role, peer, inbox| Signalling {
        sender: SignalSender { outlet: Outlet::Peer { role, peer, transcript: transcript.clone() } },
        inbox,
    };
    let offerer = side(Role::Offerer, to_answerer, offerer_inbox);
    let answerer = side(Role::Answerer, to_offerer, answerer_inbox);
    (offerer, answerer, transcript)
}

/// Connect to `broker`, subscribe to the peer's half of `room`, and start pumping.
///
/// The session is the runner's [`Session`] (`token`'s client id, credentials,
/// keep-alive — one constructor for every agent), opened ASYNC here because the
/// call runs on tokio. It is a session of its own and not the runner's: the
/// runner's client is synchronous and owns the plugin's last will.
pub async fn connect(token: &str, broker: &str, port: u16, room: &str, role: Role) -> Result<Signalling> {
    let options = Session::new(token, &format!("signal-{}", role.leaf())).to(broker, port).options();
    let (client, eventloop) = AsyncClient::new(options, 32);
    let (listen_topic, send_topic) = topics_for(room, role)?;

    let (tx, inbox) = mpsc::unbounded_channel();
    // SUBSCRIBED ON EVERY CONNACK, in the pump: a broker that restarts forgets
    // the subscription, and a receiver that no longer hears offers is a call
    // that never connects again.
    tokio::spawn(pump(eventloop, client.clone(), listen_topic.clone(), tx));
    tracing::info!(%listen_topic, %send_topic, "signalling connected");
    Ok(Signalling { sender: SignalSender { outlet: Outlet::Bus { client, topic: send_topic } }, inbox })
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
async fn pump(mut eventloop: EventLoop, client: AsyncClient, listen: String, tx: mpsc::UnboundedSender<Signal>) {
    let mut pacing = Pacing::new("signalling");
    loop {
        match eventloop.poll().await {
            Ok(event) => {
                pacing.on_success();
                if let rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(_)) = &event {
                    if let Err(e) = client.try_subscribe(&listen, QoS::AtLeastOnce) {
                        tracing::warn!(error = %e, %listen, "could not subscribe");
                    }
                }
                if let rumqttc::Event::Incoming(rumqttc::Packet::Publish(p)) = event {
                    match decode(&p.payload) {
                        Ok(sig) => {
                            if tx.send(sig).is_err() {
                                return;
                            }
                        }
                        Err(e) => tracing::warn!(error = %e, "signalling payload ignored"),
                    }
                }
            }
            Err(e) => pacing.on_failure(&e).await,
        }
    }
}

/// The reconnect pacing for ONE of this process's extra MQTT sessions (the
/// card-state session and the signalling session; the runner's own session
/// paces itself).
///
/// It was a flat `sleep(2 s)` on every error, which treated a broker that
/// REFUSED these credentials the same as one that is merely down: a refusal is
/// a configuration fault that retrying cannot fix, so it now backs off to a
/// minute and says so on the contract's schedule, while a broker that is away
/// is retried on the short transient curve. The policy is
/// `apkaudio_contracts::broker_backoff`; this only classifies rumqttc's error
/// and hands it tokio's timer via `wait_out`, never `step`/`drain`, whose
/// `std::thread::sleep` would hold a tokio worker. One `Pacing` per session,
/// owned by the loop that polls it.
///
/// SOUNDCARD_OUT uses this one (no twin copy any more).
pub struct Pacing {
    session: &'static str,
    backoff: broker_backoff::Backoff,
    last_reason: Option<String>,
}

impl Pacing {
    pub fn new(session: &'static str) -> Self {
        Self { session, backoff: broker_backoff::Backoff::new(), last_reason: None }
    }

    /// A packet moved: every curve resets.
    pub fn on_success(&mut self) {
        self.backoff.on_success();
        self.last_reason = None;
    }

    /// The session failed: say so when it is news, then wait the paced time.
    pub async fn on_failure(&mut self, error: &rumqttc::ConnectionError) {
        let refused = matches!(error, rumqttc::ConnectionError::ConnectionRefused(_));
        let retry = self.backoff.on_failure(refused);
        let reason = error.to_string();
        if retry.is_news(&reason, &mut self.last_reason) {
            match &retry.message {
                Some(message) => tracing::warn!(session = self.session, "{message}"),
                None => tracing::warn!(
                    session = self.session,
                    "{reason} -- reconnecting in {:?}{}",
                    retry.wait,
                    retry.suppressed_note()
                ),
            }
        }
        broker_backoff::wait_out(&retry, tokio::time::sleep).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two lists of role names with nothing comparing them is how they drift.
    /// `webrtc_signal` refuses any role outside the contract's closed pair, so
    /// a rename there would strand this crate at startup; this fails first.
    /// A refused CONNACK is a configuration fault and backs off on the
    /// contract's refusal curve; a broker that is away is transient. Both go
    /// through `broker_backoff`, never a fixed sleep.
    #[tokio::test(start_paused = true)]
    async fn pacing_waits_the_contract_curve() {
        let mut pacing = Pacing::new("test");
        let refused = rumqttc::ConnectionError::ConnectionRefused(rumqttc::ConnectReturnCode::NotAuthorized);
        let started = tokio::time::Instant::now();
        pacing.on_failure(&refused).await;
        assert_eq!(started.elapsed(), broker_backoff::REFUSAL_FIRST);

        pacing.on_success();
        let away = rumqttc::ConnectionError::NetworkTimeout;
        let started = tokio::time::Instant::now();
        pacing.on_failure(&away).await;
        assert_eq!(started.elapsed(), broker_backoff::TRANSIENT_FIRST);
    }

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

    /// `hello` is the whole wire form of the answerer's request, versioned.
    #[test]
    fn hello_is_a_bare_type() {
        assert_eq!(String::from_utf8(encode(&Signal::Hello).unwrap()).unwrap(), r#"{"v":1,"type":"hello"}"#);
        assert_eq!(decode(br#"{"v":1,"type":"hello"}"#).unwrap(), Signal::Hello);
    }

    /// The fixed documents of version 1, both ways.
    #[test]
    fn the_wire_form_is_the_published_one() {
        let cases = [
            (Signal::Offer { sdp: "o".into() }, r#"{"v":1,"type":"offer","sdp":"o"}"#),
            (Signal::Answer { sdp: "a".into() }, r#"{"v":1,"type":"answer","sdp":"a"}"#),
            (Signal::Ice { candidate: "c".into() }, r#"{"v":1,"type":"ice","candidate":"c"}"#),
        ];
        for (msg, wire) in cases {
            assert_eq!(String::from_utf8(encode(&msg).unwrap()).unwrap(), wire);
            assert_eq!(decode(wire.as_bytes()).unwrap(), msg);
        }
    }

    /// A sender from before `v` is version 1; a newer one is refused, not half-read.
    #[test]
    fn versions_are_read_honestly() {
        assert_eq!(decode(br#"{"type":"hello"}"#).unwrap(), Signal::Hello);
        assert!(decode(br#"{"v":2,"type":"hello"}"#).is_err());
        assert!(decode(br#"{"v":1,"type":"bye"}"#).is_err());
    }

    /// The loopback carries the wire form and keeps the order.
    #[tokio::test]
    async fn the_loopback_pair_faces_itself() {
        let (offerer, mut answerer, transcript) = loopback();
        offerer.send(&Signal::Offer { sdp: "x".into() }).await.unwrap();
        assert_eq!(answerer.inbox.recv().await.unwrap(), Signal::Offer { sdp: "x".into() });
        answerer.send(&Signal::Hello).await.unwrap();
        assert_eq!(*transcript.lock().unwrap(), vec![(Role::Offerer, "offer"), (Role::Answerer, "hello")]);
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
