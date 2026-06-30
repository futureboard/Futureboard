#pragma once

#include "../../../include/editor_windows.hpp"

#include <string>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <dwmapi.h>
#include <dwrite.h>
#include <windows.h>

namespace daux_editor_windows {

constexpr const wchar_t *kTopClass = L"FutureboardDauxVst3EditorDetached";
constexpr const wchar_t *kContentClass = L"FutureboardDauxVst3EditorContent";
constexpr UINT_PTR kWakeTimerTop = 0xDA01;
constexpr UINT_PTR kWakeTimerContent = 0xDA02;
constexpr int kTitlebarLogicalH = 32;
constexpr int kTitleButtonLogicalW = 46;
constexpr int kTitleTextLeftLogical = 12;
constexpr int kTitleTextRightGapLogical = 8;

enum Button {
  kBtnNone = -1,
  kBtnPin = 0,
  kBtnMin = 1,
  kBtnMax = 2,
  kBtnClose = 3
};

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
  std::wstring plugin_name;
  std::wstring error_text;
  bool load_failed{false};
};

HWND hwnd(void *p);
void *ptr(HWND h);
unsigned dpi(HWND h);
int titlebar_h(HWND h);
int button_w(HWND h);
int daux_dpi_scale(int value, int sdpi);
float daux_dpi_scale_f(float value, int sdpi);
RECT button_rect_px(int client_w, int bw, int th, int button);
TitlebarLayout compute_titlebar_layout(int client_w, int sdpi);
void log_titlebar_layout_if_needed(HWND h, Context *c,
                                   const TitlebarLayout &layout);
bool live(Context *c);
bool attached(Context *c);
bool resizing(Context *c);
void set_resizing(Context *c, bool v);
bool can_resize(Context *c);
bool constrain(Context *c, int *w, int *h);
bool message_debug();
void log_message(const char *tag, HWND h, UINT msg);
void set_dark_titlebar(HWND h);

bool draw_dwrite_text(HDC dest, RECT rect, const wchar_t *text, COLORREF bg,
                      COLORREF fg, int sdpi);
bool draw_dwrite_centered(HDC dest, RECT rect, const wchar_t *text,
                          float size_dip, DWRITE_FONT_WEIGHT weight,
                          COLORREF bg, COLORREF fg, int sdpi);
void draw_centered_line(HDC dc, RECT rect, const wchar_t *text, float size_dip,
                        DWRITE_FONT_WEIGHT weight, COLORREF bg, COLORREF fg,
                        int sdpi);
void paint_content_overlay(HWND content, Context *c);

int button_at(HWND h, int x, int y);
void invalidate_titlebar(HWND h);
void paint_titlebar(HWND h, Context *c);

bool content_screen_rect(HWND parent, int x, int y, int w, int h, RECT *out);
std::wstring hwnd_title(HWND h);
std::wstring hwnd_class(HWND h);
void log_hwnd_identity(const char *label, HWND h);
HWND normalize_owner_hwnd(HWND owner);
void apply_owned_popup_styles(HWND editor, HWND owner);

LRESULT hit_test(HWND h, Context *c, LPARAM lp);
void resize_content(HWND h, Context *c);
bool handle_sizing(HWND h, Context *c, WPARAM wp, LPARAM lp, bool borderless);
LRESULT CALLBACK content_proc(HWND h, UINT msg, WPARAM wp, LPARAM lp);
LRESULT CALLBACK top_proc(HWND h, UINT msg, WPARAM wp, LPARAM lp);

void register_classes();
const char *renderer_name();
HWND create_content(HWND top, int w, int h, int y);
HWND create_top(const DauxEditorWindowConfig &cfg, Context *ctx);

} // namespace daux_editor_windows
