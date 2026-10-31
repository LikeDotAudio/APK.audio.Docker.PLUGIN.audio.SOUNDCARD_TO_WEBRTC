# 🔍 Architecture audit — PLUGIN.SOUNDCARD_IN (2026-10-30)

Audited against the APK.audio multi-repo architecture standard: coupling, boundaries, duplication, contracts.
← [Audits](INDEX.md) · Improvements: [🛣Roadmap/IMPROVEMENTS.md](../%F0%9F%9B%A3Roadmap/IMPROVEMENTS.md)

## Bounded context
Capture one sound card and offer it as one Opus WebRTC track, signalled over MQTT (the sending half; PLUGIN.SOUNDCARD_OUT answers). Rust crate `apkaudio-soundcard-to-webrtc`, 11 source files, 1,264 lines; container `Plugin-SOUNDCARD_IN`. All source, compose, config and CI were read.

## Context map
| Talks to | How (topic / endpoint / file / import) | Relationship |
|---|---|---|
| APK.plugin.SDK (`apkaudio-contracts`, `apk-plugin-runner`) | git dep `tag = "sdk-v0.5"` (`SRC/Cargo.toml:8-9`) | Conformist to a versioned SDK |
| PLUGIN.SOUNDCARD_OUT | `APK.audio/Audio/WebRTC/<room>/offerer` ↔ `…/answerer`, built by `topics::webrtc_signal` (`SRC/src/signal.rs:68-79`) | Partnership (shared signalling protocol) |
| DISCOVERY.Engine | compose `extends: ../../../DISCOVERY.Engine/Docker/plugin-base.yml` (`Docker/docker-compose.yml:18-19`); binary built in its shared plugin image | Customer–Supplier by file path |
| Broker / shell | `APK.audio/System/Protocols/soundcard_in/{config,status,host,incoming/state,incoming/detail}` (`SRC/config.ini`, `SRC/src/card_state.rs`) | Published Language |
| STUN server | `stun:stun.l.google.com:19302` (`SRC/config.ini:21`) | external |
| APK.plugin.SDK CI | `uses: …/plugin-ci.yml@main` (`.github/workflows/ci.yml:5`) | Conformist |

## Scorecard
| Category | Score (0–3) | Finding |
|---|---|---|
| Coupling & Cohesion | 3 | SDK by tag; compose `extends` plugin-base.yml and the patched image build are the documented house pattern (PRINCIPLES.md) |
| Boundary Integrity | 3 | each vendor library has one module (cpal `capture.rs`, audiopus `encode.rs`, webrtc `session.rs`, rumqttc `signal.rs`/`card_state.rs`); topics come from the contracts; sessions from the runner's `Session` |
| Duplication Control | 3 | `signal`, `card_state`, `settings` exist once, here; SOUNDCARD_OUT re-exports them |
| Testing & Contracts | 3 | unit tests pin topic shapes and the versioned wire form; in-process offer/answer contract test (SOUNDCARD_OUT `tests/offer_answer.rs`) |
| Change management | 3 | SDK and CI pinned to `sdk-v0.5`; signalling carries `v` |
Score key: 3 meets the standard · 2 minor gaps · 1 significant gaps · 0 absent / violated.

## Findings
1. **Good: versioned SDK dependency.** `SRC/Cargo.toml:8-9` names `apkaudio-contracts` and `apk-plugin-runner` by `tag = "sdk-v0.5"`; the history shows each SDK release pinned (`chore: pin APK.plugin.SDK sdk-v0.5`).
2. **Good: topics through the contract.** `SRC/src/signal.rs` builds both signalling topics with `apkaudio_contracts::topics::webrtc_signal` and validates `--room` as one segment. Tests assert the shapes.
3. **Good: config honesty.** `SRC/config.ini` declares no `topic_listen`/`topic_publish` it does not read — consistent with the bus-ledger rule.

Delivered items are recorded in [🌍Publishing/DELIVERY.md](../%F0%9F%8C%8DPublishing/DELIVERY.md).
