//! Registry of shared embedded assets.
//!
//! The native DirectWrite path can't read fonts from the running app's GPUI
//! text system, so it loads the **same** `packages/shared/fonts` blobs that the
//! rest of Futureboard embeds. Embedding them here (once, behind a stable API)
//! keeps every native surface on the same typeface without hardcoding file
//! paths at runtime — the paths below are resolved at compile time by
//! `include_bytes!`.

/// Primary variable UI font (Inter).
const INTER_VARIABLE: &[u8] = include_bytes!("../../../../packages/shared/fonts/InterVariable.ttf");

/// Fallback variable font with Thai / extended Unicode coverage (Google Sans).
const GOOGLE_SANS_VARIABLE: &[u8] =
    include_bytes!("../../../../packages/shared/fonts/GoogleSans-VariableFont.ttf");

/// Accessor for the shared embedded assets.
///
/// A zero-sized handle: the assets are `'static` and embedded in the binary, so
/// this is purely a namespacing convenience with no state to construct.
pub struct SharedAssetRegistry;

impl SharedAssetRegistry {
    /// Primary UI font bytes (Inter Variable).
    pub fn inter() -> &'static [u8] {
        INTER_VARIABLE
    }

    /// Fallback font bytes (Google Sans Variable) for Thai / extended Unicode.
    pub fn google_sans() -> &'static [u8] {
        GOOGLE_SANS_VARIABLE
    }

    /// All UI font blobs in priority order (primary first, fallback second).
    ///
    /// Suitable to hand straight to [`crate::DWriteTextRenderer::new`] /
    /// [`crate::DWriteFontManager::new`].
    pub fn ui_font_blobs() -> Vec<&'static [u8]> {
        vec![INTER_VARIABLE, GOOGLE_SANS_VARIABLE]
    }
}
