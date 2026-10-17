#[macro_use]
mod util;

mod app;
mod app_state;
mod cursor;
mod event;
mod event_handler;
mod event_loop;
mod ffi;
mod menu;
mod monitor;
mod observer;
mod surrounding;
mod view;
mod window;
mod window_delegate;

use std::fmt;
use std::ops::Range;

use objc2_app_kit::NSApplication;
use objc2_foundation::MainThreadMarker;

pub(crate) use self::event::{physicalkey_to_scancode, scancode_to_physicalkey, KeyEventExtra};
pub(crate) use self::event_loop::{
    ActiveEventLoop, EventLoop, EventLoopProxy, OwnedDisplayHandle,
    PlatformSpecificEventLoopAttributes,
};
pub(crate) use self::monitor::{MonitorHandle, VideoModeHandle};
pub(crate) use self::window::WindowId;
pub(crate) use self::window_delegate::PlatformSpecificWindowAttributes;
use crate::event::DeviceId as RootDeviceId;

pub(crate) use self::cursor::CustomCursor as PlatformCustomCursor;
pub(crate) use self::window::Window;
pub(crate) use crate::cursor::OnlyCursorImageSource as PlatformCustomCursorSource;
pub(crate) use crate::icon::NoIcon as PlatformIcon;
pub(crate) use crate::platform_impl::Fullscreen;

/// See [`crate::platform::macos::set_ime_surrounding_text`].
pub fn set_ime_surrounding_text(text: &str, selection: Range<usize>) {
    if MainThreadMarker::new().is_some() {
        surrounding::set(text, selection);
    }
}

/// See [`crate::platform::macos::clear_ime_surrounding_text`].
pub fn clear_ime_surrounding_text() {
    if MainThreadMarker::new().is_some() {
        surrounding::clear();
    }
}

/// See [`crate::platform::macos::take_ime_replacement`].
pub fn take_ime_replacement() -> Option<crate::platform::macos::ImeReplacement> {
    MainThreadMarker::new().and_then(|_| surrounding::take_replacement())
}

/// See [`crate::platform::macos::order_front_character_palette`].
pub fn order_front_character_palette() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).orderFrontCharacterPalette(None);
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId;

impl DeviceId {
    pub const fn dummy() -> Self {
        DeviceId
    }
}

// Constant device ID; to be removed when if backend is updated to report real device IDs.
pub(crate) const DEVICE_ID: RootDeviceId = RootDeviceId(DeviceId);

#[derive(Debug)]
pub enum OsError {
    CGError(core_graphics::base::CGError),
    CreationError(&'static str),
}

impl fmt::Display for OsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OsError::CGError(e) => f.pad(&format!("CGError {e}")),
            OsError::CreationError(e) => f.pad(e),
        }
    }
}
