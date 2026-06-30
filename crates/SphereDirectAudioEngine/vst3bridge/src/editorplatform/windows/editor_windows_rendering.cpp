#include "editor_windows_internal.hpp"

#include <algorithm>
#include <cstdio>
#include <mutex>

namespace daux_editor_windows {

class GdiDWriteRenderer final : public IDWriteTextRenderer {
public:
  GdiDWriteRenderer(IDWriteBitmapRenderTarget *target,
                    IDWriteRenderingParams *params, COLORREF color, FLOAT ppd)
      : target_(target), params_(params), color_(color), ppd_(ppd) {}
  HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void **obj) override {
    if (!obj)
      return E_POINTER;
    if (iid == __uuidof(IUnknown) || iid == __uuidof(IDWritePixelSnapping) ||
        iid == __uuidof(IDWriteTextRenderer)) {
      *obj = static_cast<IDWriteTextRenderer *>(this);
      AddRef();
      return S_OK;
    }
    *obj = nullptr;
    return E_NOINTERFACE;
  }
  ULONG STDMETHODCALLTYPE AddRef() override { return 2; }
  ULONG STDMETHODCALLTYPE Release() override { return 1; }
  HRESULT STDMETHODCALLTYPE IsPixelSnappingDisabled(void *,
                                                    BOOL *disabled) noexcept override {
    if (!disabled)
      return E_POINTER;
    *disabled = FALSE;
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE
  GetCurrentTransform(void *, DWRITE_MATRIX *transform) noexcept override {
    if (!transform)
      return E_POINTER;
    *transform = DWRITE_MATRIX{1, 0, 0, 1, 0, 0};
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE GetPixelsPerDip(void *, FLOAT *ppd) noexcept override {
    if (!ppd)
      return E_POINTER;
    *ppd = ppd_;
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE DrawGlyphRun(void *, FLOAT x, FLOAT y,
                                         DWRITE_MEASURING_MODE mode,
                                         const DWRITE_GLYPH_RUN *run,
                                         const DWRITE_GLYPH_RUN_DESCRIPTION *,
                                         IUnknown *) noexcept override {
    return target_->DrawGlyphRun(x, y, mode, run, params_, color_, nullptr);
  }
  HRESULT STDMETHODCALLTYPE DrawUnderline(void *, FLOAT, FLOAT,
                                          const DWRITE_UNDERLINE *,
                                          IUnknown *) noexcept override {
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE DrawStrikethrough(void *, FLOAT, FLOAT,
                                              const DWRITE_STRIKETHROUGH *,
                                              IUnknown *) noexcept override {
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE DrawInlineObject(void *, FLOAT, FLOAT,
                                             IDWriteInlineObject *, BOOL, BOOL,
                                             IUnknown *) noexcept override {
    return S_OK;
  }

private:
  IDWriteBitmapRenderTarget *target_;
  IDWriteRenderingParams *params_;
  COLORREF color_;
  FLOAT ppd_;
};

template <typename T> void release(T *&p) {
  if (p) {
    p->Release();
    p = nullptr;
  }
}

void dwrite_log_once(const char *status, HRESULT hr) {
  static std::once_flag once;
  std::call_once(once, [status, hr] {
    std::fprintf(
        stderr,
        "[NativeEditorShell] dwrite=%s hr=0x%08lx path=gdi_interop d2d=false\n",
        status, static_cast<unsigned long>(hr));
  });
}

bool draw_dwrite_text(HDC dest, RECT rect, const wchar_t *text, COLORREF bg,
                      COLORREF fg, int sdpi) {
  const int w = std::max<LONG>(1, rect.right - rect.left);
  const int h = std::max<LONG>(1, rect.bottom - rect.top);
  const FLOAT ppd = static_cast<FLOAT>(sdpi > 0 ? sdpi : 96) / 96.0f;
  const FLOAT layout_w_dip = std::max<FLOAT>(1.0f, static_cast<FLOAT>(w) / ppd);
  const FLOAT layout_h_dip = std::max<FLOAT>(1.0f, static_cast<FLOAT>(h) / ppd);
  HRESULT hr = S_OK;
  IDWriteFactory *factory = nullptr;
  IDWriteGdiInterop *gdi = nullptr;
  IDWriteBitmapRenderTarget *target = nullptr;
  IDWriteRenderingParams *params = nullptr;
  IDWriteTextFormat *format = nullptr;
  IDWriteTextLayout *layout = nullptr;
  IDWriteInlineObject *ellipsis = nullptr;
  hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                           reinterpret_cast<IUnknown **>(&factory));
  if (SUCCEEDED(hr))
    hr = factory->GetGdiInterop(&gdi);
  if (SUCCEEDED(hr))
    hr = gdi->CreateBitmapRenderTarget(dest, w, h, &target);
  if (SUCCEEDED(hr))
    hr = factory->CreateRenderingParams(&params);
  if (SUCCEEDED(hr)) {
    hr = factory->CreateTextFormat(
        L"Segoe UI", nullptr, DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 12.0f, L"",
        &format);
  }
  if (SUCCEEDED(hr)) {
    format->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
    format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    const UINT32 len = text ? static_cast<UINT32>(wcsnlen_s(text, 1024)) : 0;
    hr = factory->CreateTextLayout(text ? text : L"", len, format, layout_w_dip,
                                   layout_h_dip, &layout);
  }
  if (SUCCEEDED(hr)) {
    hr = factory->CreateEllipsisTrimmingSign(format, &ellipsis);
    if (SUCCEEDED(hr)) {
      DWRITE_TRIMMING trimming{};
      trimming.granularity = DWRITE_TRIMMING_GRANULARITY_CHARACTER;
      layout->SetTrimming(&trimming, ellipsis);
    }
  }
  if (SUCCEEDED(hr)) {
    HDC mem = target->GetMemoryDC();
    RECT local{0, 0, w, h};
    HBRUSH b = CreateSolidBrush(bg);
    FillRect(mem, &local, b);
    DeleteObject(b);
    target->SetPixelsPerDip(ppd);
    GdiDWriteRenderer renderer(target, params, fg, ppd);
    hr = layout->Draw(nullptr, &renderer, 0.0f, 0.0f);
    if (SUCCEEDED(hr))
      BitBlt(dest, rect.left, rect.top, w, h, mem, 0, 0, SRCCOPY);
  }
  release(ellipsis);
  release(layout);
  release(format);
  release(params);
  release(target);
  release(gdi);
  release(factory);
  dwrite_log_once(SUCCEEDED(hr) ? "ok" : "fail", hr);
  return SUCCEEDED(hr);
}

// Centered single-line text (DirectWrite via GDI interop). Used for the content
// "Loading Plugin" overlay. Mirrors `draw_dwrite_text` but center-aligns the
// run both horizontally and vertically inside `rect`. Returns false so callers
// can fall back to a GDI `DrawTextW`.
bool draw_dwrite_centered(HDC dest, RECT rect, const wchar_t *text,
                          float size_dip, DWRITE_FONT_WEIGHT weight,
                          COLORREF bg, COLORREF fg, int sdpi) {
  const int w = std::max<LONG>(1, rect.right - rect.left);
  const int h = std::max<LONG>(1, rect.bottom - rect.top);
  const FLOAT ppd = static_cast<FLOAT>(sdpi > 0 ? sdpi : 96) / 96.0f;
  const FLOAT layout_w_dip = std::max<FLOAT>(1.0f, static_cast<FLOAT>(w) / ppd);
  const FLOAT layout_h_dip = std::max<FLOAT>(1.0f, static_cast<FLOAT>(h) / ppd);
  HRESULT hr = S_OK;
  IDWriteFactory *factory = nullptr;
  IDWriteGdiInterop *gdi = nullptr;
  IDWriteBitmapRenderTarget *target = nullptr;
  IDWriteRenderingParams *params = nullptr;
  IDWriteTextFormat *format = nullptr;
  IDWriteTextLayout *layout = nullptr;
  IDWriteInlineObject *ellipsis = nullptr;
  hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                           reinterpret_cast<IUnknown **>(&factory));
  if (SUCCEEDED(hr))
    hr = factory->GetGdiInterop(&gdi);
  if (SUCCEEDED(hr))
    hr = gdi->CreateBitmapRenderTarget(dest, w, h, &target);
  if (SUCCEEDED(hr))
    hr = factory->CreateRenderingParams(&params);
  if (SUCCEEDED(hr)) {
    hr = factory->CreateTextFormat(
        L"Segoe UI", nullptr, weight, DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL, size_dip, L"", &format);
  }
  if (SUCCEEDED(hr)) {
    format->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
    format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    const UINT32 len = text ? static_cast<UINT32>(wcsnlen_s(text, 1024)) : 0;
    hr = factory->CreateTextLayout(text ? text : L"", len, format, layout_w_dip,
                                   layout_h_dip, &layout);
  }
  if (SUCCEEDED(hr)) {
    hr = factory->CreateEllipsisTrimmingSign(format, &ellipsis);
    if (SUCCEEDED(hr)) {
      DWRITE_TRIMMING trimming{};
      trimming.granularity = DWRITE_TRIMMING_GRANULARITY_CHARACTER;
      layout->SetTrimming(&trimming, ellipsis);
    }
  }
  if (SUCCEEDED(hr)) {
    HDC mem = target->GetMemoryDC();
    RECT local{0, 0, w, h};
    HBRUSH b = CreateSolidBrush(bg);
    FillRect(mem, &local, b);
    DeleteObject(b);
    target->SetPixelsPerDip(ppd);
    GdiDWriteRenderer renderer(target, params, fg, ppd);
    hr = layout->Draw(nullptr, &renderer, 0.0f, 0.0f);
    if (SUCCEEDED(hr))
      BitBlt(dest, rect.left, rect.top, w, h, mem, 0, 0, SRCCOPY);
  }
  release(ellipsis);
  release(layout);
  release(format);
  release(params);
  release(target);
  release(gdi);
  release(factory);
  return SUCCEEDED(hr);
}

void draw_centered_line(HDC dc, RECT rect, const wchar_t *text, float size_dip,
                        DWRITE_FONT_WEIGHT weight, COLORREF bg, COLORREF fg,
                        int sdpi) {
  if (!text || !*text)
    return;
  if (draw_dwrite_centered(dc, rect, text, size_dip, weight, bg, fg, sdpi))
    return;
  // GDI fallback: same rect, horizontally + vertically centered, ellipsized.
  SetBkMode(dc, TRANSPARENT);
  SetTextColor(dc, fg);
  HFONT font = CreateFontW(
      -MulDiv(static_cast<int>(size_dip), sdpi, 96), 0, 0, 0,
      weight >= DWRITE_FONT_WEIGHT_SEMI_BOLD ? FW_SEMIBOLD : FW_NORMAL, FALSE,
      FALSE, FALSE, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
      CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI");
  HGDIOBJ old_font = SelectObject(dc, font);
  RECT r = rect;
  DrawTextW(dc, text, -1, &r,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS |
                DT_NOPREFIX);
  SelectObject(dc, old_font);
  DeleteObject(font);
}

// Paint the content child's "Loading Plugin" / failure overlay. Drawn only
// before the plug-in's IPlugView attaches (afterwards the plug-in's own child
// HWNDs cover the content). `c` may be null during early window creation.
void paint_content_overlay(HWND content, Context *c) {
  PAINTSTRUCT ps{};
  HDC dc = BeginPaint(content, &ps);
  if (!dc)
    return;
  RECT rc{};
  GetClientRect(content, &rc);
  const int cw = rc.right - rc.left;
  const int ch = rc.bottom - rc.top;
  const int sdpi = static_cast<int>(dpi(content));

  const COLORREF bg = RGB(16, 17, 20);
  const COLORREF label = RGB(150, 152, 158);
  const COLORREF name = RGB(220, 221, 225);
  const COLORREF error = RGB(229, 115, 115);

  // Double-buffer the whole content into a memory bitmap to avoid flicker.
  HDC mem = CreateCompatibleDC(dc);
  HBITMAP bmp = CreateCompatibleBitmap(dc, std::max(1, cw), std::max(1, ch));
  HGDIOBJ old_bmp = SelectObject(mem, bmp);
  RECT full{0, 0, cw, ch};
  HBRUSH bgb = CreateSolidBrush(bg);
  FillRect(mem, &full, bgb);
  DeleteObject(bgb);

  // Only draw the overlay while the editor view has not attached yet (or has
  // failed). Once attached, the plug-in owns the content area.
  const bool show_overlay = !c || c->load_failed || !attached(c);
  if (show_overlay && cw > 0 && ch > 0) {
    const int line1_h = std::max(1, MulDiv(20, sdpi, 96));
    const int gap = std::max(1, MulDiv(6, sdpi, 96));
    const int line2_h = std::max(1, MulDiv(26, sdpi, 96));
    const int pad = std::max(1, MulDiv(16, sdpi, 96));
    const int block_h = line1_h + gap + line2_h;
    const int top = rc.top + std::max(0, (ch - block_h) / 2);
    RECT r1{rc.left + pad, top, rc.right - pad, top + line1_h};
    RECT r2{rc.left + pad, top + line1_h + gap, rc.right - pad,
            top + line1_h + gap + line2_h};

    std::wstring secondary = c && !c->plugin_name.empty()
                                 ? c->plugin_name
                                 : std::wstring(L"Unknown Plugin");
    if (c && c->load_failed) {
      draw_centered_line(mem, r1, L"Plugin Failed to Load", 12.0f,
                         DWRITE_FONT_WEIGHT_SEMI_BOLD, bg, error, sdpi);
      const std::wstring detail =
          !c->error_text.empty() ? c->error_text : secondary;
      draw_centered_line(mem, r2, detail.c_str(), 13.0f,
                         DWRITE_FONT_WEIGHT_NORMAL, bg, label, sdpi);
    } else if (c && !c->error_text.empty()) {
      draw_centered_line(mem, r1, L"Waiting for plugin editor...", 12.0f,
                         DWRITE_FONT_WEIGHT_SEMI_BOLD, bg, label, sdpi);
      const std::wstring detail =
          std::wstring(L"Current stage: ") + c->error_text;
      draw_centered_line(mem, r2, detail.c_str(), 13.0f,
                         DWRITE_FONT_WEIGHT_NORMAL, bg, name, sdpi);
    } else {
      draw_centered_line(mem, r1, L"Loading Plugin", 12.0f,
                         DWRITE_FONT_WEIGHT_NORMAL, bg, label, sdpi);
      draw_centered_line(mem, r2, secondary.c_str(), 15.0f,
                         DWRITE_FONT_WEIGHT_SEMI_BOLD, bg, name, sdpi);
    }
  }

  BitBlt(dc, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
  SelectObject(mem, old_bmp);
  DeleteObject(bmp);
  DeleteDC(mem);
  EndPaint(content, &ps);
}

} // namespace daux_editor_windows
