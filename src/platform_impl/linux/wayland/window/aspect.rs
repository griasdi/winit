//! Aspect ratio lock for interactive resizes.
//!
//! `xdg_toplevel` has no aspect ratio hint. While the `resizing` state is set, the size in a
//! configure is a hint the client may refine: "Clients that have aspect ratio or cell sizing
//! configuration can use a smaller size" (xdg-shell, `xdg_toplevel.state.resizing`). The lock
//! picks the size in the requested ratio that follows the side the user drags.
//!
//! The ratio is registered per window id (the `wl_surface` pointer, the same value the raw
//! window handle carries), so a toolkit that hands out only raw handles can still set it.

use std::sync::Mutex;

use crate::dpi::LogicalSize;

use super::WindowId;

/// Locked windows and their ratio (width over height).
static RATIOS: Mutex<Vec<(WindowId, f64)>> = Mutex::new(Vec::new());

/// Locks the window `id` to `ratio`, or unlocks it with `None` or a ratio that is not a finite
/// positive number.
pub fn set(id: WindowId, ratio: Option<f64>) {
    let mut ratios = RATIOS.lock().unwrap();
    ratios.retain(|(window, _)| *window != id);
    if let Some(ratio) = ratio.filter(|r| r.is_finite() && *r > 0.0) {
        ratios.push((id, ratio));
    }
}

/// The ratio window `id` is locked to.
pub fn get(id: WindowId) -> Option<f64> {
    RATIOS.lock().unwrap().iter().find(|(window, _)| *window == id).map(|(_, ratio)| *ratio)
}

/// `requested` in `ratio`, following the side that changed more against `current`: a drag on
/// the left or right edge sets the width, one on the top or bottom edge the height, a corner
/// the side it moved further. The result keeps `ratio` within `min` and `max` where both
/// allow it; whole logical pixels, never zero.
pub fn lock(
    requested: LogicalSize<u32>,
    current: LogicalSize<u32>,
    ratio: f64,
    min: LogicalSize<u32>,
    max: Option<LogicalSize<u32>>,
) -> LogicalSize<u32> {
    let dw = requested.width.abs_diff(current.width);
    let dh = requested.height.abs_diff(current.height);
    let (mut w, mut h) = if dw >= dh {
        let w = f64::from(requested.width);
        (w, w / ratio)
    } else {
        let h = f64::from(requested.height);
        (h * ratio, h)
    };
    let grow = (f64::from(min.width) / w).max(f64::from(min.height) / h);
    if grow > 1.0 {
        w *= grow;
        h *= grow;
    }
    if let Some(max) = max {
        let shrink = (f64::from(max.width) / w).min(f64::from(max.height) / h);
        if shrink < 1.0 {
            w *= shrink;
            h *= shrink;
        }
    }
    let side = |v: f64| (v.round() as u32).max(1);
    LogicalSize::new(side(w), side(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: LogicalSize<u32> = LogicalSize::new(2, 1);

    fn size(width: u32, height: u32) -> LogicalSize<u32> {
        LogicalSize::new(width, height)
    }

    #[test]
    fn a_side_edge_sets_the_width() {
        let got = lock(size(800, 360), size(640, 360), 16.0 / 9.0, MIN, None);
        assert_eq!(got, size(800, 450));
    }

    #[test]
    fn a_top_or_bottom_edge_sets_the_height() {
        let got = lock(size(640, 450), size(640, 360), 16.0 / 9.0, MIN, None);
        assert_eq!(got, size(800, 450));
    }

    #[test]
    fn a_corner_follows_the_side_it_moved_further() {
        let got = lock(size(700, 380), size(640, 360), 16.0 / 9.0, MIN, None);
        assert_eq!(got, size(700, 394));
        let got = lock(size(660, 420), size(640, 360), 16.0 / 9.0, MIN, None);
        assert_eq!(got, size(747, 420));
    }

    #[test]
    fn the_min_size_grows_it_in_the_ratio() {
        let got = lock(size(100, 108), size(640, 360), 16.0 / 9.0, size(192, 108), None);
        assert_eq!(got, size(192, 108));
    }

    #[test]
    fn the_max_size_shrinks_it_in_the_ratio() {
        let got = lock(size(2000, 360), size(640, 360), 2.0, MIN, Some(size(1000, 1000)));
        assert_eq!(got, size(1000, 500));
    }

    #[test]
    fn the_registry_keeps_one_ratio_per_window() {
        let id = WindowId(0xdead_beef);
        set(id, Some(2.0));
        set(id, Some(1.5));
        assert_eq!(get(id), Some(1.5));
        set(id, Some(f64::NAN));
        assert_eq!(get(id), None);
    }
}
