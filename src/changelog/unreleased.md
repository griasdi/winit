The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

The sections should follow the order `Added`, `Changed`, `Deprecated`,
`Removed`, and `Fixed`.

Platform specific changed should be added to the end of the section and grouped
by platform name. Common API additions should have `, implemented` at the end
for platforms where the API was initially implemented. See the following example
on how to add them:

```md
### Added

- Add `Window::turbo()`, implemented on X11, Wayland, and Web.
- On X11, add `Window::some_rare_api`.
- On X11, add `Window::even_more_rare_api`.
- On Wayland, add `Window::common_api`.
- On Windows, add `Window::some_rare_api`.
```

When the change requires non-trivial amount of work for users to comply
with it, the migration guide should be added below the entry, like:

```md
- Deprecate `Window` creation outside of `EventLoop::run`

  This was done to simply migration in the future. Consider the
  following code:

  // Code snippet.

  To migrate it we should do X, Y, and then Z, for example:

  // Code snippet.

```

The migration guide could reference other migration examples in the current
changelog entry.

## Unreleased

### Fixed

- On Windows, fix a transparent window turning opaque outside its drawn content after leaving
  fullscreen.
- On Windows, fix a window that redraws every frame keeping the other windows from redrawing.
- On Windows, fix a minimized window coming back only a title bar high after its minimum or
  maximum size was set while it was minimized.
- On Windows, fix the transparency mask of icons created from RGBA data. Places that draw icons
  through the mask, such as Task Manager, showed a background and rough edges.
- On Windows, fix window icons rendering with red and blue swapped when the same `Icon` is
  applied more than once. The RGBA to BGRA conversion no longer mutates the shared pixel buffer.
- On Windows, fix IME preedit cursor offsets around surrogate pairs when no target clause is
  selected.
- On Windows, avoid overflow when negating minimum mouse-wheel deltas.
- On Windows, fix a crash occurring when trying to create a DXGI surface on a window that is both
  fullscreen and always on top.
- On Windows, fix a deadlock when the taskbar is recreated while another thread holds the
  window's state lock.
- On Windows, fix getting the window's DPI internally leaking `HDC` handles. `GetDC` is now only
  called on versions before Windows 8.1, which also improves performance.
- On Windows, fix a freeze that occurs when the keyboard layout is switched by tools such as
  Punto Switcher. The `WM_INPUTLANGCHANGE` message now refreshes the cached keyboard layout,
  while still deferring to `DefWindowProc` for normal propagation.
- On Wayland, fix the text input being destroyed whenever a seat capability changed. This made IME
  input stop working in already-running applications after the seat's capabilities were updated.
- On Wayland, use the touch serial for `Window::drag_window`, `Window::drag_resize_window` and
  `Window::show_window_menu` when the interaction was started by touch.
- On Wayland, fix a protocol error when setting a custom cursor on compositors with `wl_surface`
  version below 3.
- On macOS, fix a panic and incorrect cursor position in `Ime::Preedit` when the preedit string
  contains special characters (e.g. emojis) caused by incorrect UTF-16 to UTF-8 offset conversion.
- On macOS, report the `NSUnderlineStyle` and `NSMarkedClauseSegment` attributes as valid for
  marked text, so input methods can style the clauses of a preedit.
- On macOS, discard the input method's marked text when IME is disabled, so an unfinished
  composition is not resumed (and committed again) when IME is re-enabled.
