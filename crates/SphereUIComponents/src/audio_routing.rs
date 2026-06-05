//! Audio input/output **channel route options** derived from a device's channel
//! count (roadmap Phase B).
//!
//! The engine (`SphereDirectAudioEngine`) enumerates devices and reports a
//! channel *count* per device (`JsAudioDeviceInfo.channels`). This module turns
//! that count into the concrete, selectable routes a DAW exposes — "Input 1",
//! "Input 2", "Input 1+2 (Stereo)", … — so both Preferences > Audio (Phase C)
//! and the Track Inspector (Phase E) build their selectors from one source of
//! truth instead of hardcoded placeholders.
//!
//! Pure data logic: no GPUI, no device I/O, fully unit-tested. The runtime
//! recording config already takes a `Vec<u32>` of source channel indices
//! (`JsRecordingTrackConfig.input_channels`), so [`AudioRouteOption::channels`]
//! maps straight through — the full `AudioChannelSelection` stereo-pair model
//! (roadmap Phase D) can be layered on later without changing this builder.

/// `FUTUREBOARD_AUDIO_DEVICE_DEBUG=1` enables device/channel enumeration traces.
/// Cached on first read.
pub fn audio_device_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_AUDIO_DEVICE_DEBUG").is_some())
}

/// One selectable hardware route derived from a device's channel count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRouteOption {
    /// Stable id for combo selection state, e.g. `ch:0`, `ch:0+1`, `all`.
    pub id: String,
    /// Human label, e.g. `Input 1`, `Input 1+2 (Stereo)`, `All Inputs`.
    pub label: String,
    /// 0-based source channel indices this route captures. One entry = mono,
    /// two = stereo pair, more = multi/all.
    pub channels: Vec<u32>,
}

impl AudioRouteOption {
    fn mono(prefix: &str, ch: u32) -> Self {
        Self {
            id: format!("ch:{ch}"),
            label: format!("{prefix} {}", ch + 1),
            channels: vec![ch],
        }
    }

    fn pair(prefix: &str, left: u32, right: u32) -> Self {
        Self {
            id: format!("ch:{left}+{right}"),
            label: format!("{prefix} {}+{} (Stereo)", left + 1, right + 1),
            channels: vec![left, right],
        }
    }

    fn all(prefix: &str, count: u32) -> Self {
        Self {
            id: "all".to_string(),
            label: format!("All {prefix}s"),
            channels: (0..count).collect(),
        }
    }
}

/// Build input route options for a device with `channel_count` input channels.
///
/// - `0`  → empty (caller should show "No input channels").
/// - `1`  → `Input 1`.
/// - `2`  → `Input 1`, `Input 2`, `Input 1+2 (Stereo)`.
/// - `>2` → all mono channels, then consecutive stereo pairs, then `All Inputs`.
pub fn build_input_channel_options(channel_count: u32) -> Vec<AudioRouteOption> {
    build_channel_options("Input", channel_count)
}

/// Build output route options for a device with `channel_count` output channels.
/// Same shape as [`build_input_channel_options`] but labelled `Output`. The
/// logical `Main` / bus targets are prepended by the UI, not here.
pub fn build_output_channel_options(channel_count: u32) -> Vec<AudioRouteOption> {
    build_channel_options("Output", channel_count)
}

fn build_channel_options(prefix: &str, channel_count: u32) -> Vec<AudioRouteOption> {
    let mut out = Vec::new();
    if channel_count == 0 {
        return out;
    }
    // Mono routes for every channel.
    for ch in 0..channel_count {
        out.push(AudioRouteOption::mono(prefix, ch));
    }
    // Consecutive stereo pairs: (0,1), (2,3), …
    let mut left = 0;
    while left + 1 < channel_count {
        out.push(AudioRouteOption::pair(prefix, left, left + 1));
        left += 2;
    }
    // "All" only adds value beyond a single stereo pair.
    if channel_count > 2 {
        out.push(AudioRouteOption::all(prefix, channel_count));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_channels_yields_no_options() {
        assert!(build_input_channel_options(0).is_empty());
        assert!(build_output_channel_options(0).is_empty());
    }

    #[test]
    fn one_channel_is_mono_only() {
        let opts = build_input_channel_options(1);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].label, "Input 1");
        assert_eq!(opts[0].channels, vec![0]);
        assert_eq!(opts[0].id, "ch:0");
    }

    #[test]
    fn two_channels_give_two_mono_plus_one_stereo() {
        let opts = build_input_channel_options(2);
        let labels: Vec<&str> = opts.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Input 1", "Input 2", "Input 1+2 (Stereo)"]);
        assert_eq!(opts[2].channels, vec![0, 1]);
        // No redundant "All" for a single stereo pair.
        assert!(opts.iter().all(|o| o.id != "all"));
    }

    #[test]
    fn four_channels_give_mono_pairs_and_all() {
        let opts = build_input_channel_options(4);
        let labels: Vec<&str> = opts.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Input 1",
                "Input 2",
                "Input 3",
                "Input 4",
                "Input 1+2 (Stereo)",
                "Input 3+4 (Stereo)",
                "All Inputs",
            ]
        );
        let all = opts.iter().find(|o| o.id == "all").unwrap();
        assert_eq!(all.channels, vec![0, 1, 2, 3]);
    }

    #[test]
    fn output_prefix_is_applied() {
        let opts = build_output_channel_options(2);
        assert_eq!(opts[0].label, "Output 1");
        assert_eq!(opts[2].label, "Output 1+2 (Stereo)");
    }

    #[test]
    fn odd_channel_count_does_not_pair_the_last() {
        let opts = build_input_channel_options(3);
        let pairs: Vec<&str> = opts
            .iter()
            .filter(|o| o.channels.len() == 2)
            .map(|o| o.label.as_str())
            .collect();
        // Only (0,1) pairs; channel 2 stays mono-only.
        assert_eq!(pairs, ["Input 1+2 (Stereo)"]);
    }
}
