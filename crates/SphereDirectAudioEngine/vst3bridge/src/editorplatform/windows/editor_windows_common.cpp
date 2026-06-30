#include "editor_windows_internal.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>

namespace daux_editor_windows {

HWND hwnd(void *p) { return reinterpret_cast<HWND>(p); }
void *ptr(HWND h) { return reinterpret_cast<void *>(h); }

unsigned dpi(HWND h) {
  if (!h || !IsWindow(h))
    return 96;
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
    layout.buttons[b] =
        button_rect_px(layout.client_w, layout.button_w, layout.titlebar_h, b);
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

void log_titlebar_layout_if_needed(HWND h, Context *c,
                                   const TitlebarLayout &layout) {
  if (!c)
    return;
  if (c->logged_layout_dpi == layout.dpi &&
      c->logged_layout_w == layout.client_w)
    return;
  c->logged_layout_dpi = layout.dpi;
  c->logged_layout_w = layout.client_w;
  std::fprintf(stderr, "[NativeEditorShell] dpi=%d\n", layout.dpi);
  std::fprintf(stderr, "[NativeEditorShell] titlebar_h=%d\n",
               layout.titlebar_h);
  std::fprintf(stderr, "[NativeEditorShell] title_rect=(%ld,%ld,%ld,%ld)\n",
               layout.title_text_rect.left, layout.title_text_rect.top,
               layout.title_text_rect.right, layout.title_text_rect.bottom);
  const char *names[4] = {"pin", "min", "max", "close"};
  for (int b = kBtnPin; b <= kBtnClose; ++b) {
    const RECT &r = layout.buttons[b];
    std::fprintf(stderr,
                 "[NativeEditorShell] button_rect %s=(%ld,%ld,%ld,%ld)\n",
                 names[b], r.left, r.top, r.right, r.bottom);
  }
  std::fprintf(stderr, "[NativeEditorShell] dwrite_font_size=12.00dip %.2fpx\n",
               daux_dpi_scale_f(12.0f, layout.dpi));
  (void)h;
}

bool live(Context *c) {
  return c && (!c->cb.is_live || c->cb.is_live(c->cb.user_data));
}

bool attached(Context *c) {
  return c && c->cb.is_attached && c->cb.is_attached(c->cb.user_data);
}

bool resizing(Context *c) {
  return c && c->cb.is_resize_in_progress &&
         c->cb.is_resize_in_progress(c->cb.user_data);
}

void set_resizing(Context *c, bool v) {
  if (c && c->cb.set_resize_in_progress)
    c->cb.set_resize_in_progress(c->cb.user_data, v);
}

bool can_resize(Context *c) {
  return c && c->cb.can_resize && c->cb.can_resize(c->cb.user_data);
}

bool constrain(Context *c, int *w, int *h) {
  return c && c->cb.constrain_content_size &&
         c->cb.constrain_content_size(c->cb.user_data, w, h);
}

bool message_debug() {
  static const bool enabled =
      std::getenv("FUTUREBOARD_PLUGIN_VIEW_DEBUG") != nullptr ||
      std::getenv("FUTUREBOARD_VST3_EDITOR_DEBUG") != nullptr ||
      std::getenv("FUTUREBOARD_PLUGIN_DEBUG") != nullptr;
  return enabled;
}

void log_message(const char *tag, HWND h, UINT msg) {
  if (!message_debug())
    return;
  const char *name = nullptr;
  switch (msg) {
  case WM_CREATE:
    name = "WM_CREATE";
    break;
  case WM_SHOWWINDOW:
    name = "WM_SHOWWINDOW";
    break;
  case WM_SIZE:
    name = "WM_SIZE";
    break;
  case WM_CLOSE:
    name = "WM_CLOSE";
    break;
  case WM_DESTROY:
    name = "WM_DESTROY";
    break;
  case WM_DPICHANGED:
    name = "WM_DPICHANGED";
    break;
  case WM_PAINT:
    name = "WM_PAINT";
    break;
  case WM_ERASEBKGND:
    name = "WM_ERASEBKGND";
    break;
  case WM_TIMER:
    name = "WM_TIMER";
    break;
  default:
    break;
  }
  if (name) {
    std::fprintf(stderr, "[%s] %s hwnd=0x%p tid=%lu\n", tag, name, ptr(h),
                 GetCurrentThreadId());
  }
}

void set_dark_titlebar(HWND h) {
  if (!h)
    return;
  BOOL dark = TRUE;
  HRESULT dark_hr = DwmSetWindowAttribute(h, 20, &dark, sizeof(dark));
  if (FAILED(dark_hr))
    dark_hr = DwmSetWindowAttribute(h, 19, &dark, sizeof(dark));
  std::fprintf(stderr, "[NativeEditorShell] dwm dark_mode=%s hr=0x%08lx\n",
               SUCCEEDED(dark_hr) ? "ok" : "fail",
               static_cast<unsigned long>(dark_hr));

  const bool rounded_disabled = [] {
    const char *v = std::getenv("FUTUREBOARD_EDITOR_ROUNDED");
    return v && (_stricmp(v, "0") == 0 || _stricmp(v, "false") == 0 ||
                 _stricmp(v, "off") == 0);
  }();
  if (rounded_disabled) {
    std::fprintf(stderr, "[NativeEditorShell] rounded=disabled\n");
  } else {
    const int preference = 2;
    const HRESULT hr =
        DwmSetWindowAttribute(h, 33, &preference, sizeof(preference));
    std::fprintf(stderr, "[NativeEditorShell] rounded=%s\n",
                 SUCCEEDED(hr) ? "enabled" : "unsupported");
  }
  COLORREF border = RGB(44, 46, 51);
  const HRESULT border_hr =
      DwmSetWindowAttribute(h, 34, &border, sizeof(border));
  std::fprintf(stderr, "[NativeEditorShell] dwm border_color=%s\n",
               SUCCEEDED(border_hr) ? "ok" : "fail");
  const COLORREF caption = RGB(14, 19, 25);
  const HRESULT caption_hr =
      DwmSetWindowAttribute(h, DWMWA_CAPTION_COLOR, &caption, sizeof(caption));
  std::fprintf(stderr, "[NativeEditorShell] dwm caption_color=%s\n",
               SUCCEEDED(caption_hr) ? "ok" : "fail");
}

} // namespace daux_editor_windows
