use serde::{Deserialize, Serialize};

use crate::stems::StemKind;

/// Offline source-separation model identifiers exposed in the Stem Extractor UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StemModel {
    /// Classic 4-stem MDX-NET pack (Kuielab A: vocals / drums / bass / other).
    #[default]
    MdxNet,
    /// MDX-NET karaoke variant (vocals + instrumental).
    MdxNetKaraoke,
    /// UVR MDX-NET Main (vocals + instrumental).
    MdxNetMain,
    /// Fine-tuned vocal MDX-NET (vocals + instrumental residual).
    MdxNetVocFt,
    /// High-quality instrumental MDX-NET (instrumental + vocals residual).
    MdxNetInstHq,
    /// Kim Vocal 2 (vocals + instrumental residual).
    KimVocal,
    /// Kim Instrumental (instrumental + vocals residual).
    KimInst,
}

impl StemModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MdxNet => "mdx-net",
            Self::MdxNetKaraoke => "mdx-net-karaoke",
            Self::MdxNetMain => "mdx-net-main",
            Self::MdxNetVocFt => "mdx-net-voc-ft",
            Self::MdxNetInstHq => "mdx-net-inst-hq",
            Self::KimVocal => "kim-vocal",
            Self::KimInst => "kim-inst",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MdxNet => "MDX-NET",
            Self::MdxNetKaraoke => "MDX-NET Karaoke",
            Self::MdxNetMain => "MDX-NET Main",
            Self::MdxNetVocFt => "MDX-NET Voc FT",
            Self::MdxNetInstHq => "MDX-NET Inst HQ",
            Self::KimVocal => "Kim Vocal 2",
            Self::KimInst => "Kim Instrumental",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::MdxNet => "4-stem Kuielab A: vocals, drums, bass, other",
            Self::MdxNetKaraoke => "2-stem karaoke: vocals and instrumental",
            Self::MdxNetMain => "2-stem UVR Main: vocals and instrumental",
            Self::MdxNetVocFt => "2-stem vocal fine-tune: vocals and instrumental",
            Self::MdxNetInstHq => "2-stem instrumental HQ: instrumental and vocals",
            Self::KimVocal => "2-stem Kim Vocal 2: vocals and instrumental",
            Self::KimInst => "2-stem Kim Instrumental: instrumental and vocals",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mdx-net" | "mdxnet" | "MDX-NET" => Some(Self::MdxNet),
            "mdx-net-karaoke" | "mdxnet-karaoke" | "MDX-NET Karaoke" => Some(Self::MdxNetKaraoke),
            "mdx-net-main" | "mdxnet-main" | "MDX-NET Main" => Some(Self::MdxNetMain),
            "mdx-net-voc-ft" | "mdxnet-voc-ft" | "MDX-NET Voc FT" => Some(Self::MdxNetVocFt),
            "mdx-net-inst-hq" | "mdxnet-inst-hq" | "MDX-NET Inst HQ" => Some(Self::MdxNetInstHq),
            "kim-vocal" | "kim-vocal-2" | "Kim Vocal 2" => Some(Self::KimVocal),
            "kim-inst" | "kim-instrumental" | "Kim Instrumental" => Some(Self::KimInst),
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
            Self::MdxNetKaraoke
            | Self::MdxNetMain
            | Self::MdxNetVocFt
            | Self::MdxNetInstHq
            | Self::KimVocal
            | Self::KimInst => &[StemKind::Vocals, StemKind::Instrumental],
        }
    }

    pub fn supports_stem(self, stem: StemKind) -> bool {
        self.default_stems().contains(&stem)
    }

    /// Downloadable ONNX weight package for this model (UVR public models).
    pub fn package(self) -> StemModelPackage {
        match self {
            Self::MdxNet => StemModelPackage {
                model: self,
                files: &[
                    StemModelFile {
                        file_name: "kuielab_a_vocals.onnx",
                    },
                    StemModelFile {
                        file_name: "kuielab_a_drums.onnx",
                    },
                    StemModelFile {
                        file_name: "kuielab_a_bass.onnx",
                    },
                    StemModelFile {
                        file_name: "kuielab_a_other.onnx",
                    },
                ],
                approx_bytes: 116_000_000,
                source_label: "UVR / Kuielab A",
            },
            Self::MdxNetKaraoke => StemModelPackage {
                model: self,
                files: &[StemModelFile {
                    file_name: "UVR_MDXNET_KARA.onnx",
                }],
                approx_bytes: 29_700_000,
                source_label: "UVR MDX-NET Karaoke",
            },
            Self::MdxNetMain => StemModelPackage {
                model: self,
                files: &[StemModelFile {
                    file_name: "UVR_MDXNET_Main.onnx",
                }],
                approx_bytes: 66_800_000,
                source_label: "UVR MDX-NET Main",
            },
            Self::MdxNetVocFt => StemModelPackage {
                model: self,
                files: &[StemModelFile {
                    file_name: "UVR-MDX-NET-Voc_FT.onnx",
                }],
                approx_bytes: 66_800_000,
                source_label: "UVR MDX-NET Voc FT",
            },
            Self::MdxNetInstHq => StemModelPackage {
                model: self,
                files: &[StemModelFile {
                    file_name: "UVR-MDX-NET-Inst_HQ_3.onnx",
                }],
                approx_bytes: 66_800_000,
                source_label: "UVR MDX-NET Inst HQ 3",
            },
            Self::KimVocal => StemModelPackage {
                model: self,
                files: &[StemModelFile {
                    file_name: "Kim_Vocal_2.onnx",
                }],
                approx_bytes: 66_800_000,
                source_label: "Kim Vocal 2",
            },
            Self::KimInst => StemModelPackage {
                model: self,
                files: &[StemModelFile {
                    file_name: "Kim_Inst.onnx",
                }],
                approx_bytes: 66_800_000,
                source_label: "Kim Instrumental",
            },
        }
    }
}

