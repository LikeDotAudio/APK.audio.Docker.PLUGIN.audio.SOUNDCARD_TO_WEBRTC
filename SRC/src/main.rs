// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! `soundcard-to-webrtc` -- capture one card and offer it to a peer.
use anyhow::{Context, Result};
use apkaudio_soundcard_to_webrtc::{capture, devices, encode, session, signal};
use clap::Parser;

#[derive(Parser)]
#[command(about = "Capture a system sound card and send it as Opus over WebRTC")]
struct Args {
    /// Print every input device this host exposes, then exit.
    #[arg(long)]
    list_devices: bool,

    /// Input device, by exact name or unambiguous substring. Default: the host default.
    #[arg(long)]
    device: Option<String>,

    /// Signalling broker.
    #[arg(long, default_value = "127.0.0.1")]
    broker: String,

    #[arg(long, default_value_t = 1883)]
    broker_port: u16,

    /// Room name. The receiver must be given the same one.
    #[arg(long, default_value = "default")]
    room: String,

    #[arg(long, default_value = "stun:stun.l.google.com:19302")]
    stun: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
    let args = Args::parse();

    if args.list_devices {
        for d in devices::list()? {
            println!("{}  ({} Hz, {} ch)", d.name, d.default_rate, d.default_channels);
        }
        return Ok(());
    }

    // A room name is ONE topic segment. Refuse a bad one HERE, before a card is
    // opened: `--room "a/b"` builds a topic two segments deep, the peer
    // subscribing to `{room}/{role}` never sees it, and the call would simply
    // not connect with nothing logged anywhere.
    signal::check_room(&args.room, session::ROLE)?;

    let device = devices::resolve(args.device.as_deref())?;
    let capture = capture::start(&device).context("could not start capture")?;
    tracing::info!(rate = capture.source_rate, channels = capture.source_channels, "capturing");

    // Opus runs on its own thread -- see the note in `encode`.
    let (packet_tx, packet_rx) = tokio::sync::mpsc::unbounded_channel();
    let frames = capture.frames;
    std::thread::spawn(move || {
        if let Err(e) = encode::run(frames, packet_tx) {
            tracing::error!(error = %e, "encoder stopped");
        }
    });

    let signalling = signal::connect(&args.broker, args.broker_port, &args.room, session::ROLE).await?;
    let (pc, track) = session::build(&args.stun).await?;
    session::run(pc, track, packet_rx, signalling).await
}
