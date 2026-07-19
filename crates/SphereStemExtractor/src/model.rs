use serde::{Deserialize, Serialize};

use crate::stems::StemKind;

/// Offline source-separation model identifiers exposed in the Stem Extractor UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StemModel {
    /// Classic MDX-NET 4-stem separator (vocals / drums / bass / other).
    #[default]
    MdxNet,
    /// MDX-NET karaoke variant (vocals + instrumental).
    MdxNetKaraoke,
}

impl StemModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MdxNet => "mdx-net",
            Self::MdxNetKaraoke => "mdx-net-karaoke",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MdxNet => "MDX-NET",
            Self::MdxNetKaraoke => "MDX-NET Karaoke",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::MdxNet => "4-stem separation: vocals, drums, bass, other",
            Self::MdxNetKaraoke => "2-stem separation: vocals and instrumental",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mdx-net" | "mdxnet" | "MDX-NET" => Some(Self::MdxNet),
            "mdx-net-karaoke" | "mdxnet-karaoke" | "MDX-NET Karaoke" => Some(Self::MdxNetKaraoke),
            _ => None,
        }
    }

    pub fn default_stems(self) -> &'static [StemKind] {
        match self {
            Self::MdxNet => &[
                StemKind::Vocals,
                StemKind::Drums,
                StemKind::Bass,
                StemKind::Other,
            ],
            Self::MdxNetKaraoke => &[StemKind::Vocals, StemKind::Instrumental],
        }
    }

    pub fn supports_stem(self, stem: StemKind) -> bool {
        self.default_stems().contains(&stem)
    }
}

/// Static catalog entry for the Stem Extractor model dropdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StemModelInfo {
    pub model: StemModel,
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

/// Ordered model list shown in the Stem Extractor dialog.
pub const STEM_MODELS: &[StemModelInfo] = &[
    StemModelInfo {
        model: StemModel::MdxNet,
        id: "mdx-net",
        label: "MDX-NET",
        description: "4-stem separation: vocals, drums, bass, other",
    },
    StemModelInfo {
        model: StemModel::MdxNetKaraoke,
        id: "mdx-net-karaoke",
        label: "MDX-NET Karaoke",
        description: "2-stem separation: vocals and instrumental",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mdx_net_is_default_and_round_trips() {
        assert_eq!(StemModel::default(), StemModel::MdxNet);
        assert_eq!(StemModel::parse("mdx-net"), Some(StemModel::MdxNet));
        assert_eq!(StemModel::MdxNet.as_str(), "mdx-net");
        assert!(StemModel::MdxNet.supports_stem(StemKind::Vocals));
        assert!(!StemModel::MdxNet.supports_stem(StemKind::Instrumental));
        assert!(StemModel::MdxNetKaraoke.supports_stem(StemKind::Instrumental));
    }
}
