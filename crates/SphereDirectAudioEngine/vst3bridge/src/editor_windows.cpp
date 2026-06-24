#include "editor_windows.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  define NOMINMAX
#  include <windows.h>
#  include <dwmapi.h>
#  include <dwrite.h>
#endif

#if defined(_WIN32)
namespace {

constexpr const wchar_t* kTopClass = L"FutureboardDauxVst3EditorDetached";
constexpr const wchar_t* kContentClass = L"FutureboardDauxVst3EditorContent";
constexpr UINT_PTR kWakeTimerTop = 0xDA01;
constexpr UINT_PTR kWakeTimerContent = 0xDA02;
constexpr int kTitlebarLogicalH = 32;
constexpr int kTitleButtonLogicalW = 46;
constexpr int kTitleTextLeftLogical = 12;
constexpr int kTitleTextRightGapLogical = 8;

enum Button { kBtnNone = -1, kBtnPin = 0, kBtnMin = 1, kBtnMax = 2, kBtnClose = 3 };

struct TitlebarLayout {
  int dpi = 96;
  int client_w = 0;
  int titlebar_h = 0;
  int button_w = 0;
  RECT title_text_rect{0, 0, 0, 0};
  RECT buttons[4]{};
};

struct Context {
  DauxEditorWindow window{};
  DauxEditorWindowCallbacks cb{};
  int hover{kBtnNone};
  int press{kBtnNone};
  bool tracking{false};
  int logged_layout_dpi{0};
  int logged_layout_w{0};
  // Content "Loading Plugin" overlay, drawn until the plug-in's IPlugView
  // attaches (or, on failure, replaced by an error line). `plugin_name` is the
  // display name; `load_failed`/`error_text` drive the failure state.
  std::wstring plugin_name;
  std::wstring error_text;
  bool load_failed{false};
};

HWND hwnd(void* p) { return reinterpret_cast<HWND>(p); }
void* ptr(HWND h) { return reinterpret_cast<void*>(h); }

unsigned dpi(HWND h) {
  if (!h || !IsWindow(h)) return 96;
  const UINT d = GetDpiForWindow(h);
  return d ? d : 96;
}

int titlebar_h(HWND h) {
  return MulDiv(kTitlebarLogicalH, static_cast<int>(dpi(h)), 96);
}

int button_w(HWND h) {
  return MulDiv(kTitleButtonLogicalW, static_cast<int>(dpi(h)), 96);
}

int daux_dpi_scale(int value, int sdpi) {
  return MulDiv(value, sdpi > 0 ? sdpi : 96, 96);
}

float daux_dpi_scale_f(float value, int sdpi) {
  return value * static_cast<float>(sdpi > 0 ? sdpi : 96) / 96.0f;
}

RECT button_rect_px(int client_w, int bw, int th, int button) {
  const int slot = kBtnClose - button;
  return RECT{client_w - bw * (slot + 1), 0, client_w - bw * slot, th};
}

TitlebarLayout compute_titlebar_layout(int client_w, int sdpi) {
  TitlebarLayout layout{};
  layout.dpi = sdpi > 0 ? sdpi : 96;
  layout.client_w = std::max(0, client_w);
  layout.titlebar_h = daux_dpi_scale(kTitlebarLogicalH, layout.dpi);
  layout.button_w = daux_dpi_scale(kTitleButtonLogicalW, layout.dpi);
  for (int b = kBtnPin; b <= kBtnClose; ++b) {
    layout.buttons[b] = button_rect_px(layout.client_w, layout.button_w, layout.titlebar_h, b);
  }
  const int left = daux_dpi_scale(kTitleTextLeftLogical, layout.dpi);
  const int gap = daux_dpi_scale(kTitleTextRightGapLogical, layout.dpi);
  const int first_button_left = layout.buttons[kBtnPin].left;
  layout.title_text_rect = RECT{
      left,
      0,
      std::max(left, first_button_left - gap),
      layout.titlebar_h,
  };
  return layout;
}

void log_titlebar_layout_if_needed(HWND h, Context* c, const TitlebarLayout& layout) {
  if (!c) return;
  if (c->logged_layout_dpi == layout.dpi && c->logged_layout_w == layout.client_w) return;
  c->logged_layout_dpi = layout.dpi;
  c->logged_layout_w = layout.client_w;
  std::fprintf(stderr, "[NativeEditorShell] dpi=%d\n", layout.dpi);
  std::fprintf(stderr, "[NativeEditorShell] titlebar_h=%d\n", layout.titlebar_h);
  std::fprintf(stderr, "[NativeEditorShell] title_rect=(%ld,%ld,%ld,%ld)\n",
               layout.title_text_rect.left, layout.title_text_rect.top,
               layout.title_text_rect.right, layout.title_text_rect.bottom);
  const char* names[4] = {"pin", "min", "max", "close"};
  for (int b = kBtnPin; b <= kBtnClose; ++b) {
    const RECT& r = layout.buttons[b];
    std::fprintf(stderr, "[NativeEditorShell] button_rect %s=(%ld,%ld,%ld,%ld)\n",
                 names[b], r.left, r.top, r.right, r.bottom);
  }
  std::fprintf(stderr, "[NativeEditorShell] dwrite_font_size=12.00dip %.2fpx\n",
               daux_dpi_scale_f(12.0f, layout.dpi));
  (void)h;
}

bool live(Context* c) {
  return c && (!c->cb.is_live || c->cb.is_live(c->cb.user_data));
}

bool attached(Context* c) {
  return c && c->cb.is_attached && c->cb.is_attached(c->cb.user_data);
}

bool resizing(Context* c) {
  return c && c->cb.is_resize_in_progress && c->cb.is_resize_in_progress(c->cb.user_data);
}

void set_resizing(Context* c, bool v) {
  if (c && c->cb.set_resize_in_progress) c->cb.set_resize_in_progress(c->cb.user_data, v);
}

bool can_resize(Context* c) {
  return c && c->cb.can_resize && c->cb.can_resize(c->cb.user_data);
}

bool constrain(Context* c, int* w, int* h) {
  return c && c->cb.constrain_content_size && c->cb.constrain_content_size(c->cb.user_data, w, h);
}

bool message_debug() {
  static const bool enabled =
      std::getenv("FUTUREBOARD_PLUGIN_VIEW_DEBUG") != nullptr ||
      std::getenv("FUTUREBOARD_VST3_EDITOR_DEBUG") != nullptr ||
      std::getenv("FUTUREBOARD_PLUGIN_DEBUG") != nullptr;
  return enabled;
}

void log_message(const char* tag, HWND h, UINT msg) {
  if (!message_debug()) return;
  const char* name = nullptr;
  switch (msg) {
    case WM_CREATE: name = "WM_CREATE"; break;
    case WM_SHOWWINDOW: name = "WM_SHOWWINDOW"; break;
    case WM_SIZE: name = "WM_SIZE"; break;
    case WM_CLOSE: name = "WM_CLOSE"; break;
    case WM_DESTROY: name = "WM_DESTROY"; break;
    case WM_DPICHANGED: name = "WM_DPICHANGED"; break;
    case WM_PAINT: name = "WM_PAINT"; break;
    case WM_ERASEBKGND: name = "WM_ERASEBKGND"; break;
    case WM_TIMER: name = "WM_TIMER"; break;
    default: break;
  }
  if (name) {
    std::fprintf(stderr, "[%s] %s hwnd=0x%p tid=%lu\n", tag, name, ptr(h), GetCurrentThreadId());
  }
}

void set_dark_titlebar(HWND h) {
  if (!h) return;
  BOOL dark = TRUE;
  HRESULT dark_hr = DwmSetWindowAttribute(h, 20, &dark, sizeof(dark));
  if (FAILED(dark_hr)) dark_hr = DwmSetWindowAttribute(h, 19, &dark, sizeof(dark));
  std::fprintf(stderr, "[NativeEditorShell] dwm dark_mode=%s hr=0x%08lx\n",
               SUCCEEDED(dark_hr) ? "ok" : "fail", static_cast<unsigned long>(dark_hr));

  const bool rounded_disabled = [] {
    const char* v = std::getenv("FUTUREBOARD_EDITOR_ROUNDED");
    return v && (_stricmp(v, "0") == 0 || _stricmp(v, "false") == 0 || _stricmp(v, "off") == 0);
  }();
  if (rounded_disabled) {
    std::fprintf(stderr, "[NativeEditorShell] rounded=disabled\n");
  } else {
    const int preference = 2;
    const HRESULT hr = DwmSetWindowAttribute(h, 33, &preference, sizeof(preference));
    std::fprintf(stderr, "[NativeEditorShell] rounded=%s\n", SUCCEEDED(hr) ? "enabled" : "unsupported");
  }
  COLORREF border = RGB(44, 46, 51);
  const HRESULT border_hr = DwmSetWindowAttribute(h, 34, &border, sizeof(border));
  std::fprintf(stderr, "[NativeEditorShell] dwm border_color=%s\n", SUCCEEDED(border_hr) ? "ok" : "fail");
  const COLORREF caption = RGB(14, 19, 25);
  const HRESULT caption_hr = DwmSetWindowAttribute(h, DWMWA_CAPTION_COLOR, &caption, sizeof(caption));
  std::fprintf(stderr, "[NativeEditorShell] dwm caption_color=%s\n", SUCCEEDED(caption_hr) ? "ok" : "fail");
}

class GdiDWriteRenderer final : public IDWriteTextRenderer {
 public:
  GdiDWriteRenderer(IDWriteBitmapRenderTarget* target, IDWriteRenderingParams* params,
                    COLORREF color, FLOAT ppd)
      : target_(target), params_(params), color_(color), ppd_(ppd) {}
  HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** obj) override {
    if (!obj) return E_POINTER;
    if (iid == __uuidof(IUnknown) || iid == __uuidof(IDWritePixelSnapping) ||
        iid == __uuidof(IDWriteTextRenderer)) {
      *obj = static_cast<IDWriteTextRenderer*>(this);
      AddRef();
      return S_OK;
    }
    *obj = nullptr;
    return E_NOINTERFACE;
  }
  ULONG STDMETHODCALLTYPE AddRef() override { return 2; }
  ULONG STDMETHODCALLTYPE Release() override { return 1; }
  HRESULT STDMETHODCALLTYPE IsPixelSnappingDisabled(void*, BOOL* disabled) override {
    if (!disabled) return E_POINTER;
    *disabled = FALSE;
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE GetCurrentTransform(void*, DWRITE_MATRIX* transform) override {
    if (!transform) return E_POINTER;
    *transform = DWRITE_MATRIX{1, 0, 0, 1, 0, 0};
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE GetPixelsPerDip(void*, FLOAT* ppd) override {
    if (!ppd) return E_POINTER;
    *ppd = ppd_;
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE DrawGlyphRun(void*, FLOAT x, FLOAT y, DWRITE_MEASURING_MODE mode,
                                         const DWRITE_GLYPH_RUN* run,
                                         const DWRITE_GLYPH_RUN_DESCRIPTION*, IUnknown*) override {
    return target_->DrawGlyphRun(x, y, mode, run, params_, color_, nullptr);
  }
  HRESULT STDMETHODCALLTYPE DrawUnderline(void*, FLOAT, FLOAT, const DWRITE_UNDERLINE*, IUnknown*) override { return S_OK; }
  HRESULT STDMETHODCALLTYPE DrawStrikethrough(void*, FLOAT, FLOAT, const DWRITE_STRIKETHROUGH*, IUnknown*) override { return S_OK; }
  HRESULT STDMETHODCALLTYPE DrawInlineObject(void*, FLOAT, FLOAT, IDWriteInlineObject*, BOOL, BOOL, IUnknown*) override { return S_OK; }
 private:
  IDWriteBitmapRenderTarget* target_;
  IDWriteRenderingParams* params_;
  COLORREF color_;
  FLOAT ppd_;
};

template <typename T>
void release(T*& p) {
  if (p) {
    p->Release();
    p = nullptr;
  }
}

void dwrite_log_once(const char* status, HRESULT hr) {
  static std::once_flag once;
  std::call_once(once, [status, hr] {
    std::fprintf(stderr, "[NativeEditorShell] dwrite=%s hr=0x%08lx path=gdi_interop d2d=false\n",
                 status, static_cast<unsigned long>(hr));
  });
}

bool draw_dwrite_text(HDC dest, RECT rect, const wchar_t* text, COLORREF bg, COLORREF fg, int sdpi) {
  const int w = std::max<LONG>(1, rect.right - rect.left);
  const int h = std::max<LONG>(1, rect.bottom - rect.top);
  const FLOAT ppd = static_cast<FLOAT>(sdpi > 0 ? sdpi : 96) / 96.0f;
  const FLOAT layout_w_dip = std::max<FLOAT>(1.0f, static_cast<FLOAT>(w) / ppd);
  const FLOAT layout_h_dip = std::max<FLOAT>(1.0f, static_cast<FLOAT>(h) / ppd);
  HRESULT hr = S_OK;
  IDWriteFactory* factory = nullptr;
  IDWriteGdiInterop* gdi = nullptr;
  IDWriteBitmapRenderTarget* target = nullptr;
  IDWriteRenderingParams* params = nullptr;
  IDWriteTextFormat* format = nullptr;
  IDWriteTextLayout* layout = nullptr;
  IDWriteInlineObject* ellipsis = nullptr;
  hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                           reinterpret_cast<IUnknown**>(&factory));
  if (SUCCEEDED(hr)) hr = factory->GetGdiInterop(&gdi);
  if (SUCCEEDED(hr)) hr = gdi->CreateBitmapRenderTarget(dest, w, h, &target);
  if (SUCCEEDED(hr)) hr = factory->CreateRenderingParams(&params);
  if (SUCCEEDED(hr)) {
    hr = factory->CreateTextFormat(L"Segoe UI", nullptr, DWRITE_FONT_WEIGHT_SEMI_BOLD,
                                   DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
                                   12.0f, L"", &format);
  }
  if (SUCCEEDED(hr)) {
    format->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
    format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    const UINT32 len = text ? static_cast<UINT32>(wcsnlen_s(text, 1024)) : 0;
    hr = factory->CreateTextLayout(text ? text : L"", len, format,
                                   layout_w_dip, layout_h_dip, &layout);
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
    if (SUCCEEDED(hr)) BitBlt(dest, rect.left, rect.top, w, h, mem, 0, 0, SRCCOPY);
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
bool draw_dwrite_centered(HDC dest, RECT rect, const wchar_t* text, float size_dip,
                          DWRITE_FONT_WEIGHT weight, COLORREF bg, COLORREF fg, int sdpi) {
  const int w = std::max<LONG>(1, rect.right - rect.left);
  const int h = std::max<LONG>(1, rect.bottom - rect.top);
  const FLOAT ppd = static_cast<FLOAT>(sdpi > 0 ? sdpi : 96) / 96.0f;
  const FLOAT layout_w_dip = std::max<FLOAT>(1.0f, static_cast<FLOAT>(w) / ppd);
  const FLOAT layout_h_dip = std::max<FLOAT>(1.0f, static_cast<FLOAT>(h) / ppd);
  HRESULT hr = S_OK;
  IDWriteFactory* factory = nullptr;
  IDWriteGdiInterop* gdi = nullptr;
  IDWriteBitmapRenderTarget* target = nullptr;
  IDWriteRenderingParams* params = nullptr;
  IDWriteTextFormat* format = nullptr;
  IDWriteTextLayout* layout = nullptr;
  IDWriteInlineObject* ellipsis = nullptr;
  hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                           reinterpret_cast<IUnknown**>(&factory));
  if (SUCCEEDED(hr)) hr = factory->GetGdiInterop(&gdi);
  if (SUCCEEDED(hr)) hr = gdi->CreateBitmapRenderTarget(dest, w, h, &target);
  if (SUCCEEDED(hr)) hr = factory->CreateRenderingParams(&params);
  if (SUCCEEDED(hr)) {
    hr = factory->CreateTextFormat(L"Segoe UI", nullptr, weight,
                                   DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
                                   size_dip, L"", &format);
  }
  if (SUCCEEDED(hr)) {
    format->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
    format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    const UINT32 len = text ? static_cast<UINT32>(wcsnlen_s(text, 1024)) : 0;
    hr = factory->CreateTextLayout(text ? text : L"", len, format,
                                   layout_w_dip, layout_h_dip, &layout);
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
    if (SUCCEEDED(hr)) BitBlt(dest, rect.left, rect.top, w, h, mem, 0, 0, SRCCOPY);
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

void draw_centered_line(HDC dc, RECT rect, const wchar_t* text, float size_dip,
                        DWRITE_FONT_WEIGHT weight, COLORREF bg, COLORREF fg, int sdpi) {
  if (!text || !*text) return;
  if (draw_dwrite_centered(dc, rect, text, size_dip, weight, bg, fg, sdpi)) return;
  // GDI fallback: same rect, horizontally + vertically centered, ellipsized.
  SetBkMode(dc, TRANSPARENT);
  SetTextColor(dc, fg);
  HFONT font = CreateFontW(-MulDiv(static_cast<int>(size_dip), sdpi, 96), 0, 0, 0,
                           weight >= DWRITE_FONT_WEIGHT_SEMI_BOLD ? FW_SEMIBOLD : FW_NORMAL,
                           FALSE, FALSE, FALSE, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                           CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE,
                           L"Segoe UI");
  HGDIOBJ old_font = SelectObject(dc, font);
  RECT r = rect;
  DrawTextW(dc, text, -1, &r,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
  SelectObject(dc, old_font);
  DeleteObject(font);
}

// Paint the content child's "Loading Plugin" / failure overlay. Drawn only
// before the plug-in's IPlugView attaches (afterwards the plug-in's own child
// HWNDs cover the content). `c` may be null during early window creation.
void paint_content_overlay(HWND content, Context* c) {
  PAINTSTRUCT ps{};
  HDC dc = BeginPaint(content, &ps);
  if (!dc) return;
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
    RECT r2{rc.left + pad, top + line1_h + gap, rc.right - pad, top + line1_h + gap + line2_h};

    std::wstring secondary =
        c && !c->plugin_name.empty() ? c->plugin_name : std::wstring(L"Unknown Plugin");
    if (c && c->load_failed) {
      draw_centered_line(mem, r1, L"Plugin Failed to Load", 12.0f,
                         DWRITE_FONT_WEIGHT_SEMI_BOLD, bg, error, sdpi);
      const std::wstring detail = !c->error_text.empty() ? c->error_text : secondary;
      draw_centered_line(mem, r2, detail.c_str(), 13.0f, DWRITE_FONT_WEIGHT_NORMAL, bg,
                         label, sdpi);
    } else {
      draw_centered_line(mem, r1, L"Loading Plugin", 12.0f, DWRITE_FONT_WEIGHT_NORMAL, bg,
                         label, sdpi);
      draw_centered_line(mem, r2, secondary.c_str(), 15.0f, DWRITE_FONT_WEIGHT_SEMI_BOLD, bg,
                         name, sdpi);
    }
  }

  BitBlt(dc, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
  SelectObject(mem, old_bmp);
  DeleteObject(bmp);
  DeleteDC(mem);
  EndPaint(content, &ps);
}

int button_at(HWND h, int x, int y) {
  const int th = titlebar_h(h);
  if (y < 0 || y >= th) return kBtnNone;
  RECT rc{};
  GetClientRect(h, &rc);
  const int cw = rc.right - rc.left;
  const int bw = button_w(h);
  if (x >= cw - bw) return kBtnClose;
  if (x >= cw - 2 * bw) return kBtnMax;
  if (x >= cw - 3 * bw) return kBtnMin;
  if (x >= cw - 4 * bw) return kBtnPin;
  return kBtnNone;
}

void invalidate_titlebar(HWND h) {
  RECT tb{};
  GetClientRect(h, &tb);
  tb.bottom = titlebar_h(h);
  InvalidateRect(h, &tb, FALSE);
}

void paint_titlebar(HWND h, Context* c) {
  PAINTSTRUCT ps{};
  HDC dc = BeginPaint(h, &ps);
  if (!dc) return;
  const int sdpi = static_cast<int>(dpi(h));
  RECT rc{};
  GetClientRect(h, &rc);
  const int cw = rc.right - rc.left;
  const TitlebarLayout layout = compute_titlebar_layout(cw, sdpi);
  log_titlebar_layout_if_needed(h, c, layout);
  const int th = layout.titlebar_h;
  if (cw <= 0 || th <= 0) {
    EndPaint(h, &ps);
    return;
  }

  const COLORREF bg = RGB(24, 25, 28);
  const COLORREF border = RGB(44, 46, 51);
  const COLORREF title_text = RGB(220, 221, 225);
  const COLORREF glyph = RGB(205, 206, 210);
  const COLORREF glyph_hot = RGB(245, 246, 248);
  const COLORREF button_hot = RGB(45, 47, 53);
  const COLORREF close_hot = RGB(196, 43, 43);
  const int bw = layout.button_w;
  const int hover = c ? c->hover : kBtnNone;

  HDC mem = CreateCompatibleDC(dc);
  HBITMAP bmp = CreateCompatibleBitmap(dc, cw, th);
  HGDIOBJ old_bmp = SelectObject(mem, bmp);
  RECT strip{0, 0, cw, th};
  HBRUSH bgb = CreateSolidBrush(bg);
  FillRect(mem, &strip, bgb);
  DeleteObject(bgb);

  for (int b = kBtnPin; b <= kBtnClose; ++b) {
    if (b != hover && !(b == kBtnPin && c && c->window.pinned)) continue;
    RECT br = layout.buttons[b];
    HBRUSH hb = CreateSolidBrush(b == kBtnClose ? close_hot : button_hot);
    FillRect(mem, &br, hb);
    DeleteObject(hb);
  }

  wchar_t title[256] = {0};
  if (GetWindowTextW(h, title, 255) <= 0) wcscpy_s(title, 256, L"Plugin Editor");
  RECT tr = layout.title_text_rect;
  if (!draw_dwrite_text(mem, tr, title, bg, title_text, sdpi)) {
    SetBkMode(mem, TRANSPARENT);
    SetTextColor(mem, title_text);
    HFONT font = CreateFontW(-MulDiv(12, sdpi, 96), 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE,
                             DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                             CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI");
    HGDIOBJ old_font = SelectObject(mem, font);
    DrawTextW(mem, title, -1, &tr, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
    SelectObject(mem, old_font);
    DeleteObject(font);
  }

  const int pw = std::max<int>(1, MulDiv(1, sdpi, 96));
  const int g = std::max<int>(3, MulDiv(5, sdpi, 96));
  for (int b = kBtnPin; b <= kBtnClose; ++b) {
    const RECT br = layout.buttons[b];
    const int cx = br.left + bw / 2;
    const int cy = th / 2;
    const bool active_pin = b == kBtnPin && c && c->window.pinned;
    HPEN pen = CreatePen(PS_SOLID, pw, (b == hover || active_pin) ? glyph_hot : glyph);
    HGDIOBJ old_pen = SelectObject(mem, pen);
    HGDIOBJ old_br = SelectObject(mem, GetStockObject(NULL_BRUSH));
    if (b == kBtnPin) {
      MoveToEx(mem, cx, cy - g - 2, nullptr); LineTo(mem, cx, cy + g + 2);
      MoveToEx(mem, cx - g, cy - g + 1, nullptr); LineTo(mem, cx + g + 1, cy - g + 1);
      MoveToEx(mem, cx - g + 2, cy - g + 1, nullptr); LineTo(mem, cx - g + 2, cy + 1);
      MoveToEx(mem, cx + g - 1, cy - g + 1, nullptr); LineTo(mem, cx + g - 1, cy + 1);
    } else if (b == kBtnMin) {
      MoveToEx(mem, cx - g, cy, nullptr); LineTo(mem, cx + g + 1, cy);
    } else if (b == kBtnMax) {
      Rectangle(mem, cx - g, cy - g, cx + g + 1, cy + g + 1);
    } else {
      MoveToEx(mem, cx - g, cy - g, nullptr); LineTo(mem, cx + g + 1, cy + g + 1);
      MoveToEx(mem, cx - g, cy + g, nullptr); LineTo(mem, cx + g + 1, cy - g - 1);
    }
    SelectObject(mem, old_br);
    SelectObject(mem, old_pen);
    DeleteObject(pen);
  }

  HPEN bpen = CreatePen(PS_SOLID, 1, border);
  HGDIOBJ old_bpen = SelectObject(mem, bpen);
  MoveToEx(mem, 0, th - 1, nullptr);
  LineTo(mem, cw, th - 1);
  SelectObject(mem, old_bpen);
  DeleteObject(bpen);
  BitBlt(dc, 0, 0, cw, th, mem, 0, 0, SRCCOPY);
  SelectObject(mem, old_bmp);
  DeleteObject(bmp);
  DeleteDC(mem);
  EndPaint(h, &ps);
}

bool content_screen_rect(HWND parent, int x, int y, int w, int h, RECT* out) {
  if (!parent || !IsWindow(parent) || !out || w <= 0 || h <= 0) return false;
  POINT tl{x, y};
  POINT br{x + w, y + h};
  if (!ClientToScreen(parent, &tl) || !ClientToScreen(parent, &br)) return false;
  out->left = tl.x;
  out->top = tl.y;
  out->right = br.x;
  out->bottom = br.y;
  return true;
}

void apply_owned_popup_styles(HWND editor, HWND owner) {
  if (!editor || !IsWindow(editor)) return;
  const bool owner_valid = owner && IsWindow(owner);
  LONG_PTR ex = GetWindowLongPtrW(editor, GWL_EXSTYLE);
  ex &= ~WS_EX_APPWINDOW;
  ex |= WS_EX_TOOLWINDOW;
  SetWindowLongPtrW(editor, GWL_EXSTYLE, ex);
  if (owner_valid) SetWindowLongPtrW(editor, GWLP_HWNDPARENT, reinterpret_cast<LONG_PTR>(owner));
  SetWindowPos(editor, nullptr, 0, 0, 0, 0,
               SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
  const LONG_PTR applied_ex = GetWindowLongPtrW(editor, GWL_EXSTYLE);
  std::fprintf(stderr, "[NativeEditorShell] owner_hwnd=0x%p\n", ptr(owner));
  std::fprintf(stderr, "[NativeEditorShell] owner valid=%s\n", owner_valid ? "true" : "false");
  std::fprintf(stderr, "[NativeEditorShell] exstyle toolwindow=%s appwindow=%s\n",
               (applied_ex & WS_EX_TOOLWINDOW) ? "true" : "false",
               (applied_ex & WS_EX_APPWINDOW) ? "true" : "false");
  std::fprintf(stderr, "[NativeEditorShell] taskbar_hidden=%s\n",
               ((applied_ex & WS_EX_TOOLWINDOW) && !(applied_ex & WS_EX_APPWINDOW)) ? "true" : "false");
}

LRESULT hit_test(HWND h, Context* c, LPARAM lp) {
  POINT pt{static_cast<int>(static_cast<short>(LOWORD(lp))),
           static_cast<int>(static_cast<short>(HIWORD(lp)))};
  ScreenToClient(h, &pt);
  RECT rc{};
  GetClientRect(h, &rc);
  const int cw = rc.right;
  const int ch = rc.bottom;
  if (can_resize(c) && !IsZoomed(h)) {
    const int grab = std::max<int>(4, MulDiv(6, static_cast<int>(dpi(h)), 96));
    const bool l = pt.x < grab, r = pt.x >= cw - grab;
    const bool t = pt.y < grab, b = pt.y >= ch - grab;
    if (t && l) return HTTOPLEFT;
    if (t && r) return HTTOPRIGHT;
    if (b && l) return HTBOTTOMLEFT;
    if (b && r) return HTBOTTOMRIGHT;
    if (l) return HTLEFT;
    if (r) return HTRIGHT;
    if (t) return HTTOP;
    if (b) return HTBOTTOM;
  }
  const int th = titlebar_h(h);
  if (pt.y >= 0 && pt.y < th) {
    if (button_at(h, pt.x, pt.y) != kBtnNone) return HTCLIENT;
    return HTCAPTION;
  }
  return HTCLIENT;
}

void resize_content(HWND h, Context* c) {
  HWND content = hwnd(c->window.content_hwnd);
  if (!content || !IsWindow(content) || resizing(c)) return;
  RECT rc{};
  GetClientRect(h, &rc);
  const int th = c->window.host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow) ? titlebar_h(h) : 0;
  const int w = std::max<LONG>(0, rc.right - rc.left);
  const int ht = std::max<LONG>(0, (rc.bottom - rc.top) - th);
  if (w <= 0 || ht <= 0) return;
  set_resizing(c, true);
  SetWindowPos(content, nullptr, 0, th, w, ht, SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
  set_resizing(c, false);
  c->window.content_width = w;
  c->window.content_height = ht;
  std::fprintf(stderr, "[plugin-view] resize top=(%d,%d) content=(%d,%d)\n", w, ht, w, ht);
  if (attached(c) && c->cb.on_content_resized) c->cb.on_content_resized(c->cb.user_data, ptr(content), w, ht);
}

bool handle_sizing(HWND h, Context* c, WPARAM wp, LPARAM lp, bool borderless) {
  if (!attached(c) || !lp || !can_resize(c)) return false;
  RECT* drag = reinterpret_cast<RECT*>(lp);
  int nc_w = 0;
  int nc_h = 0;
  if (borderless) {
    nc_h = titlebar_h(h);
  } else {
    RECT frame{0, 0, 0, 0};
    const DWORD style = static_cast<DWORD>(GetWindowLongPtrW(h, GWL_STYLE));
    const DWORD ex_style = static_cast<DWORD>(GetWindowLongPtrW(h, GWL_EXSTYLE));
    if (!AdjustWindowRectExForDpi(&frame, style, FALSE, ex_style, dpi(h))) {
      AdjustWindowRectEx(&frame, style, FALSE, ex_style);
    }
    nc_w = static_cast<int>(frame.right - frame.left);
    nc_h = static_cast<int>(frame.bottom - frame.top);
  }
  int w = static_cast<int>(drag->right - drag->left) - nc_w;
  int ht = static_cast<int>(drag->bottom - drag->top) - nc_h;
  if (w <= 0 || ht <= 0 || !constrain(c, &w, &ht)) return false;
  const int ow = w + nc_w;
  const int oh = ht + nc_h;
  switch (wp) {
    case WMSZ_LEFT:
    case WMSZ_TOPLEFT:
    case WMSZ_BOTTOMLEFT: drag->left = drag->right - ow; break;
    default: drag->right = drag->left + ow; break;
  }
  switch (wp) {
    case WMSZ_TOP:
    case WMSZ_TOPLEFT:
    case WMSZ_TOPRIGHT: drag->top = drag->bottom - oh; break;
    default: drag->bottom = drag->top + oh; break;
  }
  return true;
}

LRESULT CALLBACK content_proc(HWND h, UINT msg, WPARAM wp, LPARAM lp) {
  log_message("plugin-content-hwnd", h, msg);
  switch (msg) {
    case WM_TIMER:
      if (wp == kWakeTimerTop || wp == kWakeTimerContent) {
        KillTimer(h, wp);
        return 0;
      }
      break;
    case WM_ERASEBKGND: return 1;  // fully repainted in WM_PAINT (no flicker)
    case WM_PAINT: {
      auto* c = reinterpret_cast<Context*>(GetWindowLongPtrW(h, GWLP_USERDATA));
      paint_content_overlay(h, c);
      return 0;
    }
    case WM_MOUSEACTIVATE: return MA_ACTIVATE;
    case WM_LBUTTONDOWN: {
      const POINT pt{static_cast<short>(LOWORD(lp)), static_cast<short>(HIWORD(lp))};
      HWND target = ChildWindowFromPointEx(h, pt, CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT);
      SetFocus(target ? target : h);
      break;
    }
    default: break;
  }
  return DefWindowProcW(h, msg, wp, lp);
}

LRESULT CALLBACK top_proc(HWND h, UINT msg, WPARAM wp, LPARAM lp) {
  auto* c = reinterpret_cast<Context*>(GetWindowLongPtrW(h, GWLP_USERDATA));
  const LONG_PTR style = GetWindowLongPtrW(h, GWL_STYLE);
  const bool borderless = (style & WS_THICKFRAME) && !(style & WS_CAPTION);
  const bool ok = live(c);
  const bool detached = ok && c && c->window.host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow);
  log_message("plugin-top-hwnd", h, msg);
  switch (msg) {
    case WM_NCCREATE: {
      auto* create = reinterpret_cast<CREATESTRUCTW*>(lp);
      c = reinterpret_cast<Context*>(create ? create->lpCreateParams : nullptr);
      SetWindowLongPtrW(h, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(c));
      if (c) c->window.shell_hwnd = ptr(h);
      return TRUE;
    }
    case WM_NCCALCSIZE:
      if (wp && borderless) return 0;
      break;
    case WM_NCACTIVATE:
      if (borderless) {
        invalidate_titlebar(h);
        return 1;
      }
      break;
    case WM_NCHITTEST:
      if (detached) return hit_test(h, c, lp);
      break;
    case WM_LBUTTONDOWN:
      if (detached) {
        const int x = static_cast<int>(static_cast<short>(LOWORD(lp)));
        const int y = static_cast<int>(static_cast<short>(HIWORD(lp)));
        const int b = button_at(h, x, y);
        if (b != kBtnNone) {
          c->press = b;
          SetCapture(h);
        }
        return 0;
      }
      break;
    case WM_LBUTTONUP:
      if (detached) {
        const int x = static_cast<int>(static_cast<short>(LOWORD(lp)));
        const int y = static_cast<int>(static_cast<short>(HIWORD(lp)));
        const int press = c->press;
        c->press = kBtnNone;
        if (press != kBtnNone) {
          ReleaseCapture();
          if (button_at(h, x, y) == press) {
            if (press == kBtnPin) {
              DauxEditorWindow copy = c->window;
              daux_editor_set_pin_to_top(&copy, !c->window.pinned);
              c->window = copy;
            } else if (press == kBtnMin) {
              ShowWindow(h, SW_MINIMIZE);
            } else if (press == kBtnMax) {
              ShowWindow(h, IsZoomed(h) ? SW_RESTORE : SW_MAXIMIZE);
            } else {
              SendMessageW(h, WM_CLOSE, 0, 0);
            }
          }
        }
        return 0;
      }
      break;
    case WM_MOUSEMOVE:
      if (detached) {
        const int x = static_cast<int>(static_cast<short>(LOWORD(lp)));
        const int y = static_cast<int>(static_cast<short>(HIWORD(lp)));
        const int b = button_at(h, x, y);
        if (b != c->hover) {
          c->hover = b;
          invalidate_titlebar(h);
        }
        if (!c->tracking) {
          TRACKMOUSEEVENT tme{sizeof(TRACKMOUSEEVENT), TME_LEAVE, h, 0};
          TrackMouseEvent(&tme);
          c->tracking = true;
        }
        return 0;
      }
      break;
    case WM_MOUSELEAVE:
      if (detached) {
        c->tracking = false;
        if (c->hover != kBtnNone) {
          c->hover = kBtnNone;
          invalidate_titlebar(h);
        }
        return 0;
      }
      break;
    case WM_ERASEBKGND: return 1;
    case WM_TIMER:
      if (wp == kWakeTimerTop || wp == kWakeTimerContent) {
        KillTimer(h, wp);
        return 0;
      }
      break;
    case WM_PAINT:
      if (detached) {
        paint_titlebar(h, c);
        return 0;
      }
      {
        PAINTSTRUCT ps{};
        BeginPaint(h, &ps);
        EndPaint(h, &ps);
        return 0;
      }
    case WM_SIZE:
      if (wp == SIZE_MINIMIZED) return 0;
      if (ok && c) resize_content(h, c);
      if (detached) invalidate_titlebar(h);
      return 0;
    case WM_GETMINMAXINFO:
      if (detached && attached(c) && !resizing(c) && !can_resize(c) && lp) {
        RECT wr{};
        if (GetWindowRect(h, &wr)) {
          auto* mmi = reinterpret_cast<MINMAXINFO*>(lp);
          const POINT size{wr.right - wr.left, wr.bottom - wr.top};
          mmi->ptMinTrackSize = size;
          mmi->ptMaxTrackSize = size;
          mmi->ptMaxSize = size;
          return 0;
        }
      }
      break;
    case WM_SIZING:
      if (detached && handle_sizing(h, c, wp, lp, borderless)) return TRUE;
      break;
    case WM_DPICHANGED:
      if (ok && c && lp) {
        const unsigned old_dpi = dpi(h);
        const unsigned new_dpi = LOWORD(wp) ? LOWORD(wp) : old_dpi;
        const RECT* suggested = reinterpret_cast<RECT*>(lp);
        SetWindowPos(h, nullptr, suggested->left, suggested->top,
                     suggested->right - suggested->left, suggested->bottom - suggested->top,
                     SWP_NOZORDER | SWP_NOACTIVATE);
        std::fprintf(stderr, "[NativeEditorShell] wm_dpichanged old=%u new=%u\n",
                     old_dpi, new_dpi);
        std::fprintf(stderr, "[PluginEditor] WM_DPICHANGED dpi=%u\n", dpi(h));
        resize_content(h, c);
        invalidate_titlebar(h);
        HWND content = hwnd(c->window.content_hwnd);
        if (content && IsWindow(content) && attached(c) && c->cb.on_dpi_changed) {
          RECT rc{};
          GetClientRect(content, &rc);
          const int w = rc.right - rc.left;
          const int ht = rc.bottom - rc.top;
          if (w > 0 && ht > 0) c->cb.on_dpi_changed(c->cb.user_data, ptr(h), ptr(content), w, ht);
        }
        return 0;
      }
      break;
    case WM_CLOSE:
      if (ok && c && c->cb.on_close_requested) c->cb.on_close_requested(c->cb.user_data);
      ShowWindow(h, SW_HIDE);
      return 0;
    case WM_MOUSEACTIVATE: return MA_ACTIVATE;
    default: break;
  }
  return DefWindowProcW(h, msg, wp, lp);
}

void register_classes() {
  static std::once_flag once;
  std::call_once(once, [] {
    WNDCLASSEXW top{};
    top.cbSize = sizeof(top);
    top.lpfnWndProc = top_proc;
    top.hInstance = GetModuleHandleW(nullptr);
    top.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(32512));
    top.hbrBackground = nullptr;
    top.lpszClassName = kTopClass;
    RegisterClassExW(&top);

    WNDCLASSEXW content{};
    content.cbSize = sizeof(content);
    content.lpfnWndProc = content_proc;
    content.hInstance = GetModuleHandleW(nullptr);
    content.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(32512));
    content.hbrBackground = nullptr;
    content.lpszClassName = kContentClass;
    RegisterClassExW(&content);
  });
}

const char* renderer_name() {
  const char* renderer = std::getenv("FUTUREBOARD_EDITOR_RENDERER");
  if (!renderer || !*renderer) return "gdi_dwrite";
  if (_stricmp(renderer, "gdi") == 0) return "gdi";
  if (_stricmp(renderer, "gdi_dwrite") == 0) return "gdi_dwrite";
  if (_stricmp(renderer, "dx11_dwrite") == 0) return "dx11_dwrite";
  return "gdi_dwrite";
}

HWND create_content(HWND top, int w, int h, int y) {
  if (!top || !IsWindow(top)) return nullptr;
  return CreateWindowExW(WS_EX_NOPARENTNOTIFY, kContentClass, L"",
                         WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                         0, y, w > 0 ? w : 640, h > 0 ? h : 480,
                         top, nullptr, GetModuleHandleW(nullptr), nullptr);
}

HWND create_top(const DauxEditorWindowConfig& cfg, Context* ctx) {
  HWND parent = hwnd(cfg.owner_hwnd);
  DWORD style = WS_CLIPCHILDREN | WS_CLIPSIBLINGS;
  DWORD ex_style = WS_EX_NOPARENTNOTIFY;
  HWND owner = nullptr;
  if (cfg.host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow)) {
    style |= WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
    ex_style |= WS_EX_TOOLWINDOW;
    owner = (parent && IsWindow(parent)) ? parent : nullptr;
  } else if (cfg.host_kind == static_cast<int>(DauxEditorKind::OwnedToolWindow)) {
    style |= WS_POPUP;
    ex_style |= WS_EX_TOOLWINDOW;
    owner = parent;
  } else {
    style |= WS_CHILD | WS_VISIBLE;
    owner = parent;
  }

  RECT r{0, 0, cfg.content_width > 0 ? cfg.content_width : 640,
         cfg.content_height > 0 ? cfg.content_height : 480};
  if (cfg.host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow)) {
    r.bottom += MulDiv(kTitlebarLogicalH, static_cast<int>(parent ? dpi(parent) : 96), 96);
  } else if (parent && IsWindow(parent)) {
    if (!AdjustWindowRectExForDpi(&r, style, FALSE, ex_style, dpi(parent))) {
      AdjustWindowRectEx(&r, style, FALSE, ex_style);
    }
  } else {
    AdjustWindowRectEx(&r, style, FALSE, ex_style);
  }

  int x = cfg.x;
  int y = cfg.y;
  if (cfg.host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow)) {
    x = CW_USEDEFAULT;
    y = CW_USEDEFAULT;
  }
  if (cfg.host_kind == static_cast<int>(DauxEditorKind::OwnedToolWindow) && parent && IsWindow(parent)) {
    RECT screen{};
    if (content_screen_rect(parent, cfg.x, cfg.y, cfg.content_width, cfg.content_height, &screen)) {
      x = screen.left;
      y = screen.top;
    }
  } else if (cfg.host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow) && parent && IsWindow(parent)) {
    RECT pr{};
    if (GetWindowRect(parent, &pr)) {
      x = pr.left + 48;
      y = pr.top + 48;
    }
  }

  HWND top = CreateWindowExW(ex_style, kTopClass,
                             cfg.title && *cfg.title ? cfg.title : L"Plugin Editor",
                             style, x, y, r.right - r.left, r.bottom - r.top,
                             owner, nullptr, GetModuleHandleW(nullptr), ctx);
  if (top) {
    std::fprintf(stderr, "[NativeEditorShell] backend=cpp_shell\n");
    std::fprintf(stderr, "[NativeEditorShell] create hwnd=0x%p\n", ptr(top));
    std::fprintf(stderr, "[NativeEditorShell] style=0x%08lx exstyle=0x%08lx\n",
                 static_cast<unsigned long>(style), static_cast<unsigned long>(ex_style));
    std::fprintf(stderr, "[NativeEditorShell] renderer=%s%s\n",
                 renderer_name(), _stricmp(renderer_name(), "dx11_dwrite") == 0 ? "|fallback=gdi" : "");
    std::fprintf(stderr, "[NativeEditorShell] dwrite=forced gdi_fallback=true d2d=false\n");
    set_dark_titlebar(top);
    if (cfg.host_kind == static_cast<int>(DauxEditorKind::OwnedToolWindow)) {
      daux_editor_apply_tool_styles(ptr(top), cfg.owner_hwnd);
    }
    if (cfg.host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow)) {
      apply_owned_popup_styles(top, parent);
    }
  }
  return top;
}

}  // namespace
#endif

