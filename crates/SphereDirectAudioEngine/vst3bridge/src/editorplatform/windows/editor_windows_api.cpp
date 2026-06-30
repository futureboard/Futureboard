#include "editor_windows_internal.hpp"

#include <cstdio>
#include <cstdlib>

using namespace daux_editor_windows;

int daux_editor_resolve_host_kind() {
#if defined(_WIN32)
  const char *mode = std::getenv("FUTUREBOARD_PLUGIN_EDITOR_MODE");
  if (mode && *mode) {
    if (_stricmp(mode, "legacy") == 0 || _stricmp(mode, "default") == 0 ||
        _stricmp(mode, "child") == 0 || _stricmp(mode, "ws_child") == 0 ||
        _stricmp(mode, "embedded_child") == 0)
      return 0;
    if (_stricmp(mode, "tool") == 0 || _stricmp(mode, "owned") == 0 ||
        _stricmp(mode, "owned_top_level") == 0 ||
        _stricmp(mode, "popup") == 0 || _stricmp(mode, "embedded") == 0)
      return 1;
    if (_stricmp(mode, "detached") == 0 ||
        _stricmp(mode, "detached_top_level") == 0 ||
        _stricmp(mode, "external") == 0 || _stricmp(mode, "window") == 0)
      return 2;
  }
  return 0;
#else
  return 1;
#endif
}

const char *daux_editor_host_kind_name(int kind) {
  if (kind == 2)
    return "DetachedNativeWindow";
  return kind == 1 ? "EmbeddedOwnedToolWindow" : "ChildHwndEmbed";
}

const char *daux_editor_selected_mode_label(int kind) {
#if defined(_WIN32)
  const char *mode = std::getenv("FUTUREBOARD_PLUGIN_EDITOR_MODE");
  if (mode && *mode) {
    if (kind == 2)
      return "detached_top_level";
    if (_stricmp(mode, "embedded") == 0)
      return "embedded";
    if (_stricmp(mode, "child") == 0 || _stricmp(mode, "ws_child") == 0)
      return "child";
    if (kind == 1)
      return "owned_top_level";
  }
#endif
  if (kind == 2)
    return "detached_top_level";
  return kind == 1 ? "owned_top_level" : "legacy";
}

bool daux_editor_env_truthy(const char *name) {
#if defined(_WIN32)
  const char *value = std::getenv(name);
  return value && (_stricmp(value, "1") == 0 || _stricmp(value, "true") == 0 ||
                   _stricmp(value, "yes") == 0 || _stricmp(value, "on") == 0);
#else
  (void)name;
  return false;
#endif
}

bool daux_editor_content_screen_rect(void *parent_hwnd, int x, int y, int width,
                                     int height, long *left, long *top,
                                     long *right, long *bottom) {
#if defined(_WIN32)
  RECT rc{};
  if (!content_screen_rect(hwnd(parent_hwnd), x, y, width, height, &rc))
    return false;
  if (left)
    *left = rc.left;
  if (top)
    *top = rc.top;
  if (right)
    *right = rc.right;
  if (bottom)
    *bottom = rc.bottom;
  return true;
#else
  (void)parent_hwnd;
  (void)x;
  (void)y;
  (void)width;
  (void)height;
  (void)left;
  (void)top;
  (void)right;
  (void)bottom;
  return false;
#endif
}

void daux_editor_apply_tool_styles(void *shell_hwnd, void *owner_hwnd) {
#if defined(_WIN32)
  HWND overlay = hwnd(shell_hwnd);
  HWND owner = normalize_owner_hwnd(hwnd(owner_hwnd));
  if (!overlay || !IsWindow(overlay))
    return;
  log_hwnd_identity("tool_shell", overlay);
  log_hwnd_identity("tool_owner", owner);
  LONG_PTR ex = GetWindowLongPtr(overlay, GWL_EXSTYLE);
  ex &= ~WS_EX_APPWINDOW;
  ex |= WS_EX_TOOLWINDOW;
  SetWindowLongPtr(overlay, GWL_EXSTYLE, ex);
  if (owner && IsWindow(owner))
    SetWindowLongPtrW(overlay, GWLP_HWNDPARENT,
                      reinterpret_cast<LONG_PTR>(owner));
#else
  (void)shell_hwnd;
  (void)owner_hwnd;
#endif
}

