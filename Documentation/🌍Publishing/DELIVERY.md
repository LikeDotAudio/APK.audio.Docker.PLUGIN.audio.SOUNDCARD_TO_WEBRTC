# PLUGIN.SOUNDCARD_IN delivery

← [Publishing](INDEX.md) · [Repo README](../../README.md)

## Built and tested

The crate `apkaudio-soundcard-to-webrtc` (`SRC/`) is a member of the plugin workspace
`PODS/POD.THICK/DISCOVERY.Engine/SRC`: `cargo test -p apkaudio-soundcard-to-webrtc` there. The
binary `soundcard-to-webrtc` is compiled with every other plugin into the shared image
`apk-plugins:local` by [`DISCOVERY.Engine/Docker/Dockerfile`](../../../../DISCOVERY.Engine/Docker/Dockerfile),
whose build stage installs `libasound2-dev` and `libopus-dev` for it. This repo has no Dockerfile.

## Shipped

As the container `Plugin-SOUNDCARD_IN` from [`Docker/docker-compose.yml`](../../Docker/docker-compose.yml):
service `soundcard-in`, profile `sound`, host network (ICE), `/dev/snd` and group `audio`,
`extends: plugin-host` in [`plugin-base.yml`](../../../../DISCOVERY.Engine/Docker/plugin-base.yml).
The broker and room are passed on the command line from `APK_BAREMETAL_MQTT_HOST` / `_PORT` and
`APK_SOUNDCARD_ROOM`; the device from `APK_SOUNDCARD_DEVICE`. `DISCOVERY.Engine/Docker/docker-compose.yml`
`include:`s the file into the project `apk-audio`, so it starts on a node whose role list names `sound`.

It lands on the node's broker (`APK.audio/Audio/WebRTC/{room}/offerer`, unretained, and
`APK.audio/System/Protocols/soundcard_in/incoming/{state,detail}`, retained) and as an Opus WebRTC
track to the peer. There is no live URL. The repository
(https://github.com/LikeDotAudio/APK.audio.Docker.PLUGIN.audio.SOUNDCARD_TO_WEBRTC.git) is pushed with
every other submodule by `.apk.scripts/deploy_all.py` at the APK.audio root.

## Not automated

* CI ([`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)) runs the SDK's reusable plugin
  workflow pinned at `@sdk-v0.7`; no test exercises a real card. The offer/answer contract runs in
  process (SOUNDCARD_OUT `tests/offer_answer.rs`), not over a broker.
* No release artefact or version: the image is built locally on each node (`up -d --build`).
* The STUN server defaults to a public Google one (`--stun`); nothing provisions a local one.

## Delivered from the 2026-10-30 audit

Part of the plugin image (`apk-plugins:local`, DISCOVERY.Engine) as `Plugin-SOUNDCARD_IN`; the code
reaches a running container on the next `up-stack.sh` of the plugins (needs owner).

- **One shared WebRTC link** — `SRC/src/signal.rs`, `card_state.rs` and `settings.rs` are the only copy; PLUGIN.SOUNDCARD_OUT deletes its three and `pub use`s them from this crate by git rev (sibling convention of `plugin_standalone.py`). Verified: `cargo test -p apkaudio-soundcard-to-webrtc` (23 passed) and SOUNDCARD_OUT's tests against it. The engine `[patch]` line for this repository's URL is a handoff to DISCOVERY.Engine; the pinned rev must be pushed before a standalone SOUNDCARD_OUT build fetches it (needs owner).
- **Offer/answer contract test** — `signal::loopback()` joins an offerer and an answerer in process through the versioned wire form; SOUNDCARD_OUT's `tests/offer_answer.rs` runs this crate's offerer against its answerer through hello → offer → answer → ice. Unit test `the_loopback_pair_faces_itself` here.
- **Crossed offer fixed** — the contract test found that the answerer's `hello` and the first offer can cross, the offer is said twice and answered twice; `session::run` now ignores a second answer once negotiated instead of failing a good call (SOUNDCARD_OUT ignores a repeated identical offer). Contract test passed 6/6 runs after.
- **Versioned signalling** — `{"v":1,"type":…}`; `encode`/`decode`, `SIGNAL_VERSION`; missing `v` = 1, newer refused (tests `the_wire_form_is_the_published_one`, `versions_are_read_honestly`); documented in ARCHITECTURE.md as the WebRTC signalling Published Language.
- **CI pinned** — `.github/workflows/ci.yml` uses `plugin-ci.yml@sdk-v0.5`. Standalone CI of the compose include and `Session` needs the next SDK tag (publishing tags: needs owner).
- **One session constructor** — card state and signalling take their options from `apk_plugin_runner::session::Session` (unique client ids, credentials); the signalling resubscribes on every ConnAck. Kept by design: separate connections (the runner's sync client owns the will; these are tokio) — PRINCIPLES.md.
- **Shared volumes included** — `Docker/docker-compose.yml` includes `DISCOVERY.Engine/Docker/plugin-volumes.yml` instead of a copied block; `docker compose config` byte-identical before and after.
- **Image builds the patched workspace** — Kept by design: the engine `[patch]` + `members/` link is the house pattern (PRINCIPLES.md).
- **Compose `extends` plugin-base.yml** — Kept by design: one plugin image and base service per node (PRINCIPLES.md).
- **SDK tag `sdk-v0.7`** (0388231 → bbde443) — `SRC/Cargo.toml` (runner and contracts) and `.github/workflows/ci.yml` (`plugin-ci.yml@sdk-v0.7`). v0.7 carries the runner's `session`, `unkept` and `docker/plugin-volumes.yml`, so the standalone build the items above waited on now works. Verified 2026-10-31: `git archive HEAD`, extracted alone, `cd SRC && cargo test` 23 passed. SOUNDCARD_IN is pushed (remote HEAD bbde443); PLUGIN.SOUNDCARD_OUT still pins this repository at `c98eba6`, which builds on `sdk-v0.5`, so its standalone build fails until it takes bbde443.
