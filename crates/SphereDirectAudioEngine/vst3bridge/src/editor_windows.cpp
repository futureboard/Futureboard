#include "../include/editor_windows.hpp"

#if !defined(_WIN32)

int daux_editor_resolve_host_kind() { return 1; }

const char *daux_editor_host_kind_name(int kind) {
  if (kind == 2)
    return "DetachedNativeWindow";
  return kind == 1 ? "EmbeddedOwnedToolWindow" : "ChildHwndEmbed";
}

const char *daux_editor_selected_mode_label(int kind) {
  if (kind == 2)
    return "detached_top_level";
  return kind == 1 ? "owned_top_level" : "legacy";
}

bool daux_editor_env_truthy(const char *name) {
  (void)name;
  return false;
}

bool daux_editor_content_screen_rect(void *parent_hwnd, int x, int y, int width,
                                     int height, long *left, long *top,
                                     long *right, long *bottom) {
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
}

void daux_editor_apply_tool_styles(void *shell_hwnd, void *owner_hwnd) {
  (void)shell_hwnd;
  (void)owner_hwnd;
}

void daux_editor_apply_owner(DauxEditorWindow *window, void *owner_hwnd) {
  (void)window;
  (void)owner_hwnd;
}

bool daux_editor_show_and_focus(DauxEditorWindow *window) {
  (void)window;
  return false;
}

void daux_editor_raise_children(void *shell_hwnd) { (void)shell_hwnd; }

bool daux_editor_create_window(const DauxEditorWindowConfig *config,
                               DauxEditorWindow *out_window) {
  (void)config;
  (void)out_window;
  return false;
}

void daux_editor_destroy_window(DauxEditorWindow *window) { (void)window; }

void daux_editor_set_load_state(DauxEditorWindow *window, bool failed,
                                const wchar_t *message) {
  (void)window;
  (void)failed;
  (void)message;
}

bool daux_editor_resize_content(DauxEditorWindow *window, int content_width,
                                int content_height) {
  (void)window;
  (void)content_width;
  (void)content_height;
  return false;
}

void daux_editor_set_pin_to_top(DauxEditorWindow *window, bool pinned) {
  (void)window;
  (void)pinned;
}

bool daux_editor_is_window_valid(const DauxEditorWindow *window) {
  (void)window;
  return false;
}

void *daux_editor_get_content_hwnd(const DauxEditorWindow *window) {
  return window ? window->content_hwnd : nullptr;
}

int daux_editor_titlebar_height(void *shell_hwnd) {
  (void)shell_hwnd;
  return 0;
}

unsigned daux_editor_hwnd_dpi(void *hwnd) {
  (void)hwnd;
  return 96;
}

void daux_editor_settle_pump_thread(unsigned max_ms) { (void)max_ms; }

#endif