int daux_editor_resolve_host_kind() {
#if defined(_WIN32)
  const char* mode = std::getenv("FUTUREBOARD_PLUGIN_EDITOR_MODE");
  if (mode && *mode) {
    if (_stricmp(mode, "child") == 0 || _stricmp(mode, "ws_child") == 0) return 0;
    if (_stricmp(mode, "tool") == 0 || _stricmp(mode, "owned") == 0 ||
        _stricmp(mode, "popup") == 0 || _stricmp(mode, "default") == 0 ||
        _stricmp(mode, "embedded") == 0) return 1;
    if (_stricmp(mode, "detached") == 0 || _stricmp(mode, "external") == 0 ||
        _stricmp(mode, "window") == 0) return 2;
  }
  return 1;
#else
  return 1;
#endif
}

const char* daux_editor_host_kind_name(int kind) {
  if (kind == 2) return "DetachedNativeWindow";
  return kind == 1 ? "EmbeddedOwnedToolWindow" : "ChildHwndEmbed";
}

const char* daux_editor_selected_mode_label(int kind) {
#if defined(_WIN32)
  const char* mode = std::getenv("FUTUREBOARD_PLUGIN_EDITOR_MODE");
  if (mode && *mode) {
    if (kind == 2) return "detached";
    if (_stricmp(mode, "embedded") == 0) return "embedded";
    if (_stricmp(mode, "child") == 0 || _stricmp(mode, "ws_child") == 0) return "child";
  }
#endif
  return kind == 2 ? "detached" : "default";
}

