# PLUGIN.SOUNDCARD_IN architecture

How to use it, and how it works inside. Flags and topics are tabled in the [repo README](../../README.md);
the receiving half is `PLUGIN.SOUNDCARD_OUT`.

← [Manual and Artifacts](INDEX.md)

## Use

```sh
# from PODS/POD.THICK/DISCOVERY.Engine/SRC (the cargo workspace)
cargo test -p apkaudio-soundcard-to-webrtc
cargo run -p apkaudio-soundcard-to-webrtc -- --list-devices
cargo run -p apkaudio-soundcard-to-webrtc -- --device USB --room studio-a
COMPOSE_PROFILES=sound docker compose -f PODS/POD.THICK/PLUGINS/PLUGIN.SOUNDCARD_IN/Docker/docker-compose.yml up -d --build
```

Start `PLUGIN.SOUNDCARD_OUT` with the same `--room` (compose: `APK_SOUNDCARD_ROOM`), in either order.

## Entry points

| File | What |
|---|---|
| [`SRC/src/main.rs`](../../SRC/src/main.rs) | args, room check, card open loop, encoder thread, then signalling and the session |
| [`SRC/src/devices.rs`](../../SRC/src/devices.rs) | `list`, `resolve` (exact name, else unambiguous substring, else host default) |
| [`SRC/src/capture.rs`](../../SRC/src/capture.rs) | cpal input stream; `drain` conforms each callback to 48 kHz stereo and cuts 20 ms frames |
| [`SRC/src/format.rs`](../../SRC/src/format.rs) | `conform`: rate and channel conversion; the 48 kHz / 2 ch / 20 ms constants |
| [`SRC/src/encode.rs`](../../SRC/src/encode.rs) | `run`: frames → Opus packets on its own thread |
| [`SRC/src/signal.rs`](../../SRC/src/signal.rs) | MQTT signalling: `Signal` (`hello` / `offer` / `answer` / `ice`), `topics_for`, `connect` |
| [`SRC/src/session.rs`](../../SRC/src/session.rs) | `build` (peer connection + one Opus track), `run` (offer, re-offer on `hello`, apply answer, candidates, pump) |
| [`SRC/src/card_state.rs`](../../SRC/src/card_state.rs) | retained `incoming/state` + `incoming/detail` for the card; kept identical in SOUNDCARD_OUT |

## Data flow

```
cpal callback ─▶ capture::drain (conform, 20 ms frames) ─▶ sync channel
   ─▶ encode thread (Opus) ─▶ unbounded channel ─▶ session pump ─▶ WebRTC track ─▶ peer
MQTT  APK.audio/Audio/WebRTC/{room}/offerer   ◀─ offer, ice        (this side says)
      APK.audio/Audio/WebRTC/{room}/answerer  ─▶ hello, answer, ice (this side hears)
```

1. The card is opened in a loop; until it opens, `card_state` says `unavailable` and why.
2. Once capturing, `listening` is retained, the encoder thread starts, signalling connects and the
   offer is said.
3. A `hello` before the answer re-says the offer; a `hello` after it ends the call (the old peer is
   gone). The call also ends when the encoder stops. Ending is a process exit; Docker restarts it.