void daux_editor_apply_owner(DauxEditorWindow *window, void *owner_hwnd) {
#if defined(_WIN32)
  if (!window)
    return;
  HWND shell = hwnd(window->shell_hwnd);
  HWND owner = normalize_owner_hwnd(hwnd(owner_hwnd));
  if (!shell || !IsWindow(shell))
    return;
  apply_owned_popup_styles(shell, owner);
  window->owner_hwnd = owner_hwnd;
  if (auto *c = reinterpret_cast<Context *>(window->internal))
    c->window.owner_hwnd = owner_hwnd;
#else
  (void)window;
  (void)owner_hwnd;
#endif
}

bool daux_editor_show_and_focus(DauxEditorWindow *window) {
#if defined(_WIN32)
  if (!window)
    return false;
  HWND editor = hwnd(window->shell_hwnd);
  HWND content = hwnd(window->content_hwnd);
  if (!editor || !IsWindow(editor))
    return false;
  std::fprintf(stderr, "[NativeEditorShell] show/focus requested\n");
  ShowWindow(editor, SW_SHOWNORMAL);
  SetWindowPos(editor, HWND_TOP, 0, 0, 0, 0,
               SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
  BringWindowToTop(editor);
  BOOL foreground = SetForegroundWindow(editor);
  if (!foreground) {
    HWND current = GetForegroundWindow();
    const DWORD fg_thread =
        current ? GetWindowThreadProcessId(current, nullptr) : 0;
    const DWORD editor_thread = GetWindowThreadProcessId(editor, nullptr);
    if (fg_thread && editor_thread && fg_thread != editor_thread) {
      AttachThreadInput(editor_thread, fg_thread, TRUE);
      foreground = SetForegroundWindow(editor);
      SetFocus(content && IsWindow(content) ? content : editor);
      AttachThreadInput(editor_thread, fg_thread, FALSE);
    }
  }
  SetFocus(content && IsWindow(content) ? content : editor);
  std::fprintf(stderr, "[NativeEditorShell] foreground result=%s\n",
               foreground ? "true" : "false");
  return foreground ? true : false;
#else
  (void)window;
  return false;
#endif
}

void daux_editor_raise_children(void *shell_hwnd) {
#if defined(_WIN32)
  HWND host = hwnd(shell_hwnd);
  if (!host || !IsWindow(host))
    return;
  EnumChildWindows(
      host,
      [](HWND child, LPARAM) -> BOOL {
        ShowWindow(child, SW_SHOW);
        SetWindowPos(child, HWND_TOP, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        return TRUE;
      },
      0);
#else
  (void)shell_hwnd;
#endif
}

bool daux_editor_create_window(const DauxEditorWindowConfig *cfg,
                               DauxEditorWindow *out) {
#if defined(_WIN32)
  if (!cfg || !out)
    return false;
  register_classes();
  auto *c = new Context();
  c->cb = cfg->callbacks;
  c->window.owner_hwnd = cfg->owner_hwnd;
  c->window.host_kind = cfg->host_kind;
  c->window.content_width = cfg->content_width > 0 ? cfg->content_width : 640;
  c->window.content_height =
      cfg->content_height > 0 ? cfg->content_height : 480;
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
      cfg->host_kind == static_cast<int>(DauxEditorKind::DetachedNativeWindow)
          ? titlebar_h(top)
          : 0;
  HWND content =
      create_content(top, c->window.content_width, c->window.content_height,
                     c->window.titlebar_height);
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
  if (cfg->host_kind ==
      static_cast<int>(DauxEditorKind::DetachedNativeWindow)) {
    daux_editor_set_pin_to_top(out, cfg->pin_default);
    c->window = *out;
  }
  return true;
#else
  (void)cfg;
  (void)out;
  return false;
#endif
}

void daux_editor_destroy_window(DauxEditorWindow *window) {
#if defined(_WIN32)
  if (!window)
    return;
  auto *c = reinterpret_cast<Context *>(window->internal);
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

void daux_editor_set_load_state(DauxEditorWindow *window, bool failed,
                                const wchar_t *message) {
#if defined(_WIN32)
  if (!window)
    return;
  auto *c = reinterpret_cast<Context *>(window->internal);
  if (!c)
    return;
  c->load_failed = failed;
  c->error_text = message ? std::wstring(message) : std::wstring();
  HWND content = hwnd(window->content_hwnd);
  if (content && IsWindow(content)) {
    InvalidateRect(content, nullptr, FALSE);
    UpdateWindow(content);
  }
  std::fprintf(stderr, "[NativeEditorShell] load_state=%s\n",
               failed ? "failed" : "loading");
#else
  (void)window;
  (void)failed;
  (void)message;
#endif
}

bool daux_editor_resize_content(DauxEditorWindow *window, int content_w,
                                int content_h) {
#if defined(_WIN32)
  if (!window || content_w <= 0 || content_h <= 0)
    return false;
  HWND top = hwnd(window->shell_hwnd);
  HWND content = hwnd(window->content_hwnd);
  if (!top || !IsWindow(top))
    return false;
  const int th = titlebar_h(top);
  const int win_w = content_w;
  const int win_h = content_h + th;
  bool changed = false;
  RECT cur{};
  GetWindowRect(top, &cur);
  auto *c = reinterpret_cast<Context *>(window->internal);
  if ((cur.right - cur.left) != win_w || (cur.bottom - cur.top) != win_h) {
    set_resizing(c, true);
    SetWindowPos(top, nullptr, 0, 0, win_w, win_h,
                 SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
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
  if (c)
    c->window = *window;
  return changed;
#else
  (void)window;
  (void)content_w;
  (void)content_h;
  return false;
#endif
}

void daux_editor_set_pin_to_top(DauxEditorWindow *window, bool pinned) {
#if defined(_WIN32)
  if (!window)
    return;
  HWND h = hwnd(window->shell_hwnd);
  if (!h || !IsWindow(h))
    return;
  window->pinned = pinned;
  SetWindowPos(h, pinned ? HWND_TOPMOST : HWND_NOTOPMOST, 0, 0, 0, 0,
               SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
  std::fprintf(stderr, "[NativeEditorShell] pin_to_top=%s\n",
               pinned ? "true" : "false");
  if (auto *c = reinterpret_cast<Context *>(window->internal))
    c->window.pinned = pinned;
  invalidate_titlebar(h);
#else
  (void)window;
  (void)pinned;
#endif
}

bool daux_editor_is_window_valid(const DauxEditorWindow *window) {
#if defined(_WIN32)
  if (!window)
    return false;
  HWND shell = hwnd(window->shell_hwnd);
  HWND content = hwnd(window->content_hwnd);
  return shell && IsWindow(shell) && content && IsWindow(content);
#else
  (void)window;
  return false;
#endif
}

void *daux_editor_get_content_hwnd(const DauxEditorWindow *window) {
  return window ? window->content_hwnd : nullptr;
}

int daux_editor_titlebar_height(void *shell_hwnd) {
#if defined(_WIN32)
  return titlebar_h(hwnd(shell_hwnd));
#else
  (void)shell_hwnd;
  return 0;
#endif
}

unsigned daux_editor_hwnd_dpi(void *h) {
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
      if (GetTickCount64() - start >= max_ms)
        break;
    }
    if (any) {
      idle = 0;
    } else if (++idle >= kIdlePollsToSettle) {
      break;
    }
    Sleep(1);
  }
  std::fprintf(
      stderr, "[vst3-editor] settle_pump dispatched=%d elapsed_ms=%llu\n",
      dispatched, static_cast<unsigned long long>(GetTickCount64() - start));
#else
  (void)max_ms;
#endif
}
