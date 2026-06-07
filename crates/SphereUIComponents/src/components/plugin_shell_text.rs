//! DirectWrite text rendering for the native plugin editor shell chrome.
//!
//! Mirrors GPUI's Windows font path (`gpui_windows::direct_write`): embedded
//! font bytes via `IDWriteInMemoryFontFileLoader`, a custom font collection, and
//! `IDWriteFontFallback` for Thai/Unicode. **Direct2D is never used** — glyphs
//! are rasterized through DirectWrite GDI interop (`IDWriteBitmapRenderTarget`)
//! and composited with `BitBlt`.
//!
//! Font bytes come from the same `packages/shared/fonts` embeds as GPUI
//! (`crate::assets::{INTER_VARIABLE, GOOGLE_SANS_VARIABLE}`).

/// Horizontal alignment for [`draw_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
}

/// Font metrics shared with the native editor shell theme (spec Part 5).
#[derive(Debug, Clone, Copy)]
pub struct PluginShellFontTheme {
    pub family_primary: &'static str,
    pub family_fallback: &'static str,
    pub title_size: f32,
    pub body_size: f32,
    pub weight_title: u32,
    pub weight_body: u32,
}

/// Centralized shell font theme — sourced from [`crate::theme`], not magic
/// strings scattered through the chrome.
pub fn shell_font_theme() -> PluginShellFontTheme {
    PluginShellFontTheme {
        family_primary: crate::theme::FONT_FAMILY,
        family_fallback: crate::theme::THAI_FONT_FAMILY,
        title_size: 13.0,
        body_size: 13.0,
        weight_title: 400,
        weight_body: 400,
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::cell::RefCell;
    use std::sync::Once;

    use super::{PluginShellFontTheme, TextAlign, shell_font_theme};
    use crate::assets::{GOOGLE_SANS_VARIABLE, INTER_VARIABLE};
    use windows::core::{implement, BOOL, HSTRING, PCWSTR};
    use windows_core::Interface;
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateSolidBrush, DeleteObject, FillRect, HDC, SRCCOPY,
    };
    use windows::Win32::Graphics::DirectWrite::{
        DWriteCreateFactory, IDWriteBitmapRenderTarget, IDWriteFactory5, IDWriteFontCollection1,
        IDWriteFontFallback, IDWriteFontSetBuilder1, IDWriteGdiInterop, IDWriteInMemoryFontFileLoader,
        IDWritePixelSnapping_Impl, IDWriteRenderingParams, IDWriteTextFormat1, IDWriteTextLayout,
        IDWriteTextRenderer, IDWriteTextRenderer_Impl,
        DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_WEIGHT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_MEASURING_MODE, DWRITE_UNICODE_RANGE,
    };
    use windows::Win32::System::SystemServices::LOCALE_NAME_MAX_LENGTH;

    struct DWriteFontManager {
        factory: IDWriteFactory5,
        in_memory_loader: IDWriteInMemoryFontFileLoader,
        custom_collection: IDWriteFontCollection1,
        system_collection: IDWriteFontCollection1,
        font_fallback: IDWriteFontFallback,
        gdi: IDWriteGdiInterop,
        rendering_params: IDWriteRenderingParams,
        locale: HSTRING,
        theme: PluginShellFontTheme,
        resolved_primary: String,
        custom_collection_ok: bool,
        font_bytes_loaded: u32,
    }

    impl Drop for DWriteFontManager {
        fn drop(&mut self) {
            unsafe {
                let _ = self
                    .factory
                    .UnregisterFontFileLoader(&self.in_memory_loader);
            }
        }
    }

    thread_local! {
        static STATE: RefCell<Option<DWriteFontManager>> = const { RefCell::new(None) };
    }

    static INIT_LOG: Once = Once::new();

    fn font_debug_enabled() -> bool {
        std::env::var_os("FUTUREBOARD_PLUGIN_SHELL_FONT_DEBUG").is_some()
    }

    unsafe fn load_embedded_fonts(
        factory: &IDWriteFactory5,
        loader: &IDWriteInMemoryFontFileLoader,
        builder: &IDWriteFontSetBuilder1,
        blobs: &[&[u8]],
    ) -> u32 {
        let mut loaded = 0u32;
        for bytes in blobs {
            if bytes.is_empty() {
                continue;
            }
            if loader
                .CreateInMemoryFontFileReference(
                    factory,
                    bytes.as_ptr().cast(),
                    bytes.len() as u32,
                    None,
                )
                .and_then(|file| builder.AddFontFile(&file))
                .is_ok()
            {
                loaded += 1;
            }
        }
        loaded
    }

    unsafe fn build_font_fallback(
        factory: &IDWriteFactory5,
        custom_collection: &IDWriteFontCollection1,
        system_collection: &IDWriteFontCollection1,
        theme: &PluginShellFontTheme,
    ) -> Option<IDWriteFontFallback> {
        let builder = factory.CreateFontFallbackBuilder().ok()?;
        let fallback_name = HSTRING::from(theme.family_fallback);
        for collection in [custom_collection, system_collection] {
            let font_set = collection.GetFontSet().ok()?;
            let fonts = font_set
                .GetMatchingFonts(
                    &fallback_name,
                    DWRITE_FONT_WEIGHT(400),
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )
                .ok()?;
            if fonts.GetFontCount() == 0 {
                continue;
            }
            let face_ref = fonts.GetFontFaceReference(0).ok()?;
            let face = face_ref.CreateFontFace().ok()?;
            let mut count = 0u32;
            face.GetUnicodeRanges(None, &mut count).ok()?;
            if count == 0 {
                continue;
            }
            let mut ranges = vec![DWRITE_UNICODE_RANGE::default(); count as usize];
            face.GetUnicodeRanges(Some(&mut ranges), &mut count).ok()?;
            let _ = builder.AddMapping(
                &ranges,
                &[fallback_name.as_ptr()],
                None,
                None,
                None,
                1.0,
            );
            break;
        }
        if let Ok(system) = factory.GetSystemFontFallback() {
            let _ = builder.AddMappings(&system);
        }
        builder.CreateFontFallback().ok()
    }

    unsafe fn resolve_family_in_collection(
        collection: &IDWriteFontCollection1,
        family: &str,
        weight: u32,
        locale: &HSTRING,
    ) -> Option<String> {
        let family_h = HSTRING::from(family);
        let font_set = collection.GetFontSet().ok()?;
        let fonts = font_set
            .GetMatchingFonts(
                &family_h,
                DWRITE_FONT_WEIGHT(weight as i32),
                DWRITE_FONT_STRETCH_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
            )
            .ok()?;
        if fonts.GetFontCount() == 0 {
            return None;
        }
        let face_ref = fonts.GetFontFaceReference(0).ok()?;
        let face = face_ref.CreateFontFace().ok()?;
        let names = face.GetFamilyNames().ok()?;
        let mut index = 0u32;
        let mut exists = BOOL(0);
        names
            .FindLocaleName(locale, &mut index, &mut exists)
            .ok()?;
        if !exists.as_bool() {
            names
                .FindLocaleName(PCWSTR::null(), &mut index, &mut exists)
                .ok()?;
        }
        if !exists.as_bool() {
            return Some(family.to_string());
        }
        let len = names.GetStringLength(index).ok()? as usize;
        let mut buf = vec![0u16; len + 1];
        names.GetString(index, &mut buf).ok()?;
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    unsafe fn make_font_manager() -> Option<DWriteFontManager> {
        let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let in_memory_loader = factory.CreateInMemoryFontFileLoader().ok()?;
        factory.RegisterFontFileLoader(&in_memory_loader).ok()?;
        let builder = factory.CreateFontSetBuilder().ok()?;

        let theme = shell_font_theme();
        let font_bytes_loaded = load_embedded_fonts(
            &factory,
            &in_memory_loader,
            &builder,
            &[INTER_VARIABLE, GOOGLE_SANS_VARIABLE],
        );

        let custom_font_set = builder.CreateFontSet().ok()?;
        let custom_collection = factory
            .CreateFontCollectionFromFontSet(&custom_font_set)
            .ok()?;
        let custom_collection_ok = custom_collection.GetFontFamilyCount() > 0;

        let mut system_collection = None;
        factory
            .GetSystemFontCollection(false, &mut system_collection, true)
            .ok()?;
        let system_collection = system_collection?;

        let mut locale = [0u16; LOCALE_NAME_MAX_LENGTH as usize];
        GetUserDefaultLocaleName(&mut locale);
        let locale = HSTRING::from_wide(&locale);

        let font_fallback = build_font_fallback(
            &factory,
            &custom_collection,
            &system_collection,
            &theme,
        )
        .or_else(|| factory.GetSystemFontFallback().ok())?;

        let resolved_primary = resolve_family_in_collection(
            &custom_collection,
            theme.family_primary,
            theme.weight_title,
            &locale,
        )
        .or_else(|| {
            resolve_family_in_collection(
                &system_collection,
                theme.family_primary,
                theme.weight_title,
                &locale,
            )
        })
        .unwrap_or_else(|| "Segoe UI".to_string());

        let gdi = factory.GetGdiInterop().ok()?;
        let rendering_params = factory.CreateRenderingParams().ok()?;

        INIT_LOG.call_once(|| {
            eprintln!("[plugin-shell-font] source=shared_embedded");
            eprintln!("[plugin-shell-font] primary={}", theme.family_primary);
            eprintln!("[plugin-shell-font] fallback={}", theme.family_fallback);
            eprintln!("[plugin-shell-font] dwrite_factory_version=5");
            eprintln!(
                "[plugin-shell-font] custom_collection_created={custom_collection_ok}"
            );
            eprintln!("[plugin-shell-font] font_bytes_loaded count={font_bytes_loaded}");
            eprintln!("[plugin-shell-font] family_resolved={resolved_primary}");
            eprintln!("[plugin-shell-font] loaded=true");
        });

        Some(DWriteFontManager {
            factory,
            in_memory_loader,
            custom_collection,
            system_collection,
            font_fallback,
            gdi,
            rendering_params,
            locale,
            theme,
            resolved_primary,
            custom_collection_ok,
            font_bytes_loaded,
        })
    }

    struct DrawContext {
        brt: IDWriteBitmapRenderTarget,
        params: IDWriteRenderingParams,
        fg: COLORREF,
        pixels_per_dip: f32,
    }

    #[implement(IDWriteTextRenderer)]
    struct ShellTextRenderer;

    #[allow(non_snake_case)]
    impl IDWritePixelSnapping_Impl for ShellTextRenderer_Impl {
        fn IsPixelSnappingDisabled(
            &self,
            _clientdrawingcontext: *const core::ffi::c_void,
        ) -> windows::core::Result<BOOL> {
            Ok(BOOL(0))
        }

        fn GetCurrentTransform(
            &self,
            _clientdrawingcontext: *const core::ffi::c_void,
            transform: *mut windows::Win32::Graphics::DirectWrite::DWRITE_MATRIX,
        ) -> windows::core::Result<()> {
            unsafe {
                *transform = windows::Win32::Graphics::DirectWrite::DWRITE_MATRIX {
                    m11: 1.0,
                    m12: 0.0,
                    m21: 0.0,
                    m22: 1.0,
                    dx: 0.0,
                    dy: 0.0,
                };
            }
            Ok(())
        }

        fn GetPixelsPerDip(
            &self,
            clientdrawingcontext: *const core::ffi::c_void,
        ) -> windows::core::Result<f32> {
            let ctx = unsafe { &*(clientdrawingcontext.cast::<DrawContext>()) };
            Ok(ctx.pixels_per_dip)
        }
    }

    #[allow(non_snake_case)]
    impl IDWriteTextRenderer_Impl for ShellTextRenderer_Impl {
        fn DrawGlyphRun(
            &self,
            clientdrawingcontext: *const core::ffi::c_void,
            baselineoriginx: f32,
            baselineoriginy: f32,
            measuringmode: DWRITE_MEASURING_MODE,
            glyphrun: *const windows::Win32::Graphics::DirectWrite::DWRITE_GLYPH_RUN,
            _glyphrundescription: *const windows::Win32::Graphics::DirectWrite::DWRITE_GLYPH_RUN_DESCRIPTION,
            _clientdrawingeffect: windows::core::Ref<windows::core::IUnknown>,
        ) -> windows::core::Result<()> {
            let ctx = unsafe { &*(clientdrawingcontext.cast::<DrawContext>()) };
            let glyphrun = unsafe { &*glyphrun };
            unsafe {
                ctx.brt.DrawGlyphRun(
                    baselineoriginx,
                    baselineoriginy,
                    measuringmode,
                    glyphrun,
                    &ctx.params,
                    ctx.fg,
                    None,
                )?;
            }
            Ok(())
        }

        fn DrawUnderline(
            &self,
            _clientdrawingcontext: *const core::ffi::c_void,
            _baselineoriginx: f32,
            _baselineoriginy: f32,
            _underline: *const windows::Win32::Graphics::DirectWrite::DWRITE_UNDERLINE,
            _clientdrawingeffect: windows::core::Ref<windows::core::IUnknown>,
        ) -> windows::core::Result<()> {
            Ok(())
        }

        fn DrawStrikethrough(
            &self,
            _clientdrawingcontext: *const core::ffi::c_void,
            _baselineoriginx: f32,
            _baselineoriginy: f32,
            _strikethrough: *const windows::Win32::Graphics::DirectWrite::DWRITE_STRIKETHROUGH,
            _clientdrawingeffect: windows::core::Ref<windows::core::IUnknown>,
        ) -> windows::core::Result<()> {
            Ok(())
        }

        fn DrawInlineObject(
            &self,
            _clientdrawingcontext: *const core::ffi::c_void,
            _originx: f32,
            _originy: f32,
            _inlineobject: windows::core::Ref<
                windows::Win32::Graphics::DirectWrite::IDWriteInlineObject,
            >,
            _issideways: BOOL,
            _isrighttoleft: BOOL,
            _clientdrawingeffect: windows::core::Ref<windows::core::IUnknown>,
        ) -> windows::core::Result<()> {
            Ok(())
        }
    }

    unsafe fn pick_collection(
        manager: &DWriteFontManager,
        family: &str,
        weight: u32,
    ) -> (IDWriteFontCollection1, String, bool) {
        if let Some(name) =
            resolve_family_in_collection(&manager.custom_collection, family, weight, &manager.locale)
        {
            return (manager.custom_collection.clone(), name, false);
        }
        if let Some(name) = resolve_family_in_collection(
            &manager.system_collection,
            family,
            weight,
            &manager.locale,
        ) {
            eprintln!("[plugin-shell-font] fallback_used=true family={family}");
            return (manager.system_collection.clone(), name, true);
        }
        (
            manager.system_collection.clone(),
            manager.resolved_primary.clone(),
            true,
        )
    }

    unsafe fn create_text_layout(
        manager: &DWriteFontManager,
        text: &str,
        family: &str,
        weight: u32,
        em_px: f32,
        max_w: f32,
        max_h: f32,
        align: TextAlign,
    ) -> Option<IDWriteTextLayout> {
        let (collection, resolved_name, _) = pick_collection(manager, family, weight);
        let family_h = HSTRING::from(resolved_name.as_str());
        let format: IDWriteTextFormat1 = manager
            .factory
            .CreateTextFormat(
                &family_h,
                &collection,
                DWRITE_FONT_WEIGHT(weight as i32),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                em_px,
                &manager.locale,
            )
            .ok()?
            .cast()
            .ok()?;
        let _ = format.SetFontFallback(Some(&manager.font_fallback));
        let (text_align, para_align) = match align {
            TextAlign::Left => (DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_NEAR),
            TextAlign::Center => (
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            ),
        };
        let _ = format.SetTextAlignment(text_align);
        let _ = format.SetParagraphAlignment(para_align);

        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = manager
            .factory
            .CreateTextLayout(&wide, &format, max_w, max_h)
            .ok()?;
        Some(layout)
    }

    /// Render `text` into `rect` on `hdc` with DirectWrite. Returns `false` on
    /// any failure so the caller can fall back to GDI `DrawTextW`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(
        hdc: HDC,
        rect: RECT,
        text: &str,
        family: &str,
        weight: u32,
        em_px: f32,
        bg: COLORREF,
        fg: COLORREF,
        align: TextAlign,
        dpi_scale: f32,
    ) -> bool {
        let w = (rect.right - rect.left).max(0);
        let h = (rect.bottom - rect.top).max(0);
        if w == 0 || h == 0 || text.is_empty() {
            return false;
        }

        STATE.with(|cell| {
            let mut borrow = cell.borrow_mut();
            if borrow.is_none() {
                *borrow = unsafe { make_font_manager() };
            }
            let Some(manager) = borrow.as_ref() else {
                return false;
            };

            let layout = unsafe {
                create_text_layout(
                    manager,
                    text,
                    family,
                    weight,
                    em_px,
                    w as f32,
                    h as f32,
                    align,
                )
            };
            let Some(layout) = layout else {
                eprintln!("[plugin-shell-font] text_layout failed title=\"{text}\"");
                return false;
            };

            let mut metrics = windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS::default();
            if unsafe { layout.GetMetrics(&mut metrics) }.is_err() {
                return false;
            }

            if font_debug_enabled() {
                eprintln!(
                    "[plugin-shell-text] renderer=DirectWriteCustomRenderer d2d=false dpi={}",
                    (dpi_scale * 96.0).round() as u32
                );
                eprintln!(
                    "[plugin-shell-font] text_layout title=\"{text}\" width={:.1} height={:.1}",
                    metrics.width, metrics.height
                );
            }

            unsafe { draw_layout(manager, hdc, rect, w, h, &layout, bg, fg, dpi_scale) }
        })
    }

    unsafe fn draw_layout(
        manager: &DWriteFontManager,
        dst: HDC,
        rect: RECT,
        w: i32,
        h: i32,
        layout: &IDWriteTextLayout,
        bg: COLORREF,
        fg: COLORREF,
        dpi_scale: f32,
    ) -> bool {
        let Ok(brt) = manager.gdi.CreateBitmapRenderTarget(None, w as u32, h as u32) else {
            return false;
        };
        let mem = brt.GetMemoryDC();
        let brush = CreateSolidBrush(bg);
        let full = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        FillRect(mem, &full, brush);
        let _ = DeleteObject(brush.into());

        let mut metrics = windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS::default();
        if layout.GetMetrics(&mut metrics).is_err() {
            return false;
        }
        let origin_x = match metrics.left {
            x if x.is_finite() => x.max(0.0),
            _ => 0.0,
        };
        let origin_y = ((h as f32 - metrics.height) / 2.0).max(0.0);

        let ctx = DrawContext {
            brt: brt.clone(),
            params: manager.rendering_params.clone(),
            fg,
            pixels_per_dip: dpi_scale.max(1.0),
        };
        let renderer: IDWriteTextRenderer = ShellTextRenderer.into();
        if layout
            .Draw(
                Some((&raw const ctx).cast::<core::ffi::c_void>()),
                &renderer,
                origin_x,
                origin_y,
            )
            .is_err()
        {
            return false;
        }

        BitBlt(dst, rect.left, rect.top, w, h, Some(mem), 0, 0, SRCCOPY).is_ok()
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::TextAlign;

    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(
        _hdc: isize,
        _rect: (),
        _text: &str,
        _family: &str,
        _weight: u32,
        _em_px: f32,
        _bg: (),
        _fg: (),
        _align: TextAlign,
        _dpi_scale: f32,
    ) -> bool {
        false
    }
}

#[cfg(target_os = "windows")]
pub use imp::draw_text;