bool daux_editor_env_truthy(const char* name) {
#if defined(_WIN32)
  const char* value = std::getenv(name);
  return value && (_stricmp(value, "1") == 0 || _stricmp(value, "true") == 0 ||
                   _stricmp(value, "yes") == 0 || _stricmp(value, "on") == 0);
#else
  (void)name;
  return false;
#endif
}

bool daux_editor_content_screen_rect(void* parent_hwnd, int x, int y, int width, int height,
                                     long* left, long* top, long* right, long* bottom) {
#if defined(_WIN32)
  RECT rc{};
  if (!content_screen_rect(hwnd(parent_hwnd), x, y, width, height, &rc)) return false;
  if (left) *left = rc.left;
  if (top) *top = rc.top;
  if (right) *right = rc.right;
  if (bottom) *bottom = rc.bottom;
  return true;
#else
  (void)parent_hwnd; (void)x; (void)y; (void)width; (void)height;
  (void)left; (void)top; (void)right; (void)bottom;
  return false;
#endif
}

void daux_editor_apply_tool_styles(void* shell_hwnd, void* owner_hwnd) {
#if defined(_WIN32)
  HWND overlay = hwnd(shell_hwnd);
  HWND owner = hwnd(owner_hwnd);
  if (!overlay || !IsWindow(overlay)) return;
  LONG_PTR ex = GetWindowLongPtr(overlay, GWL_EXSTYLE);
  ex &= ~WS_EX_APPWINDOW;
  ex |= WS_EX_TOOLWINDOW;
  SetWindowLongPtr(overlay, GWL_EXSTYLE, ex);
  if (owner && IsWindow(owner)) SetWindowLongPtrW(overlay, GWLP_HWNDPARENT, reinterpret_cast<LONG_PTR>(owner));
#else
  (void)shell_hwnd; (void)owner_hwnd;
#endif
}

