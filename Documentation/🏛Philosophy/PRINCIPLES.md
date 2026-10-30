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
| **On the runner, like every plugin.** The runner owns config, last will, `incoming/api`, telemetry and the self-test door; this crate owns the card and the call. Its status session uses the client id `apk-plugin-soundcard_in-card-<pid>` so it never kicks the runner's `apk-plugin-soundcard_in-<pid>` off the broker. | [`SRC/src/main.rs`](../../SRC/src/main.rs) `apk_plugin_runner::serve`; [`SRC/src/self_test.rs`](../../SRC/src/self_test.rs); `CardState::connect`. |
| **Opus off the async runtime.** Encoding runs on its own OS thread, never in cpal's callback or on a tokio worker. | [`SRC/src/encode.rs`](../../SRC/src/encode.rs) header; the thread spawn in [`SRC/src/main.rs`](../../SRC/src/main.rs). |

## Where the code bends a rule

* **Declared config is read config.** [`SRC/config.ini`](../../SRC/config.ini) is published by the
  runner but no key in it is read; it describes the flags.