/// One ONNX weight file belonging to a [`StemModelPackage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StemModelFile {
    pub file_name: &'static str,
}

/// Downloadable model package metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StemModelPackage {
    pub model: StemModel,
    pub files: &'static [StemModelFile],
    /// Approximate total download size in bytes (UI hint only).
    pub approx_bytes: u64,
    pub source_label: &'static str,
}

impl StemModelPackage {
    pub fn file_count(self) -> usize {
        self.files.len()
    }

    pub fn approx_size_label(self) -> String {
        let mb = (self.approx_bytes as f64) / (1024.0 * 1024.0);
        if mb >= 100.0 {
            format!("~{:.0} MB", mb)
        } else {
            format!("~{:.0} MB", mb)
        }
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
        description: "4-stem Kuielab A: vocals, drums, bass, other",
    },
    StemModelInfo {
        model: StemModel::MdxNetKaraoke,
        id: "mdx-net-karaoke",
        label: "MDX-NET Karaoke",
        description: "2-stem karaoke: vocals and instrumental",
    },
    StemModelInfo {
        model: StemModel::MdxNetMain,
        id: "mdx-net-main",
        label: "MDX-NET Main",
        description: "2-stem UVR Main: vocals and instrumental",
    },
    StemModelInfo {
        model: StemModel::MdxNetVocFt,
        id: "mdx-net-voc-ft",
        label: "MDX-NET Voc FT",
        description: "2-stem vocal fine-tune: vocals and instrumental",
    },
    StemModelInfo {
        model: StemModel::MdxNetInstHq,
        id: "mdx-net-inst-hq",
        label: "MDX-NET Inst HQ",
        description: "2-stem instrumental HQ: instrumental and vocals",
    },
    StemModelInfo {
        model: StemModel::KimVocal,
        id: "kim-vocal",
        label: "Kim Vocal 2",
        description: "2-stem Kim Vocal 2: vocals and instrumental",
    },
    StemModelInfo {
        model: StemModel::KimInst,
        id: "kim-inst",
        label: "Kim Instrumental",
        description: "2-stem Kim Instrumental: instrumental and vocals",
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
        assert_eq!(STEM_MODELS.len(), 7);
        assert_eq!(StemModel::MdxNet.package().file_count(), 4);
        assert_eq!(StemModel::KimVocal.package().file_count(), 1);
    }
}
