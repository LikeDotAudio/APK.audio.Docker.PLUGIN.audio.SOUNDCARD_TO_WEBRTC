// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The peer connection: one Opus track out, offer first, ICE trickled both ways.
//!
//! THIS SIDE ALWAYS OFFERS. It is the one with media to send, so it knows when
//! there is a session worth having; the receiver cannot offer without guessing.
//! It offers once at start and again whenever a receiver says `hello`.
//!
//! ICE is trickled rather than gathered in full before the offer goes out.
//! Waiting for gathering to complete adds the STUN timeout to every call setup
//! and is the usual reason a bench call takes thirty seconds to connect.
use crate::{encode, format};
use anyhow::{Context, Result};
use bytes::Bytes;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MIME_TYPE_OPUS, MediaEngine};
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::media::Sample;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

use crate::signal::{Role, Signal, SignalSender, Signalling};

/// Build a peer connection carrying one outbound Opus track.
pub async fn build(stun: &str) -> Result<(Arc<RTCPeerConnection>, Arc<TrackLocalStaticSample>)> {
    let mut media = MediaEngine::default();
    media.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media)?;
    let api = APIBuilder::new().with_media_engine(media).with_interceptor_registry(registry).build();

    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer { urls: vec![stun.to_owned()], ..Default::default() }],
        ..Default::default()
    };
    let pc = Arc::new(api.new_peer_connection(config).await?);

    // The SDP this advertises has to match what `encode` actually produces.
    let track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_OPUS.to_owned(),
            clock_rate: encode::CLOCK_RATE,
            channels: format::CHANNELS as u16,
            sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
            ..Default::default()
        },
        "audio".to_owned(),
        "apk-soundcard".to_owned(),
    ));
    pc.add_track(track.clone()).await?;
    Ok((pc, track))
}

/// Offer, then service the peer's answer and candidates until the call drops.
pub async fn run(
    pc: Arc<RTCPeerConnection>,
    track: Arc<TrackLocalStaticSample>,
    mut packets: UnboundedReceiver<Bytes>,
    mut sig: Signalling,
) -> Result<()> {
    publish_candidates(&pc, sig.sender.clone());

    let offer = pc.create_offer(None).await?;
    pc.set_local_description(offer.clone()).await?;
    sig.send(&Signal::Offer { sdp: offer.sdp.clone() }).await.context("could not publish the offer")?;
    tracing::info!("offer published, waiting for an answer");

    // Feeding the track is independent of signalling: a sample written before
    // the answer arrives is simply discarded by the track, which is correct.
    //
    // THE PUMP ENDING IS THE CALL ENDING. `packets` closes only when the
    // encoder thread has stopped (an encoder error, or capture gone), and from
    // then on there is no audio to send. Left running, the process would stay
    // up saying `listening` with silence behind it; instead `run` returns an
    // error, the process exits and the container restarts, card and all.
    let mut pump = tokio::spawn(async move {
        let duration = Duration::from_millis(format::FRAME_MS as u64);
        while let Some(packet) = packets.recv().await {
            let sample = Sample { data: packet, duration, ..Default::default() };
            if let Err(e) = track.write_sample(&sample).await {
                tracing::warn!(error = %e, "track write failed");
            }
        }
    });

    let mut answered = false;
    // A call that is up says nothing on signalling for as long as it lasts, so
    // the loop also turns on a timer: every turn ticks the runner's stall clock
    // (RP-09), and a turn that never comes — this task wedged on an await — is
    // what the runner's watchdog exits 75 for.
    let mut alive = tokio::time::interval(TICK_EVERY);
    loop {
        apk_plugin_runner::tick();
        let msg = tokio::select! {
            _ = alive.tick() => continue,
            msg = sig.inbox.recv() => match msg {
                Some(msg) => msg,
                None => break,
            },
            _ = &mut pump => {
                anyhow::bail!("the encoder stopped, so there is no audio to send -- ending the call");
            }
        };
        match msg {
            // A receiver that subscribed after the offer went out (the offer is
            // unretained -- see `signal`). Say the same offer again. If a call
            // is already up, the answerer that sent this is a NEW one: the peer
            // this connection was negotiated with has gone, and this connection
            // cannot take a second answer, so the call ends -- the process
            // exits and the container restarts with a fresh offer for it.
            Signal::Hello if !answered => {
                sig.send(&Signal::Offer { sdp: offer.sdp.clone() }).await.context("could not re-publish the offer")?;
                tracing::info!("a receiver asked; offer published again");
            }
            Signal::Hello => {
                tracing::info!("a new receiver asked while a call was up; ending this call so it can be offered afresh");
                break;
            }
            // A SECOND ANSWER TO ONE OFFER. The answerer's `hello` and this
            // side's first offer can cross, so the offer is said twice and a
            // well-behaved answerer may answer both; the connection is already
            // negotiated, and applying another answer in `stable` would fail
            // and end a good call (found by SOUNDCARD_OUT's tests/offer_answer.rs).
            Signal::Answer { .. } if answered => {
                tracing::debug!("a second answer to the same offer, ignored: the call is already negotiated");
            }
            Signal::Answer { sdp } => {
                let answer = RTCSessionDescription::answer(sdp)?;
                pc.set_remote_description(answer).await?;
                answered = true;
                tracing::info!("answer applied, call is up");
            }
            Signal::Ice { candidate } => {
                let init = RTCIceCandidateInit { candidate, ..Default::default() };
                if let Err(e) = pc.add_ice_candidate(init).await {
                    tracing::warn!(error = %e, "peer candidate rejected");
                }
            }
            // We are the offerer; another offer on this room is a second
            // sender that has not been told the room is taken.
            Signal::Offer { .. } => tracing::warn!("ignoring an offer: this agent is the offerer"),
        }
    }
    pump.abort();
    Ok(())
}

/// Publish our own candidates as they are discovered.
///
/// A candidate that cannot be serialised is dropped rather than fatal: ICE
/// succeeds on any one working pair, so losing a single candidate costs a
/// route, not the call.
fn publish_candidates(pc: &Arc<RTCPeerConnection>, sender: SignalSender) {
    pc.on_ice_candidate(Box::new(move |candidate| {
        let sender = sender.clone();
        Box::pin(async move {
            let Some(candidate) = candidate else { return };
            let init = match candidate.to_json() {
                Ok(init) => init,
                Err(e) => {
                    tracing::warn!(error = %e, "local candidate could not be serialised");
                    return;
                }
            };
            if let Err(e) = sender.send(&Signal::Ice { candidate: init.candidate }).await {
                tracing::warn!(error = %e, "local candidate could not be published");
            }
        })
    }));
}

pub const ROLE: Role = Role::Offerer;

/// How often the session loop turns with nothing to read, to tick the runner's
/// stall clock. A fraction of the runner's default 120 s stall window.
pub const TICK_EVERY: Duration = Duration::from_secs(10);