void daux_editor_apply_owner(DauxEditorWindow* window, void* owner_hwnd) {
#if defined(_WIN32)
  if (!window) return;
  HWND shell = hwnd(window->shell_hwnd);
  HWND owner = hwnd(owner_hwnd);
  if (!shell || !IsWindow(shell)) return;
  apply_owned_popup_styles(shell, owner);
  window->owner_hwnd = owner_hwnd;
  if (auto* c = reinterpret_cast<Context*>(window->internal)) c->window.owner_hwnd = owner_hwnd;
#else
  (void)window; (void)owner_hwnd;
#endif
}

bool daux_editor_show_and_focus(DauxEditorWindow* window) {
#if defined(_WIN32)
  if (!window) return false;
  HWND editor = hwnd(window->shell_hwnd);
  HWND content = hwnd(window->content_hwnd);
  if (!editor || !IsWindow(editor)) return false;
  std::fprintf(stderr, "[NativeEditorShell] show/focus requested\n");
  ShowWindow(editor, SW_SHOWNORMAL);
  SetWindowPos(editor, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
  BringWindowToTop(editor);
  BOOL foreground = SetForegroundWindow(editor);
  if (!foreground) {
    HWND current = GetForegroundWindow();
    const DWORD fg_thread = current ? GetWindowThreadProcessId(current, nullptr) : 0;
    const DWORD editor_thread = GetWindowThreadProcessId(editor, nullptr);
    if (fg_thread && editor_thread && fg_thread != editor_thread) {
      AttachThreadInput(editor_thread, fg_thread, TRUE);
      foreground = SetForegroundWindow(editor);
      SetFocus(content && IsWindow(content) ? content : editor);
      AttachThreadInput(editor_thread, fg_thread, FALSE);
    }
  }
  SetFocus(content && IsWindow(content) ? content : editor);
  std::fprintf(stderr, "[NativeEditorShell] foreground result=%s\n", foreground ? "true" : "false");
  return foreground ? true : false;
#else
  (void)window;
  return false;
#endif
}

void daux_editor_raise_children(void* shell_hwnd) {
#if defined(_WIN32)
  HWND host = hwnd(shell_hwnd);
  if (!host || !IsWindow(host)) return;
  EnumChildWindows(host, [](HWND child, LPARAM) -> BOOL {
    ShowWindow(child, SW_SHOW);
    SetWindowPos(child, HWND_TOP, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    return TRUE;
  }, 0);
#else
  (void)shell_hwnd;
#endif
}

bool daux_editor_create_window(const DauxEditorWindowConfig* cfg, DauxEditorWindow* out) {
#if defined(_WIN32)
  if (!cfg || !out) return false;
  register_classes();
  auto* c = new Context();
  c->cb = cfg->callbacks;
  c->window.owner_hwnd = cfg->owner_hwnd;
  c->window.host_kind = cfg->host_kind;
  c->window.content_width = cfg->content_width > 0 ? cfg->content_width : 640;
  c->window.content_height = cfg->content_height > 0 ? cfg->content_height : 480;
  // Name shown in the content "Loading Plugin" overlay (falls back to title).
  if (cfg->plugin_name && *cfg->plugin_name) {
    c->plugin_name = cfg->plugin_name;
  } else if (cfg->title && *cfg->title) {
    c->plugin_name = cfg->title;
  }
  HWND top = create_top(*cfg, c);
  if (!top) {
    delete c;
    return false;
  }
  c->window.shell_hwnd = ptr(top);
  c->window.titlebar_height =
      cfg->host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow) ? titlebar_h(top) : 0;
  HWND content = create_content(top, c->window.content_width, c->window.content_height, c->window.titlebar_height);
  if (!content) {
    SetWindowLongPtrW(top, GWLP_USERDATA, 0);
    DestroyWindow(top);
    delete c;
    return false;
  }
  c->window.content_hwnd = ptr(content);
  c->window.internal = c;
  // Let `content_proc` reach the Context (loading/attached state + plugin name)
  // so it can paint the "Loading Plugin" overlay until the view attaches.
  SetWindowLongPtrW(content, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(c));
  *out = c->window;
  SetTimer(top, kWakeTimerTop, 250, nullptr);
  SetTimer(content, kWakeTimerContent, 250, nullptr);
  if (cfg->host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow)) {
    daux_editor_set_pin_to_top(out, cfg->pin_default);
    c->window = *out;
  }
  return true;
#else
  (void)cfg; (void)out;
  return false;
#endif
}

