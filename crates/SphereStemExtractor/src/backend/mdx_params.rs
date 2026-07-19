//! MDX-NET per-model STFT hyperparameters.
//!
//! MDX-NET ONNX weights do **not** embed the STFT framing they were trained
//! with (`n_fft`, `dim_f`, `dim_t`, loudness compensation). UVR ships these in a
//! side `model_data.json` keyed by file hash. We keep a compact built-in table
//! keyed by the exact file names we offer for download (see
//! [`crate::model::StemModel::package`]). `dim_f` / `dim_t` are additionally
//! validated against the model's declared input shape at load time, so a table
//! miss degrades gracefully rather than producing garbage.

use crate::stems::StemKind;

/// STFT + framing parameters for one MDX-NET ONNX file.
#[derive(Clone, Copy, Debug)]
pub struct MdxParams {
    /// STFT size. MDX uses 6144 for most packs, 7680 for the "Main"/"Voc FT".
    pub n_fft: usize,
    /// Number of low frequency bins fed to the model (`<= n_fft/2 + 1`).
    pub dim_f: usize,
    /// Time frames per inference chunk (a power of two, e.g. 256).
    pub dim_t: usize,
    /// Output loudness compensation multiplier.
    pub compensate: f32,
    /// The stem this single-target model actually predicts. The complementary
    /// stem is derived as `mixture - primary`.
    pub primary: StemKind,
}

impl MdxParams {
    /// STFT hop is fixed at 1024 for the MDX-NET family.
    pub const HOP: usize = 1024;

    /// Full-resolution bin count for this `n_fft`.
    pub fn n_bins(&self) -> usize {
        self.n_fft / 2 + 1
    }

    /// Samples consumed per inference chunk (`hop * (dim_t - 1)`).
    pub fn chunk_size(&self) -> usize {
        Self::HOP * (self.dim_t.saturating_sub(1))
    }

    /// Context trimmed from each chunk edge (`n_fft / 2`).
    pub fn trim(&self) -> usize {
        self.n_fft / 2
    }

    /// Fresh samples produced per chunk (`chunk_size - 2 * trim`).
    pub fn gen_size(&self) -> usize {
        self.chunk_size().saturating_sub(2 * self.trim())
    }

    /// MDX-NET is trained at 44.1 kHz; input is resampled to this rate.
    pub const SAMPLE_RATE: u32 = 44_100;
}

/// A conservative default used when a file name is not in the table. 6144/2048
/// is the most common MDX-NET geometry; `dim_f`/`dim_t` are corrected from the
/// model's own input shape when it is static.
const DEFAULT: MdxParams = MdxParams {
    n_fft: 6144,
    dim_f: 2048,
    dim_t: 256,
    compensate: 1.0,
    primary: StemKind::Vocals,
};

/// Look up known UVR MDX-NET parameters by ONNX file name.
///
/// Returns [`DEFAULT`] for unknown files; callers should still override
/// `dim_f`/`dim_t` from the loaded model's static input dimensions.
pub fn params_for_file(file_name: &str) -> MdxParams {
    let p = |n_fft, dim_f, dim_t, compensate, primary| MdxParams {
        n_fft,
        dim_f,
        dim_t,
        compensate,
        primary,
    };
    match file_name {
        // Kuielab A 4-stem pack — one single-target model per stem.
        "kuielab_a_vocals.onnx" => p(6144, 2048, 256, 1.0, StemKind::Vocals),
        "kuielab_a_drums.onnx" => p(6144, 2048, 256, 1.0, StemKind::Drums),
        "kuielab_a_bass.onnx" => p(6144, 2048, 256, 1.0, StemKind::Bass),
        "kuielab_a_other.onnx" => p(6144, 2048, 256, 1.0, StemKind::Other),
        // 2-stem UVR MDX-NET models (primary + derived complement).
        "UVR_MDXNET_KARA.onnx" => p(6144, 2048, 256, 1.065, StemKind::Vocals),
        "UVR_MDXNET_Main.onnx" => p(7680, 3072, 256, 1.008, StemKind::Vocals),
        "UVR-MDX-NET-Voc_FT.onnx" => p(7680, 3072, 256, 1.021, StemKind::Vocals),
        "UVR-MDX-NET-Inst_HQ_3.onnx" => p(6144, 3072, 256, 1.022, StemKind::Instrumental),
        "Kim_Vocal_2.onnx" => p(6144, 3072, 256, 1.043, StemKind::Vocals),
        "Kim_Inst.onnx" => p(6144, 3072, 256, 1.017, StemKind::Instrumental),
        _ => DEFAULT,
    }
}
