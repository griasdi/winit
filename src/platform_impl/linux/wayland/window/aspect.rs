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

use sctk::reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge as XdgResizeEdge;

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

/// Which side of the window a resize sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead {
    /// The left or right edge: the width.
    Width,
    /// The top or bottom edge: the height.
    Height,
    /// A corner: the side the pointer is further out on, so the corner follows the pointer
    /// on that side.
    Corner,
    /// A resize the compositor started (no edge known): the side that changed more since its
    /// previous request.
    Unknown,
}

impl Lead {
    pub fn of(edge: XdgResizeEdge) -> Self {
        match edge {
            XdgResizeEdge::Left | XdgResizeEdge::Right => Self::Width,
            XdgResizeEdge::Top | XdgResizeEdge::Bottom => Self::Height,
            XdgResizeEdge::TopLeft
            | XdgResizeEdge::TopRight
            | XdgResizeEdge::BottomLeft
            | XdgResizeEdge::BottomRight => Self::Corner,
            _ => Self::Unknown,
        }
    }
}

/// `requested` in `ratio`, with the side `lead` names taken from it; `previous` is the
/// compositor's previous request in this resize (or the window's size before the first). Both
/// are compositor sizes, never a locked one: the compositor keeps sizing from its own
/// geometry, so comparing with a locked size would flip the leading side from one configure
/// to the next. The result keeps `ratio` within `min` and `max` where both allow it; whole
/// logical pixels, never zero.
pub fn lock(
    requested: LogicalSize<u32>,
    previous: LogicalSize<u32>,
    lead: Lead,
    ratio: f64,
    min: LogicalSize<u32>,
    max: Option<LogicalSize<u32>>,
) -> LogicalSize<u32> {
    let rw = f64::from(requested.width);
    let rh = f64::from(requested.height);
    let by_width = match lead {
        Lead::Width => true,
        Lead::Height => false,
        Lead::Corner => rw / ratio >= rh,
        Lead::Unknown => {
            requested.width.abs_diff(previous.width) >= requested.height.abs_diff(previous.height)
        },
    };
    let (mut w, mut h) = if by_width { (rw, rw / ratio) } else { (rh * ratio, rh) };
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

    const WIDE: f64 = 16.0 / 9.0;

    #[test]
    fn a_side_edge_sets_the_width() {
        // The compositor keeps its starting height while the locked height grew.
        let got = lock(size(820, 360), size(800, 360), Lead::Width, WIDE, MIN, None);
        assert_eq!(got, size(820, 461));
    }

    #[test]
    fn a_top_or_bottom_edge_sets_the_height() {
        let got = lock(size(640, 450), size(640, 440), Lead::Height, WIDE, MIN, None);
        assert_eq!(got, size(800, 450));
    }

    #[test]
    fn a_corner_follows_the_pointer_on_the_side_it_is_further_out() {
        let got = lock(size(700, 380), size(690, 370), Lead::Corner, WIDE, MIN, None);
        assert_eq!(got, size(700, 394));
        let got = lock(size(660, 420), size(650, 400), Lead::Corner, WIDE, MIN, None);
        assert_eq!(got, size(747, 420));
    }

    #[test]
    fn a_resize_the_compositor_started_follows_its_own_requests() {
        // Against the locked 800x450 the height would seem to change more and the window
        // would jump back to 640x360 (the flicker this replaced).
        let got = lock(size(820, 360), size(800, 360), Lead::Unknown, WIDE, MIN, None);
        assert_eq!(got, size(820, 461));
    }

    #[test]
    fn the_min_size_grows_it_in_the_ratio() {
        let min = size(192, 108);
        let got = lock(size(100, 108), size(110, 108), Lead::Width, WIDE, min, None);
        assert_eq!(got, size(192, 108));
    }

    #[test]
    fn the_max_size_shrinks_it_in_the_ratio() {
        let max = Some(size(1000, 1000));
        let got = lock(size(2000, 360), size(1900, 360), Lead::Width, 2.0, MIN, max);
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