void daux_editor_destroy_window(DauxEditorWindow* window) {
#if defined(_WIN32)
  if (!window) return;
  auto* c = reinterpret_cast<Context*>(window->internal);
  HWND content = hwnd(window->content_hwnd);
  HWND shell = hwnd(window->shell_hwnd);
  if (content && IsWindow(content)) {
    SetWindowLongPtrW(content, GWLP_USERDATA, 0);
    DestroyWindow(content);
  }
  if (shell && IsWindow(shell)) {
    SetWindowLongPtrW(shell, GWLP_USERDATA, 0);
    DestroyWindow(shell);
  }
  delete c;
  *window = DauxEditorWindow{};
#else
  (void)window;
#endif
}

void daux_editor_set_load_state(DauxEditorWindow* window, bool failed, const wchar_t* message) {
#if defined(_WIN32)
  if (!window) return;
  auto* c = reinterpret_cast<Context*>(window->internal);
  if (!c) return;
  c->load_failed = failed;
  c->error_text = (failed && message) ? std::wstring(message) : std::wstring();
  HWND content = hwnd(window->content_hwnd);
  if (content && IsWindow(content)) {
    InvalidateRect(content, nullptr, FALSE);
    UpdateWindow(content);
  }
  std::fprintf(stderr, "[NativeEditorShell] load_state=%s\n", failed ? "failed" : "loading");
#else
  (void)window;
  (void)failed;
  (void)message;
#endif
}

