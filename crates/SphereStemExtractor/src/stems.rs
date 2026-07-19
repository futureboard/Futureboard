use serde::{Deserialize, Serialize};

/// One output stem slot produced by MDX-NET (or karaoke) separation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StemKind {
    Vocals,
    Drums,
    Bass,
    Other,
    Instrumental,
}

impl StemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vocals => "vocals",
            Self::Drums => "drums",
            Self::Bass => "bass",
            Self::Other => "other",
            Self::Instrumental => "instrumental",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Vocals => "Vocals",
            Self::Drums => "Drums",
            Self::Bass => "Bass",
            Self::Other => "Other",
            Self::Instrumental => "Instrumental",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "vocals" | "vocal" => Some(Self::Vocals),
            "drums" | "drum" => Some(Self::Drums),
            "bass" => Some(Self::Bass),
            "other" => Some(Self::Other),
            "instrumental" | "inst" => Some(Self::Instrumental),
            _ => None,
        }
    }

    pub fn file_stem_suffix(self) -> &'static str {
        self.as_str()
    }
}

/// Selected output stems for an extract job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StemSet {
    stems: Vec<StemKind>,
}

impl StemSet {
    pub fn new(stems: impl IntoIterator<Item = StemKind>) -> Self {
        let mut stems: Vec<StemKind> = stems.into_iter().collect();
        stems.sort_by_key(|s| s.as_str());
        stems.dedup();
        Self { stems }
    }

    pub fn all_for_model(model: crate::model::StemModel) -> Self {
        Self::new(model.default_stems().iter().copied())
    }

    pub fn contains(&self, stem: StemKind) -> bool {
        self.stems.contains(&stem)
    }

    pub fn iter(&self) -> impl Iterator<Item = StemKind> + '_ {
        self.stems.iter().copied()
    }

    pub fn as_slice(&self) -> &[StemKind] {
        &self.stems
    }

    pub fn is_empty(&self) -> bool {
        self.stems.is_empty()
    }

    pub fn len(&self) -> usize {
        self.stems.len()
    }

    pub fn toggle(&mut self, stem: StemKind, enabled: bool) {
        if enabled {
            if !self.contains(stem) {
                self.stems.push(stem);
                self.stems.sort_by_key(|s| s.as_str());
            }
        } else {
            self.stems.retain(|s| *s != stem);
        }
    }
}

impl Default for StemSet {
    fn default() -> Self {
        Self::all_for_model(crate::model::StemModel::MdxNet)
    }
}
