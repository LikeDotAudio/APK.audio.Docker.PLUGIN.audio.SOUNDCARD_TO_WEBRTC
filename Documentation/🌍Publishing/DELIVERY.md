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
  workflow; no test exercises a real card or a real call.
* No release artefact or version: the image is built locally on each node (`up -d --build`).
* The STUN server defaults to a public Google one (`--stun`); nothing provisions a local one.