bool daux_editor_resize_content(DauxEditorWindow* window, int content_w, int content_h) {
#if defined(_WIN32)
  if (!window || content_w <= 0 || content_h <= 0) return false;
  HWND top = hwnd(window->shell_hwnd);
  HWND content = hwnd(window->content_hwnd);
  if (!top || !IsWindow(top)) return false;
  const int th = titlebar_h(top);
  const int win_w = content_w;
  const int win_h = content_h + th;
  bool changed = false;
  RECT cur{};
  GetWindowRect(top, &cur);
  auto* c = reinterpret_cast<Context*>(window->internal);
  if ((cur.right - cur.left) != win_w || (cur.bottom - cur.top) != win_h) {
    set_resizing(c, true);
    SetWindowPos(top, nullptr, 0, 0, win_w, win_h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    if (content && IsWindow(content)) {
      SetWindowPos(content, nullptr, 0, th, content_w, content_h,
                   SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    set_resizing(c, false);
    RECT tb{0, 0, win_w, th};
    InvalidateRect(top, &tb, FALSE);
    changed = true;
  }
  window->titlebar_height = th;
  window->content_width = content_w;
  window->content_height = content_h;
  if (c) c->window = *window;
  return changed;
#else
  (void)window; (void)content_w; (void)content_h;
  return false;
#endif
}

void daux_editor_set_pin_to_top(DauxEditorWindow* window, bool pinned) {
#if defined(_WIN32)
  if (!window) return;
  HWND h = hwnd(window->shell_hwnd);
  if (!h || !IsWindow(h)) return;
  window->pinned = pinned;
  SetWindowPos(h, pinned ? HWND_TOPMOST : HWND_NOTOPMOST, 0, 0, 0, 0,
               SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
  std::fprintf(stderr, "[NativeEditorShell] pin_to_top=%s\n", pinned ? "true" : "false");
  if (auto* c = reinterpret_cast<Context*>(window->internal)) c->window.pinned = pinned;
  invalidate_titlebar(h);
#else
  (void)window; (void)pinned;
#endif
}

bool daux_editor_is_window_valid(const DauxEditorWindow* window) {
#if defined(_WIN32)
  if (!window) return false;
  HWND shell = hwnd(window->shell_hwnd);
  HWND content = hwnd(window->content_hwnd);
  return shell && IsWindow(shell) && content && IsWindow(content);
#else
  (void)window;
  return false;
#endif
}

void* daux_editor_get_content_hwnd(const DauxEditorWindow* window) {
  return window ? window->content_hwnd : nullptr;
}

int daux_editor_titlebar_height(void* shell_hwnd) {
#if defined(_WIN32)
  return titlebar_h(hwnd(shell_hwnd));
#else
  (void)shell_hwnd;
  return 0;
#endif
}

unsigned daux_editor_hwnd_dpi(void* h) {
#if defined(_WIN32)
  return dpi(hwnd(h));
#else
  (void)h;
  return 96;
#endif
}

void daux_editor_settle_pump_thread(unsigned max_ms) {
#if defined(_WIN32)
  const ULONGLONG start = GetTickCount64();
  constexpr int kIdlePollsToSettle = 6;
  int idle = 0;
  int dispatched = 0;
  while (GetTickCount64() - start < max_ms) {
    MSG m;
    bool any = false;
    while (PeekMessageW(&m, nullptr, 0, 0, PM_REMOVE)) {
      TranslateMessage(&m);
      DispatchMessageW(&m);
      any = true;
      ++dispatched;
      if (GetTickCount64() - start >= max_ms) break;
    }
    if (any) {
      idle = 0;
    } else if (++idle >= kIdlePollsToSettle) {
      break;
    }
    Sleep(1);
  }
  std::fprintf(stderr, "[vst3-editor] settle_pump dispatched=%d elapsed_ms=%llu\n",
               dispatched, static_cast<unsigned long long>(GetTickCount64() - start));
#else
  (void)max_ms;
#endif
}
