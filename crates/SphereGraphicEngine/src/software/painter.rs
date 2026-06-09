//! Reusable GDI paint primitives for native chrome.
//!
//! These are deliberately tiny, allocation-light helpers around the handful of
//! GDI calls every native shell repeats: solid fills, an inset frame (border
//! strips), and stroked outlines. They own brush/pen lifetimes so callers don't
//! leak GDI objects. **No Direct2D** is involved.

use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreatePen, CreateSolidBrush, DeleteObject, FillRect, LineTo, MoveToEx, SelectObject, HDC,
    PS_SOLID,
};

/// Zero-sized namespace for GDI paint primitives.
pub struct SoftwarePainter;

impl SoftwarePainter {
    /// Fill `rect` with a solid `color`. Creates and frees the brush internally.
    pub fn fill_solid(hdc: HDC, rect: &RECT, color: COLORREF) {
        unsafe {
            let brush = CreateSolidBrush(color);
            FillRect(hdc, rect, brush);
            let _ = DeleteObject(brush.into());
        }
    }

    /// Paint a `thickness`-pixel inset frame (top/bottom/left/right strips)
    /// around a `w` × `h` client area, in `color`. Used for the themed window
    /// border on a borderless shell.
    pub fn frame_inset(hdc: HDC, w: i32, h: i32, thickness: i32, color: COLORREF) {
        if w <= 0 || h <= 0 || thickness <= 0 {
            return;
        }
        let strips = [
            RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: thickness,
            },
            RECT {
                left: 0,
                top: h - thickness,
                right: w,
                bottom: h,
            },
            RECT {
                left: 0,
                top: 0,
                right: thickness,
                bottom: h,
            },
            RECT {
                left: w - thickness,
                top: 0,
                right: w,
                bottom: h,
            },
        ];
        unsafe {
            let brush = CreateSolidBrush(color);
            for strip in &strips {
                FillRect(hdc, strip, brush);
            }
            let _ = DeleteObject(brush.into());
        }
    }

    /// Stroke a single line from (`x1`,`y1`) to (`x2`,`y2`) with a solid pen.
    pub fn stroke_line(hdc: HDC, x1: i32, y1: i32, x2: i32, y2: i32, width: i32, color: COLORREF) {
        unsafe {
            let pen = CreatePen(PS_SOLID, width.max(1), color);
            let old = SelectObject(hdc, pen.into());
            let _ = MoveToEx(hdc, x1, y1, None);
            let _ = LineTo(hdc, x2, y2);
            SelectObject(hdc, old);
            let _ = DeleteObject(pen.into());
        }
    }

    /// Stroke a rectangle outline (`l`,`t`)–(`r`,`b`) with a solid pen.
    pub fn rect_outline(hdc: HDC, l: i32, t: i32, r: i32, b: i32, width: i32, color: COLORREF) {
        unsafe {
            let pen = CreatePen(PS_SOLID, width.max(1), color);
            let old = SelectObject(hdc, pen.into());
            let _ = MoveToEx(hdc, l, t, None);
            let _ = LineTo(hdc, r, t);
            let _ = LineTo(hdc, r, b);
            let _ = LineTo(hdc, l, b);
            let _ = LineTo(hdc, l, t);
            SelectObject(hdc, old);
            let _ = DeleteObject(pen.into());
        }
    }
}
