# Principles PLUGIN.SOUNDCARD_IN keeps

Each rule is pointed at the code that keeps it; where the code bends one, it says so. Ecosystem
rules live in the root: [Doctrine](../../../../../../Documentation/%F0%9F%8F%9BPhilosophy/Doctrine/readme.Doctrine.md),
[Dockers](../../../../../../Documentation/%F0%9F%8F%9BPhilosophy/Dockers/readme.Dockers.md).

← [Philosophy](INDEX.md) · [Repo README](../../README.md)

| Principle | Kept by |
|---|---|
| **One card, one peer, one direction.** Capture only, one process per card; it does not mix and owns no broker. | crate header, [`SRC/src/lib.rs`](../../SRC/src/lib.rs). |
| **The bus already exists — signal over it.** SDP and ICE travel over MQTT, not a second HTTP control plane. | [`SRC/src/signal.rs`](../../SRC/src/signal.rs) header, `connect` (`:115`). |
| **A retained offer is a stale offer — so the receiver asks.** Signalling is never retained; a receiver says `hello` and this side says its offer again, so a late or restarted receiver still connects. | `Signal::Hello`, `signal.rs`; `session::run`, [`SRC/src/session.rs:64`](../../SRC/src/session.rs); test `hello_is_a_bare_type`. |
| **The sender offers.** It is the side with media, so it knows when a session is worth having; ICE is trickled, not gathered first. | `session::run`, `publish_candidates` (`session.rs:147`). |
| **Say the state, do not crash-loop.** A card that will not open is `unavailable` + why + which devices would open, retried every 30 s. | [`SRC/src/card_state.rs`](../../SRC/src/card_state.rs) (`RETRY`, `:25`); the open loop in `main.rs`. |
| **No silent success.** An encoder that stops closes the packet channel, and the call ends with an error rather than staying up `listening` with no audio. | the `tokio::select!` on the pump in `session::run`. |
| **One statement of a topic.** Topics come from `apkaudio_contracts::topics`; a room that is not one segment is refused before a card is opened. | `topics_for`, `check_room` (`signal.rs:69`, `:86`); test `a_room_that_is_not_one_segment_is_refused`. |
| **On the runner, like every plugin.** The runner owns config, last will, `incoming/api`, telemetry and the self-test door; this crate owns the card and the call. Its card-state and signalling sessions are opened with the runner's `Session` (roles `card`, `signal-offerer`: unique client ids, the one credentials read), so they never kick the runner's own session off the broker. | [`SRC/src/main.rs`](../../SRC/src/main.rs) `apk_plugin_runner::serve`; [`SRC/src/self_test.rs`](../../SRC/src/self_test.rs); `CardState::connect`, `signal::connect`. |
| **One copy of the link, both halves.** `signal`, `card_state` and `settings` are the WebRTC link; PLUGIN.SOUNDCARD_OUT takes them from this crate by git rev (the engine `[patch]` points it back here), so the protocol is changed once. | [`SRC/src/lib.rs`](../../SRC/src/lib.rs); SOUNDCARD_OUT `SRC/src/lib.rs` `pub use`. |
| **A versioned Published Language.** Every signalling message carries `v` (`SIGNAL_VERSION` = 1); no `v` reads as 1, a newer `v` is refused, never half-read. | `encode` / `decode`, [`SRC/src/signal.rs`](../../SRC/src/signal.rs); tests `the_wire_form_is_the_published_one`, `versions_are_read_honestly`. |
| **Opus off the async runtime.** Encoding runs on its own OS thread, never in cpal's callback or on a tokio worker. | [`SRC/src/encode.rs`](../../SRC/src/encode.rs) header; the thread spawn in [`SRC/src/main.rs`](../../SRC/src/main.rs). |

## Where the code bends a rule

* **Declared config is read config — no longer bent** (LOOSE-soundcard-config-unread). [`SRC/config.ini`](../../SRC/config.ini)
  gives `room`, `stun` and `device` as defaults, read by [`SRC/src/settings.rs`](../../SRC/src/settings.rs);
  argv and `APK_SOUNDCARD_DEVICE` override them, and the config the runner publishes retained is
  the file rendered with the values the process runs with.

## Kept by design (audit 2026-10-30)

* **Three broker sessions per process.** The runner's session is synchronous and owns the plugin's
  last will (RP-01); the card state and the signalling run on tokio and need an `AsyncClient`. They
  are separate connections, but one constructor: each takes its options from
  `apk_plugin_runner::session::Session`, so client id, credentials and keep-alive are decided once.
* **The image builds the workspace `[patch]`, not the tag.** `SRC/Cargo.toml` names the SDK by
  `sdk-v0.5` so the repository builds alone; inside the monorepo DISCOVERY.Engine's `[patch]` and
  `members/` link build the local runner and contracts (`.apk.scripts/plugin_standalone.py`
  docstring). That is the house pattern for every plugin, not a gap in this one.
* **No Dockerfile; compose `extends` `plugin-base.yml`.** One plugin image and one base service per
  node (DISCOVERY.Engine), so the capability floor, broker variables and healthcheck are stated once.
