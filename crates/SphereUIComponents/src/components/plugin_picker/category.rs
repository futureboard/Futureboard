//! DAW-friendly category normalization for the plugin picker.

use sphere_plugin_host::{PluginKind, RegistryPlugin};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizedCategory {
    Eq,
    Compressor,
    Dynamics,
    Reverb,
    Delay,
    Distortion,
    Modulation,
    Utility,
    Analyzer,
    Instrument,
    Synth,
    Sampler,
    Drum,
    MidiFx,
    Other,
}

impl NormalizedCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Eq => "EQ",
            Self::Compressor => "Compressor",
            Self::Dynamics => "Dynamics",
            Self::Reverb => "Reverb",
            Self::Delay => "Delay",
            Self::Distortion => "Distortion",
            Self::Modulation => "Modulation",
            Self::Utility => "Utility",
            Self::Analyzer => "Analyzer",
            Self::Instrument => "Instrument",
            Self::Synth => "Synth",
            Self::Sampler => "Sampler",
            Self::Drum => "Drum",
            Self::MidiFx => "MIDI FX",
            Self::Other => "Other",
        }
    }
}

pub fn normalize_category(plugin: &RegistryPlugin) -> NormalizedCategory {
    if plugin.kind == PluginKind::Instrument {
        let hay = category_haystack(plugin);
        if hay.contains("sampler") {
            return NormalizedCategory::Sampler;
        }
        if hay.contains("drum") {
            return NormalizedCategory::Drum;
        }
        if hay.contains("synth") {
            return NormalizedCategory::Synth;
        }
        return NormalizedCategory::Instrument;
    }

    let hay = category_haystack(plugin);
    if hay.contains("eq") || hay.contains("equalizer") {
        return NormalizedCategory::Eq;
    }
    if hay.contains("compressor") || hay.contains("limiter") || hay.contains("gate") {
        return NormalizedCategory::Compressor;
    }
    if hay.contains("dynamics") {
        return NormalizedCategory::Dynamics;
    }
    if hay.contains("reverb") {
        return NormalizedCategory::Reverb;
    }
    if hay.contains("delay") || hay.contains("echo") {
        return NormalizedCategory::Delay;
    }
    if hay.contains("distortion") || hay.contains("saturation") || hay.contains("overdrive") {
        return NormalizedCategory::Distortion;
    }
    if hay.contains("modulation") || hay.contains("chorus") || hay.contains("phaser") || hay.contains("flanger") {
        return NormalizedCategory::Modulation;
    }
    if hay.contains("analyzer") || hay.contains("meter") || hay.contains("spectrum") {
        return NormalizedCategory::Analyzer;
    }
    if hay.contains("utility") || hay.contains("tool") {
        return NormalizedCategory::Utility;
    }
    if hay.contains("midi") {
        return NormalizedCategory::MidiFx;
    }
    NormalizedCategory::Other
}

fn category_haystack(plugin: &RegistryPlugin) -> String {
    format!(
        "{} {} {}",
        plugin.display_category(),
        plugin.raw_category.as_deref().unwrap_or(""),
        plugin.sub_categories.as_deref().unwrap_or("")
    )
    .to_ascii_lowercase()
}

pub fn normalized_category_label(plugin: &RegistryPlugin) -> String {
    normalize_category(plugin).label().to_string()
}
