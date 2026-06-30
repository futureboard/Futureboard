#include "editor_windows_internal.hpp"

#include <algorithm>

namespace daux_editor_windows {

int button_at(HWND h, int x, int y) {
  const int th = titlebar_h(h);
  if (y < 0 || y >= th)
    return kBtnNone;
  RECT rc{};
  GetClientRect(h, &rc);
  const int cw = rc.right - rc.left;
  const int bw = button_w(h);
  if (x >= cw - bw)
    return kBtnClose;
  if (x >= cw - 2 * bw)
    return kBtnMax;
  if (x >= cw - 3 * bw)
    return kBtnMin;
  if (x >= cw - 4 * bw)
    return kBtnPin;
  return kBtnNone;
}

void invalidate_titlebar(HWND h) {
  RECT tb{};
  GetClientRect(h, &tb);
  tb.bottom = titlebar_h(h);
  InvalidateRect(h, &tb, FALSE);
}

void paint_titlebar(HWND h, Context *c) {
  PAINTSTRUCT ps{};
  HDC dc = BeginPaint(h, &ps);
  if (!dc)
    return;
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
    if (b != hover && !(b == kBtnPin && c && c->window.pinned))
      continue;
    RECT br = layout.buttons[b];
    HBRUSH hb = CreateSolidBrush(b == kBtnClose ? close_hot : button_hot);
    FillRect(mem, &br, hb);
    DeleteObject(hb);
  }

  wchar_t title[256] = {0};
  if (GetWindowTextW(h, title, 255) <= 0)
    wcscpy_s(title, 256, L"Plugin Editor");
  RECT tr = layout.title_text_rect;
  if (!draw_dwrite_text(mem, tr, title, bg, title_text, sdpi)) {
    SetBkMode(mem, TRANSPARENT);
    SetTextColor(mem, title_text);
    HFONT font = CreateFontW(-MulDiv(12, sdpi, 96), 0, 0, 0, FW_NORMAL, FALSE,
                             FALSE, FALSE, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                             CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                             DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI");
    HGDIOBJ old_font = SelectObject(mem, font);
    DrawTextW(mem, title, -1, &tr,
              DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
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
    HPEN pen =
        CreatePen(PS_SOLID, pw, (b == hover || active_pin) ? glyph_hot : glyph);
    HGDIOBJ old_pen = SelectObject(mem, pen);
    HGDIOBJ old_br = SelectObject(mem, GetStockObject(NULL_BRUSH));
    if (b == kBtnPin) {
      MoveToEx(mem, cx, cy - g - 2, nullptr);
      LineTo(mem, cx, cy + g + 2);
      MoveToEx(mem, cx - g, cy - g + 1, nullptr);
      LineTo(mem, cx + g + 1, cy - g + 1);
      MoveToEx(mem, cx - g + 2, cy - g + 1, nullptr);
      LineTo(mem, cx - g + 2, cy + 1);
      MoveToEx(mem, cx + g - 1, cy - g + 1, nullptr);
      LineTo(mem, cx + g - 1, cy + 1);
    } else if (b == kBtnMin) {
      MoveToEx(mem, cx - g, cy, nullptr);
      LineTo(mem, cx + g + 1, cy);
    } else if (b == kBtnMax) {
      Rectangle(mem, cx - g, cy - g, cx + g + 1, cy + g + 1);
    } else {
      MoveToEx(mem, cx - g, cy - g, nullptr);
      LineTo(mem, cx + g + 1, cy + g + 1);
      MoveToEx(mem, cx - g, cy + g, nullptr);
      LineTo(mem, cx + g + 1, cy - g - 1);
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

} // namespace daux_editor_windows
