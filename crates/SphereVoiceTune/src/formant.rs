/// Configuration for formant preservation and shifting.
#[derive(Debug, Clone, PartialEq)]
pub struct FormantConfig {
    pub shift_semitones: f64,
    pub preserve_formants: bool,
}

impl Default for FormantConfig {
    fn default() -> Self {
        Self {
            shift_semitones: 0.0,
            preserve_formants: true,
        }
    }
}
