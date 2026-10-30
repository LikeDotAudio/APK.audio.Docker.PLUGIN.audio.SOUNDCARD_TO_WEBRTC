// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! soundcard_in's self test — the door `apk-plugin-runner` answers `selftest_run` with.
//!
//! PLAN-3402.01 put this crate on the runner, and the runner needs a self-test
//! door. At [`Depth::Own`] nothing leaves the process: no card is opened and
//! no broker is asked — only what this build can say about itself.
//!
//! Twin: PLUGIN.SOUNDCARD_OUT/SRC/src/self_test.rs — keep the two in step.

use apkaudio_contracts::self_test::{Check, Depth, Report};

use crate::{format, session, signal};

/// The topic segment this crate owns on the bus.
pub const MODULE: &str = "soundcard_in";

pub fn self_test(depth: Depth) -> Report {
    let mut report = Report::new(MODULE, depth);

    report.record(Check::from_result(
        "the_default_room_is_one_topic_segment",
        Depth::Own,
        "the room compose passes by default builds a signalling topic the peer can see",
        signal::check_room("default", session::ROLE).map_err(|e| format!("{e:#}")),
    ));

    report.record(Check::from_result(
        "a_bad_room_is_refused",
        Depth::Own,
        "a room with a slash is refused before a card is opened",
        match signal::check_room("a/b", session::ROLE) {
            Ok(()) => Err("`a/b` was accepted as a room".to_string()),
            Err(_) => Ok(()),
        },
    ));

    report.record(Check::from_result(
        "the_frame_is_48k_stereo_20ms",
        Depth::Own,
        "the frame shape is the WebRTC Opus default the SDP names",
        if format::SAMPLE_RATE == 48_000 && format::CHANNELS == 2 && format::FRAME_INTERLEAVED == 1920 {
            Ok(())
        } else {
            Err(format!(
                "{} Hz x {} ch, {} values per frame",
                format::SAMPLE_RATE,
                format::CHANNELS,
                format::FRAME_INTERLEAVED
            ))
        },
    ));

    report.record(Check::from_result(
        "48k_stereo_is_not_resampled",
        Depth::Own,
        "a card already at 48 kHz stereo takes the identity path",
        {
            let input: Vec<i16> = (0..format::FRAME_INTERLEAVED as i16).collect();
            let out = format::conform(&input, format::SAMPLE_RATE, format::CHANNELS);
            if out == input { Ok(()) } else { Err(format!("{} values in, {} out, not identical", input.len(), out.len())) }
        },
    ));

    report
}

pub fn self_test_json(depth_word: &str) -> String {
    apkaudio_contracts::self_test::run_as_json(MODULE, depth_word, self_test)
}

#[cfg(test)]
mod tests {
    use super::*;
    use apkaudio_contracts::self_test::Verdict;

    #[test]
    fn the_self_test_passes_against_this_build() {
        let report = self_test(Depth::Own);
        assert_eq!(report.verdict(), Verdict::Pass, "{}", report.headline());
    }

    #[test]
    fn the_module_name_is_a_legal_topic_segment() {
        assert!(apkaudio_contracts::topics::is_segment(MODULE));
    }

    #[test]
    fn the_json_door_answers_what_the_typed_one_does() {
        let typed = self_test(Depth::Own);
        let parsed = Report::from_json(&self_test_json("self")).expect("the door emits a readable report");
        assert_eq!(parsed.verdict(), typed.verdict());
        assert_eq!(parsed.counts(), typed.counts());
    }
}
