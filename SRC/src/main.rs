// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! `soundcard-to-webrtc` -- capture one card and offer it to a peer.
use anyhow::{Context, Result};
use apkaudio_contracts::agent_state::AgentState;
use apkaudio_soundcard_to_webrtc::{capture, card_state, devices, encode, session, signal};
use clap::Parser;
use cpal::traits::DeviceTrait;

#[derive(Parser)]
#[command(about = "Capture a system sound card and send it as Opus over WebRTC")]
struct Args {
    /// Print every input device this host exposes, then exit.
    #[arg(long)]
    list_devices: bool,

    /// Input device, by exact name or unambiguous substring. Default: APK_SOUNDCARD_DEVICE, else the host default.
    #[arg(long)]
    device: Option<String>,

    /// Broker. Default: MQTT_HOST, else 127.0.0.1 — the runner's own rule.
    /// Given here, it wins over MQTT_HOST for the runner's session too, so the
    /// signalling and the plugin's own topics are always on one broker.
    #[arg(long)]
    broker: Option<String>,

    /// Broker port. Default: MQTT_PORT, else 1883.
    #[arg(long)]
    broker_port: Option<u16>,

    /// Room name. The receiver must be given the same one.
    #[arg(long, default_value = "default")]
    room: String,

    #[arg(long, default_value = "stun:stun.l.google.com:19302")]
    stun: String,
}

/// The module token on the bus: `APK.audio/System/Protocols/soundcard_in/…`.
const TOKEN: &str = "soundcard_in";

/// ON apk-plugin-runner (PLAN-3402.01). The runner owns the plugin's retained
/// config, the last will, the self-test door and `incoming/api`; this binary
/// owns the card and the call. The call runs as the runner's one agent, so a
/// call that ends is an exit the container restarts — as it was before.
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
    let args = Args::parse();

    if args.list_devices {
        match devices::list() {
            Ok(list) => {
                for d in list {
                    println!("{}  ({} Hz, {} ch)", d.name, d.default_rate, d.default_channels);
                }
            }
            Err(e) => {
                eprintln!("🛑 {e:#}");
                std::process::exit(1);
            }
        }
        return;
    }

    // A room name is ONE topic segment. Refuse a bad one HERE, before a card is
    // opened: `--room "a/b"` builds a topic two segments deep, the peer
    // subscribing to `{room}/{role}` never sees it, and the call would simply
    // not connect with nothing logged anywhere.
    if let Err(e) = signal::check_room(&args.room, session::ROLE) {
        eprintln!("🛑 {e:#}");
        std::process::exit(2);
    }

    // `--broker` / `--broker-port` STILL WORK: the runner reads MQTT_HOST and
    // MQTT_PORT, so argv is written there before it does. SAFETY: no thread has
    // been started yet — the runner and the tokio runtime both start below.
    if let Some(host) = &args.broker {
        unsafe { std::env::set_var("MQTT_HOST", host) };
    }
    if let Some(port) = args.broker_port {
        unsafe { std::env::set_var("MQTT_PORT", port.to_string()) };
    }

    // This crate's own name and version into `incoming/api` (PLAN-3407.01).
    apk_plugin_runner::stamp!();
    apk_plugin_runner::serve(
        apk_plugin_runner::Plugin {
            token: TOKEN,
            config_ini: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config.ini")),
            self_test: apkaudio_soundcard_to_webrtc::self_test::self_test_json,
        },
        vec![(
            "run_capture",
            Box::new(move |host: String, port: u16| {
                let runtime = match tokio::runtime::Runtime::new() {
                    Ok(runtime) => runtime,
                    Err(e) => {
                        eprintln!("🛑 [{TOKEN}] no tokio runtime: {e}");
                        return;
                    }
                };
                if let Err(e) = runtime.block_on(call(args, host, port)) {
                    eprintln!("🛑 [{TOKEN}] {e:#}");
                }
            }) as Box<dyn FnOnce(String, u16) + Send>,
        )],
    )
}

/// The card and the call, on the broker the runner resolved.
async fn call(args: Args, broker: String, broker_port: u16) -> Result<()> {
    // NO CARD IS A STATE, NOT A CRASH — see `card_state`.
    let status = card_state::CardState::connect(TOKEN, &broker, broker_port).await?;
    let mut said = String::new();
    // `--device`, else APK_SOUNDCARD_DEVICE (the compose file's knob). Empty —
    // compose's default — is no choice at all: the host default.
    let from_env = std::env::var("APK_SOUNDCARD_DEVICE").ok();
    let wanted = args.device.as_deref().or(from_env.as_deref()).filter(|d| !d.trim().is_empty());
    let (name, capture) = loop {
        let opened = devices::resolve(wanted).and_then(|device| {
            let name = device.name().unwrap_or_else(|_| "<unnameable>".to_owned());
            capture::start(&device).context("could not start capture").map(|c| (name, c))
        });
        match opened {
            Ok(pair) => break pair,
            Err(e) => {
                let why = format!("{e:#}");
                if why != said {
                    let names: Vec<String> = devices::list().unwrap_or_default().into_iter().map(|d| d.name).collect();
                    let detail = format!(
                        "no capture: {why} — wanted {}; set APK_SOUNDCARD_DEVICE to one of: {}; retrying every {}s",
                        wanted.unwrap_or("the host default"),
                        card_state::suggestions(&names),
                        card_state::RETRY.as_secs()
                    );
                    tracing::warn!("{detail}");
                    status.say(AgentState::Unavailable, &detail).await;
                    said = why;
                }
                tokio::time::sleep(card_state::RETRY).await;
            }
        }
    };
    tracing::info!(device = %name, rate = capture.source_rate, channels = capture.source_channels, "capturing");
    status.say(AgentState::Listening, &format!("capturing from {name}")).await;

    // Opus runs on its own thread -- see the note in `encode`.
    let (packet_tx, packet_rx) = tokio::sync::mpsc::unbounded_channel();
    let frames = capture.frames;
    std::thread::spawn(move || {
        if let Err(e) = encode::run(frames, packet_tx) {
            tracing::error!(error = %e, "encoder stopped");
        }
    });

    let signalling = signal::connect(&broker, broker_port, &args.room, session::ROLE).await?;
    let (pc, track) = session::build(&args.stun).await?;
    session::run(pc, track, packet_rx, signalling).await
}
