# APK:plugin:SOUNDCARD_IN

Capture a sound card and offer it over WebRTC, one container.

| | |
|---|---|
| **Recipe** | `Docker/docker-compose.yml` — no Dockerfile: it runs a binary from the shared plugin image built by `APK:discovery` |
| **Source** | `SRC/` (this repository — https://github.com/LikeDotAudio/APK.audio.Docker.PLUGIN.audio.SOUNDCARD_TO_WEBRTC.git) — bin `soundcard-to-webrtc`; built by the shared plugin image |
| **Profile** | `sound` |
| **Talks to** | the broker only |

A real binary that no Dockerfile built and no roster ran until PLAN-3316.01. Host networking for WebRTC ICE. Moved into its own container by PLAN-3316.01.

---

*© 2026 — Written & Maintained by Anthony P. Kuzub. MIT Licence.*
