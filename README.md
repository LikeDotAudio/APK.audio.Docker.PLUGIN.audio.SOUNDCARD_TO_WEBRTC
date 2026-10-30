# APK:plugin:SOUNDCARD_IN

Capture a sound card and offer it over WebRTC, one container. One card to one
peer: cpal capture → conform to 48 kHz stereo → Opus → one WebRTC audio track,
with the SDP and ICE carried over MQTT. The receiving half is
`PLUGIN.SOUNDCARD_OUT`.

**Wiki:** [Documentation/README.md](Documentation/README.md) — principles, architecture and delivery, each checked against the code.

| | |
|---|---|
| **Recipe** | `Docker/docker-compose.yml` — no Dockerfile: the service `soundcard-in` `extends` `plugin-host` in `PODS/POD.THICK/DISCOVERY.Engine/Docker/plugin-base.yml` and runs a binary from the shared plugin image `DISCOVERY.Engine/Docker/Dockerfile` builds (it links `libasound2` and `libopus`) |
| **Container** | `Plugin-SOUNDCARD_IN`, compose project `apk-audio`, profile `sound`, host networking (ICE), `/dev/snd` + group `audio`, the base `apk-plugin-health` healthcheck |
| **Source** | `SRC/` (this repository — https://github.com/LikeDotAudio/APK.audio.Docker.PLUGIN.audio.SOUNDCARD_TO_WEBRTC.git) — crate `apkaudio-soundcard-to-webrtc`, bin `soundcard-to-webrtc` (`SRC/src/main.rs`), a member of the `DISCOVERY.Engine/SRC` cargo workspace |
| **Talks to** | the broker (signalling and state), the STUN server (`--stun`, default `stun:stun.l.google.com:19302`), and the answering peer over WebRTC |

A real binary that no Dockerfile built and no roster ran until PLAN-3316.01.
Moved into its own container by PLAN-3316.01.

## Run, build, test

```bash
COMPOSE_PROFILES=sound docker compose -f 'PODS/POD.THICK/PLUGINS/PLUGIN.SOUNDCARD_IN/Docker/docker-compose.yml' up -d --build

# From PODS/POD.THICK/DISCOVERY.Engine/SRC (the workspace root):
cargo test -p apkaudio-soundcard-to-webrtc
cargo run -p apkaudio-soundcard-to-webrtc -- --list-devices    # every input device, then exit
cargo run -p apkaudio-soundcard-to-webrtc -- --help
```

| Flag / variable | Default | What |
|---|---|---|
| `--device` / `APK_SOUNDCARD_DEVICE` | the host default | input device, exact name or unambiguous substring; the flag wins, empty means default |
| `--broker`, `--broker-port` | `MQTT_HOST` / `MQTT_PORT`, else `127.0.0.1`, `1883` | compose passes `APK_BAREMETAL_MQTT_HOST` / `_PORT`; given, they are written to `MQTT_HOST` / `MQTT_PORT` so the runner's session and the signalling share one broker |
| `--room` / `APK_SOUNDCARD_ROOM` (compose) | `default` | one topic segment; the receiver must use the same room |
| `--stun` | `stun:stun.l.google.com:19302` | ICE server |

The binary takes everything on argv and the environment. `SRC/config.ini` is
compiled in and published retained by `apk-plugin-runner` at
`…/soundcard_in/config`; nothing in the crate reads a key from it.

## On the bus

| Topic | Kept? | What |
|---|---|---|
| `APK.audio/Audio/WebRTC/{room}/offerer` | no | this side's offer and ICE candidates (`{"type": "offer"}` / `"ice"`) |
| `APK.audio/Audio/WebRTC/{room}/answerer` | no | subscribed: the peer's `hello`, answer and candidates |
| `APK.audio/System/Protocols/soundcard_in/incoming/state` | retained | `listening` once capturing, `unavailable` while no card opens — also the last will |
| `APK.audio/System/Protocols/soundcard_in/incoming/detail` | retained | why: the device opened, or the error and the devices that would open |

Topics are built through `apkaudio_contracts::topics` / `baremetal_io`
(`SRC/src/signal.rs`, `SRC/src/card_state.rs`), never formatted in the crate.
It is built on `apk-plugin-runner` (PLAN-3402.01): the runner publishes the
retained config, `incoming/api` (with this crate's own `build.plugin_version`),
telemetry and the health file, answers `selftest_run` through
`SRC/src/self_test.rs`, and runs the card and the call as its one agent,
`run_capture`. A call that ends returns the agent, and the runner exits so
Docker restarts the container.

## When something goes wrong

- **No card that opens is a state, not a crash** (`SRC/src/card_state.rs`).
  The plugin says `unavailable` + why + which devices would open, and retries
  every 30 s (`RETRY`), so a card plugged in later is picked up.
- The status session's broker is retried every 2 s, never fatal.
- A bad `--room` (more than one topic segment) is refused before any card is
  opened (`signal::check_room`).
- A dropped ICE candidate costs a route, not the call (`publish_candidates`).
- If the Opus encoder thread stops (`encode::run` errors, or capture ends),
  the packet channel closes and `session::run` ends the call with an error,
  so the container restarts rather than staying up `listening` with no audio.
- A signalling or peer-connection error after capture starts returns from
  `main` with exit 1; the base service's `restart: unless-stopped` restarts the
  container.
- The offer is unretained, so a receiver asks: it says `{"type": "hello"}` on
  its `answerer` topic once subscribed, and this side says its offer again
  (`session::run`). A `hello` while a call is up means a new receiver — the
  call ends, the process exits and the container restarts with a fresh offer.

## Philosophy

- **One card, one peer, one direction.** Capture only; it does not mix, does
  not own a broker, and runs one process per card (`SRC/src/lib.rs`).
- **The bus already exists — signal over it.** SDP and ICE travel over MQTT
  rather than a second HTTP control plane to secure and discover
  (`SRC/src/signal.rs`).
- **A retained offer is a stale offer.** Nothing in signalling is retained
  (`SRC/src/signal.rs`); only the conclusions `state` and `detail` are.
- **Say the state, do not crash-loop.** A card that will not open is reported
  on the bus and retried (`SRC/src/card_state.rs`, measured 2026-10-25 against
  a restart every ~60 s).
- **One statement of a topic.** Topics come from the contracts crate, not a
  local `format!` (`signal::topics_for`).
- **The sender offers.** It is the side with media, so it knows when a session
  is worth having (`SRC/src/session.rs`); ICE is trickled, not gathered first.
- **Opus off the async runtime.** Encoding runs on its own thread
  (`SRC/src/encode.rs`, `SRC/src/main.rs`).

Twin: `PLUGIN.SOUNDCARD_OUT` — `card_state.rs` is kept identical in both.

## Plans

Plans live only in `Documentation/🛣Roadmap/roadmap.htm` at the root of APK.audio.

---

*© 2026 — Written & Maintained by Anthony P. Kuzub. MIT Licence.*

<!-- wiki:smoke -->
**Smoke test** — ✅ PASS 2/2 since 2026-10-30 18:01 EDT — [results](Documentation/%F0%9F%A7%A9Manual%20and%20Artifacts/SMOKE.md)
<!-- /wiki:smoke -->
