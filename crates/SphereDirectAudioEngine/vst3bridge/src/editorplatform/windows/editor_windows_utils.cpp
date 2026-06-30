#include "editor_windows_internal.hpp"

#include <cstdio>

namespace daux_editor_windows {

bool content_screen_rect(HWND parent, int x, int y, int w, int h, RECT *out) {
  if (!parent || !IsWindow(parent) || !out || w <= 0 || h <= 0)
    return false;
  POINT tl{x, y};
  POINT br{x + w, y + h};
  if (!ClientToScreen(parent, &tl) || !ClientToScreen(parent, &br))
    return false;
  out->left = tl.x;
  out->top = tl.y;
  out->right = br.x;
  out->bottom = br.y;
  return true;
}

std::wstring hwnd_title(HWND h) {
  wchar_t buf[256]{};
  int len = h && IsWindow(h) ? GetWindowTextW(h, buf, 256) : 0;
  return len > 0 ? std::wstring(buf, buf + len) : std::wstring();
}

std::wstring hwnd_class(HWND h) {
  wchar_t buf[128]{};
  int len = h && IsWindow(h) ? GetClassNameW(h, buf, 128) : 0;
  return len > 0 ? std::wstring(buf, buf + len) : std::wstring();
}

void log_hwnd_identity(const char *label, HWND h) {
  if (!h || !IsWindow(h)) {
    std::fprintf(stderr, "[NativeEditorShellHWND] %s hwnd=0x%p valid=false\n",
                 label, ptr(h));
    return;
  }
  DWORD pid = 0;
  DWORD tid = GetWindowThreadProcessId(h, &pid);
  HWND parent = GetParent(h);
  HWND owner = GetWindow(h, GW_OWNER);
  const auto cls = hwnd_class(h);
  const auto title = hwnd_title(h);
  std::fprintf(stderr,
               "[NativeEditorShellHWND] %s hwnd=0x%p pid=%lu tid=%lu "
               "parent=0x%p owner=0x%p class='%ls' title='%ls'\n",
               label, ptr(h), static_cast<unsigned long>(pid),
               static_cast<unsigned long>(tid), ptr(parent), ptr(owner),
               cls.c_str(), title.c_str());
}

HWND normalize_owner_hwnd(HWND owner) {
  if (!owner || !IsWindow(owner))
    return nullptr;
  HWND root = GetAncestor(owner, GA_ROOT);
  if (root && IsWindow(root) && root != owner) {
    std::fprintf(stderr,
                 "[NativeEditorShell] normalized owner child=0x%p root=0x%p\n",
                 ptr(owner), ptr(root));
    return root;
  }
  return owner;
}

void apply_owned_popup_styles(HWND editor, HWND owner) {
  if (!editor || !IsWindow(editor))
    return;
  owner = normalize_owner_hwnd(owner);
  const bool owner_valid = owner && IsWindow(owner);
  log_hwnd_identity("shell", editor);
  log_hwnd_identity("owner", owner);
  LONG_PTR ex = GetWindowLongPtrW(editor, GWL_EXSTYLE);
  ex &= ~WS_EX_APPWINDOW;
  ex |= WS_EX_TOOLWINDOW;
  SetWindowLongPtrW(editor, GWL_EXSTYLE, ex);
  if (owner_valid)
    SetWindowLongPtrW(editor, GWLP_HWNDPARENT,
                      reinterpret_cast<LONG_PTR>(owner));
  SetWindowPos(editor, nullptr, 0, 0, 0, 0,
               SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE |
                   SWP_FRAMECHANGED);
  const LONG_PTR applied_ex = GetWindowLongPtrW(editor, GWL_EXSTYLE);
  std::fprintf(stderr, "[NativeEditorShell] owner_hwnd=0x%p\n", ptr(owner));
  std::fprintf(stderr, "[NativeEditorShell] owner valid=%s\n",
               owner_valid ? "true" : "false");
  std::fprintf(stderr,
               "[NativeEditorShell] exstyle toolwindow=%s appwindow=%s\n",
               (applied_ex & WS_EX_TOOLWINDOW) ? "true" : "false",
               (applied_ex & WS_EX_APPWINDOW) ? "true" : "false");
  std::fprintf(
      stderr, "[NativeEditorShell] taskbar_hidden=%s\n",
      ((applied_ex & WS_EX_TOOLWINDOW) && !(applied_ex & WS_EX_APPWINDOW))
          ? "true"
          : "false");
}

} // namespace daux_editor_windows
