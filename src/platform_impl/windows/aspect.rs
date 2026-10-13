//! Aspect ratio lock for interactive resizes.
//!
//! While the user drags a window's edge or corner, Windows sends `WM_SIZING` with the proposed
//! window rectangle, which the window may change. The lock changes it to the requested ratio,
//! following the side the user drags.
//!
//! The ratio is registered per `HWND`, the value the raw window handle carries, so a toolkit
//! that hands out only raw handles can still set it.

use std::sync::Mutex;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{WMSZ_BOTTOM, WMSZ_LEFT, WMSZ_RIGHT, WMSZ_TOP};

use crate::dpi::PhysicalSize;

/// Locked windows and their ratio (width over height).
static RATIOS: Mutex<Vec<(HWND, f64)>> = Mutex::new(Vec::new());

/// Locks the window `hwnd` to `ratio`, or unlocks it with `None` or a ratio that is not a
/// finite positive number.
pub fn set(hwnd: HWND, ratio: Option<f64>) {
    let mut ratios = RATIOS.lock().unwrap();
    ratios.retain(|(window, _)| *window != hwnd);
    if let Some(ratio) = ratio.filter(|r| r.is_finite() && *r > 0.0) {
        ratios.push((hwnd, ratio));
    }
}

/// The ratio window `hwnd` is locked to.
pub fn get(hwnd: HWND) -> Option<f64> {
    RATIOS.lock().unwrap().iter().find(|(window, _)| *window == hwnd).map(|(_, ratio)| *ratio)
}

/// The client size in `ratio` for a resize from `side` (a `WMSZ_*` value) to `width`×`height`:
/// a left or right edge sets the width, a top or bottom edge the height, a corner (or an unknown
/// side) follows the pointer on the side it is further out. Grown in the ratio to `min`, then
/// shrunk in the ratio to `max`; whole pixels, never under 1.
pub fn lock(
    width: i32,
    height: i32,
    side: u32,
    ratio: f64,
    min: Option<PhysicalSize<i32>>,
    max: Option<PhysicalSize<i32>>,
) -> (i32, i32) {
    let (w, h) = (f64::from(width), f64::from(height));
    let by_width = match side {
        WMSZ_LEFT | WMSZ_RIGHT => true,
        WMSZ_TOP | WMSZ_BOTTOM => false,
        _ => w / ratio >= h,
    };
    let (mut w, mut h) = if by_width { (w, w / ratio) } else { (h * ratio, h) };
    if let Some(min) = min {
        let grow = (f64::from(min.width) / w).max(f64::from(min.height) / h);
        if grow > 1.0 {
            w *= grow;
            h *= grow;
        }
    }
    if let Some(max) = max {
        let shrink = (f64::from(max.width) / w).min(f64::from(max.height) / h);
        if shrink < 1.0 {
            w *= shrink;
            h *= shrink;
        }
    }
    ((w.round() as i32).max(1), (h.round() as i32).max(1))
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WMSZ_BOTTOMRIGHT, WMSZ_TOPLEFT};

    use super::*;

    const WIDE: f64 = 16.0 / 9.0;

    #[test]
    fn an_edge_sets_its_dimension() {
        assert_eq!(lock(800, 360, WMSZ_RIGHT, WIDE, None, None), (800, 450));
        assert_eq!(lock(640, 450, WMSZ_BOTTOM, WIDE, None, None), (800, 450));
    }

    #[test]
    fn a_corner_follows_the_side_further_out() {
        assert_eq!(lock(700, 420, WMSZ_BOTTOMRIGHT, WIDE, None, None), (747, 420));
        assert_eq!(lock(800, 300, WMSZ_TOPLEFT, WIDE, None, None), (800, 450));
    }

    #[test]
    fn the_minimum_grows_in_the_ratio() {
        let min = Some(PhysicalSize::new(192, 108));
        assert_eq!(lock(40, 360, WMSZ_LEFT, WIDE, min, None), (192, 108));
    }

    #[test]
    fn the_maximum_shrinks_in_the_ratio() {
        let max = Some(PhysicalSize::new(1000, 1000));
        assert_eq!(lock(2000, 360, WMSZ_RIGHT, 2.0, None, max), (1000, 500));
    }

    #[test]
    fn the_registry_keeps_the_last_ratio_and_unlocks() {
        let hwnd = 0xdead_beef as HWND;
        set(hwnd, Some(2.0));
        set(hwnd, Some(1.5));
        assert_eq!(get(hwnd), Some(1.5));
        set(hwnd, Some(f64::NAN));
        assert_eq!(get(hwnd), None);
    }
}
