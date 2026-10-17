//! The text around the caret, for the macOS text input system.
//!
//! The press-and-hold accent menu, the Character Viewer, dictation and input methods read and edit
//! the text of the focused view through `NSTextInputClient`. Winit does not own that text, so the
//! application pushes a copy of it with [`set_ime_surrounding_text`] and the view answers
//! `selectedRange`, `markedRange` and `attributedSubstringForProposedRange:actualRange:` from the
//! copy. Winit 0.30 had none: it reported no selection (`NSNotFound`), which makes the system
//! drop dictated text (rust-windowing/winit#4666), it ignored the replacement range of
//! `insertText:replacementRange:` (#3617), and it dropped a commit that came without marked text
//! (#3925), which is how the Character Viewer and dictation insert.
//!
//! The accent menu is why the replacement range matters: choosing an accented letter does not
//! insert it at the caret. The system replaces the base letter typed first, by calling
//! `insertText:replacementRange:` with the range of that letter. WebKit's test support describes
//! it as "holding down a vowel key (e.g. 'a') to bring up the menu containing accented version of
//! the character, then selecting an accented character to insert in place of the typed character"
//! (WebKit r208143, <https://trac.webkit.org/changeset/208143/webkit>).
//!
//! The marked text of a composition is not part of the copy: it overlays the caret and takes no
//! room in it. The ranges the system names then count in the copy, so only those that end at or
//! before the caret keep their meaning, see [`replacement_range`].
//!
//! The system counts in UTF-16 code units (`NSRange`), the application in UTF-8 bytes; the helpers
//! here convert between the two. The state is thread-local, because the view and the
//! application's UI both live on the main thread.
//!
//! [`set_ime_surrounding_text`]: crate::platform::macos::set_ime_surrounding_text

use std::cell::RefCell;
use std::ops::Range;

use objc2_foundation::NSRange;

use crate::platform::macos::ImeReplacement;

/// The text of the focused field and its selection.
struct Surrounding {
    /// The whole text.
    text: String,
    /// The selection in bytes of `text`, on char boundaries; a caret when empty.
    selection: Range<usize>,
}

struct State {
    surrounding: Option<Surrounding>,
    /// The text a composition in progress stands in for: the replacement range of the
    /// `setMarkedText:selectedRange:replacementRange:` that started it, in bytes of the
    /// surrounding text.
    composing: Option<Range<usize>>,
    /// The replacement that belongs to the last commit, until the application takes it.
    pending: Option<ImeReplacement>,
}

impl State {
    const fn new() -> Self {
        Self { surrounding: None, composing: None, pending: None }
    }
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State::new()) };
}

/// Runs `f` on the state. `None` when the state is not reachable: while the thread shuts down.
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE
        .try_with(|state| state.try_borrow_mut().ok().map(|mut state| f(&mut state)))
        .ok()
        .flatten()
}

/// Runs `f` on the surrounding text. `None` when none is set.
fn read<R>(f: impl FnOnce(&Surrounding) -> Option<R>) -> Option<R> {
    with_state(|state| state.surrounding.as_ref().and_then(f)).flatten()
}

/// Sets the surrounding text. `selection` is clamped to `text`, onto char boundaries.
pub(super) fn set(text: &str, selection: Range<usize>) {
    let start = floor_char_boundary(text, selection.start.min(selection.end));
    let end = floor_char_boundary(text, selection.start.max(selection.end));
    with_state(|state| {
        let surrounding = state
            .surrounding
            .get_or_insert_with(|| Surrounding { text: String::new(), selection: 0..0 });
        surrounding.text.clear();
        surrounding.text.push_str(text);
        surrounding.selection = start..end;
    });
}

/// Forgets the surrounding text, the composition's replacement range and the pending replacement.
pub(super) fn clear() {
    with_state(|state| *state = State::new());
}

/// Takes the replacement of the last commit.
pub(super) fn take_replacement() -> Option<ImeReplacement> {
    with_state(|state| state.pending.take()).flatten()
}

/// `selectedRange`: the selection in UTF-16. `None` when no surrounding text is set.
pub(super) fn selected_range() -> Option<NSRange> {
    read(|surrounding| {
        let start = utf8_to_utf16(&surrounding.text, surrounding.selection.start);
        let end = utf8_to_utf16(&surrounding.text, surrounding.selection.end);
        Some(NSRange::new(start, end.saturating_sub(start)))
    })
}

/// Where `markedRange` starts: the marked text sits at the caret. `None` when no surrounding text
/// is set.
pub(super) fn marked_location() -> Option<usize> {
    read(|surrounding| Some(utf8_to_utf16(&surrounding.text, surrounding.selection.start)))
}

/// `attributedSubstringForProposedRange:actualRange:`: the text of `proposed` (UTF-16) and the
/// range it covers.
///
/// A range that reaches past the text is clamped to it; one that starts behind the end is out of
/// bounds (`None`), as is every range when no surrounding text is set. An end inside a surrogate
/// pair moves outward to the pair's edge, so no character is cut in two.
pub(super) fn substring(proposed: NSRange) -> Option<(String, NSRange)> {
    read(|surrounding| {
        let text = &surrounding.text;
        let total = utf16_len(text);
        if proposed.location > total {
            return None;
        }
        let end = proposed.location.saturating_add(proposed.length).min(total);

        // Scan for the last char boundary at or before the start, and the first one at or after
        // the end (both as byte and UTF-16 offset). The scan stops at the end.
        let mut start = (0, 0);
        let mut stop = None;
        let mut utf16 = 0;
        for (byte, ch) in text.char_indices() {
            if utf16 <= proposed.location {
                start = (byte, utf16);
            }
            if utf16 >= end {
                stop = Some((byte, utf16));
                break;
            }
            utf16 += ch.len_utf16();
        }
        // The text's end is a boundary too, though no char starts there.
        if stop.is_none() && total <= proposed.location {
            start = (text.len(), total);
        }
        let stop = stop.unwrap_or((text.len(), total));

        let range = NSRange::new(start.1, stop.1 - start.1);
        Some((text[start.0..stop.0].to_owned(), range))
    })
}

/// The bytes of the surrounding text that `range` (UTF-16, as sent by the system) names, when the
/// application can replace them. `None` when there is no such text, which includes the
/// `{NSNotFound, 0}` of "no range given".
///
/// While a composition is in progress (`composing`), its marked text is not part of the
/// surrounding text: it only overlays the caret. A range that touches the caret or lies behind it
/// may mean the marked text itself (the system is free to name it), and replacing text there
/// would delete characters the user never marked. Only a range that ends at or before the caret
/// keeps its meaning.
pub(super) fn replacement_range(range: NSRange, composing: bool) -> Option<Range<usize>> {
    read(|surrounding| {
        let range = utf16_range_to_utf8(&surrounding.text, range)?;
        (!composing || range.end <= surrounding.selection.start).then_some(range)
    })
}

/// A composition starts with a `setMarkedText:selectedRange:replacementRange:` that stands in for
/// `range`, if the system named one the application can replace.
pub(super) fn start_composition(range: Option<Range<usize>>) {
    with_state(|state| state.composing = range);
}

/// The composition ended without a commit.
pub(super) fn cancel_composition() {
    with_state(|state| state.composing = None);
}

/// A commit of `text`. It replaces `range` if the system named one, otherwise the range the
/// composition stood in for, otherwise nothing: the application inserts at the caret.
///
/// Every commit sets the pending replacement, so one never outlives its commit.
pub(super) fn commit(range: Option<Range<usize>>, text: &str) {
    with_state(|state| {
        let composing = state.composing.take();
        state.pending =
            range.or(composing).map(|range| ImeReplacement { range, text: text.to_owned() });
    });
}

/// The UTF-16 length of `s`.
fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `byte` as a UTF-16 offset into `s`. An offset inside a char rounds down to its start, one
/// behind the end is the end.
pub(super) fn utf8_to_utf16(s: &str, byte: usize) -> usize {
    utf16_len(&s[..floor_char_boundary(s, byte)])
}

/// `range` (UTF-16) as a byte range of `s`. `None` when it reaches past the end of `s`, or when an
/// end falls inside a surrogate pair.
pub(super) fn utf16_range_to_utf8(s: &str, range: NSRange) -> Option<Range<usize>> {
    let end = range.location.checked_add(range.length)?;
    // A char takes at least as many bytes as UTF-16 units, so an end behind the byte length is
    // out of bounds. This also turns `NSNotFound` down without scanning.
    if end > s.len() {
        return None;
    }
    Some(byte_at(s, range.location)?..byte_at(s, end)?)
}

/// The byte offset of the UTF-16 offset `at` in `s`; `None` when `at` is behind the end or inside
/// a surrogate pair.
fn byte_at(s: &str, at: usize) -> Option<usize> {
    let mut utf16 = 0;
    for (byte, ch) in s.char_indices() {
        if utf16 >= at {
            return (utf16 == at).then_some(byte);
        }
        utf16 += ch.len_utf16();
    }
    (utf16 == at).then_some(s.len())
}

/// The largest char boundary of `s` at or before `byte`. `str::floor_char_boundary` is unstable.
fn floor_char_boundary(s: &str, mut byte: usize) -> usize {
    if byte >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(location: usize, length: usize) -> NSRange {
        NSRange::new(location, length)
    }

    fn replacement(range: Range<usize>, text: &str) -> ImeReplacement {
        ImeReplacement { range, text: text.to_owned() }
    }

    #[test]
    fn ascii_offsets_are_the_same_in_both_encodings() {
        assert_eq!(utf8_to_utf16("hello", 0), 0);
        assert_eq!(utf8_to_utf16("hello", 3), 3);
        assert_eq!(utf8_to_utf16("hello", 5), 5);
        assert_eq!(utf16_range_to_utf8("hello", range(1, 3)), Some(1..4));
        assert_eq!(utf16_range_to_utf8("hello", range(5, 0)), Some(5..5));
        assert_eq!(utf16_range_to_utf8("", range(0, 0)), Some(0..0));
    }

    #[test]
    fn an_umlaut_is_one_unit_in_utf16_and_two_bytes_in_utf8() {
        // "bär": b = 1 byte, ä = 2 bytes, r = 1 byte.
        assert_eq!(utf8_to_utf16("bär", 1), 1);
        assert_eq!(utf8_to_utf16("bär", 3), 2);
        assert_eq!(utf8_to_utf16("bär", 4), 3);
        assert_eq!(utf16_range_to_utf8("bär", range(1, 1)), Some(1..3));
        assert_eq!(utf16_range_to_utf8("bär", range(0, 3)), Some(0..4));
        assert_eq!(utf16_range_to_utf8("bär", range(2, 1)), Some(3..4));
    }

    #[test]
    fn an_emoji_is_two_units_in_utf16_and_four_bytes_in_utf8() {
        // "a😀b": a = 1 byte, 😀 = 4 bytes and 2 units (a surrogate pair), b = 1 byte.
        assert_eq!(utf8_to_utf16("a😀b", 1), 1);
        assert_eq!(utf8_to_utf16("a😀b", 5), 3);
        assert_eq!(utf8_to_utf16("a😀b", 6), 4);
        assert_eq!(utf16_range_to_utf8("a😀b", range(1, 2)), Some(1..5));
        assert_eq!(utf16_range_to_utf8("a😀b", range(3, 1)), Some(5..6));
        assert_eq!(utf16_range_to_utf8("a😀b", range(0, 4)), Some(0..6));
        assert_eq!(utf16_range_to_utf8("a😀b", range(4, 0)), Some(6..6));
    }

    #[test]
    fn a_range_that_splits_a_surrogate_pair_is_refused() {
        // Ends the range inside the emoji, starts inside it, or holds only half of it.
        assert_eq!(utf16_range_to_utf8("a😀b", range(0, 2)), None);
        assert_eq!(utf16_range_to_utf8("a😀b", range(2, 2)), None);
        assert_eq!(utf16_range_to_utf8("a😀b", range(1, 1)), None);
        assert_eq!(utf16_range_to_utf8("a😀b", range(2, 0)), None);
    }

    #[test]
    fn a_range_outside_the_text_is_refused() {
        assert_eq!(utf16_range_to_utf8("abc", range(0, 4)), None);
        assert_eq!(utf16_range_to_utf8("abc", range(4, 0)), None);
        assert_eq!(utf16_range_to_utf8("", range(0, 1)), None);
        // What the system sends for "no range": `{NSNotFound, 0}`.
        assert_eq!(utf16_range_to_utf8("abc", range(isize::MAX as usize, 0)), None);
        // An end that does not fit a `usize`.
        assert_eq!(utf16_range_to_utf8("abc", range(1, usize::MAX)), None);
    }

    #[test]
    fn a_byte_offset_inside_a_char_rounds_down() {
        assert_eq!(utf8_to_utf16("bär", 2), 1);
        assert_eq!(utf8_to_utf16("a😀b", 3), 1);
        assert_eq!(utf8_to_utf16("a😀b", 4), 1);
        // Behind the end is the end.
        assert_eq!(utf8_to_utf16("bär", 100), 3);
    }

    #[test]
    fn the_selection_is_clamped_onto_char_boundaries() {
        clear();
        // Inside the ä, inside the emoji, and the ends the wrong way round.
        set("bär", 2..9);
        assert_eq!(read(|s| Some(s.selection.clone())), Some(1..4));
        set("a😀b", Range { start: 4, end: 2 });
        assert_eq!(read(|s| Some(s.selection.clone())), Some(1..1));
        set("abc", 7..9);
        assert_eq!(read(|s| Some(s.selection.clone())), Some(3..3));
    }

    #[test]
    fn the_selection_is_reported_in_utf16() {
        clear();
        assert_eq!(selected_range(), None);
        assert_eq!(marked_location(), None);

        // The caret behind "a😀": 5 bytes, 3 units.
        set("a😀b", 5..5);
        assert_eq!(selected_range(), Some(range(3, 0)));
        assert_eq!(marked_location(), Some(3));

        // "😀b" selected.
        set("a😀b", 1..6);
        assert_eq!(selected_range(), Some(range(1, 3)));
        assert_eq!(marked_location(), Some(1));
    }

    #[test]
    fn setting_the_text_again_replaces_it() {
        clear();
        set("one two three", 13..13);
        set("one", 1..1);
        assert_eq!(selected_range(), Some(range(1, 0)));
        assert_eq!(substring(range(0, 100)), Some(("one".to_owned(), range(0, 3))));
    }

    #[test]
    fn clear_forgets_everything() {
        set("abc", 1..1);
        start_composition(Some(0..1));
        commit(Some(0..1), "x");
        clear();
        assert_eq!(selected_range(), None);
        assert_eq!(substring(range(0, 1)), None);
        assert_eq!(take_replacement(), None);
        // The composition's range is gone too: this commit has none to take.
        commit(None, "y");
        assert_eq!(take_replacement(), None);
    }

    #[test]
    fn substring_is_the_text_of_the_range() {
        clear();
        set("a😀bär", 0..0);
        assert_eq!(substring(range(0, 1)), Some(("a".to_owned(), range(0, 1))));
        assert_eq!(substring(range(1, 2)), Some(("😀".to_owned(), range(1, 2))));
        assert_eq!(substring(range(3, 2)), Some(("bä".to_owned(), range(3, 2))));
        assert_eq!(substring(range(0, 6)), Some(("a😀bär".to_owned(), range(0, 6))));
        // An empty range, also at the end.
        assert_eq!(substring(range(3, 0)), Some((String::new(), range(3, 0))));
        assert_eq!(substring(range(6, 0)), Some((String::new(), range(6, 0))));
    }

    #[test]
    fn substring_clamps_a_range_that_reaches_past_the_text() {
        clear();
        set("abc", 0..0);
        assert_eq!(substring(range(1, 100)), Some(("bc".to_owned(), range(1, 2))));
        assert_eq!(substring(range(3, 5)), Some((String::new(), range(3, 0))));
        assert_eq!(substring(range(1, usize::MAX)), Some(("bc".to_owned(), range(1, 2))));
        // Starting behind the text is out of bounds, and `{NSNotFound, 0}` is not a range.
        assert_eq!(substring(range(4, 0)), None);
        assert_eq!(substring(range(isize::MAX as usize, 0)), None);
    }

    #[test]
    fn substring_never_cuts_a_char_in_two() {
        clear();
        set("a😀b", 0..0);
        // Half an emoji grows to the whole one, whichever half it is.
        assert_eq!(substring(range(1, 1)), Some(("😀".to_owned(), range(1, 2))));
        assert_eq!(substring(range(2, 1)), Some(("😀".to_owned(), range(1, 2))));
        assert_eq!(substring(range(0, 2)), Some(("a😀".to_owned(), range(0, 3))));
        assert_eq!(substring(range(2, 2)), Some(("😀b".to_owned(), range(1, 3))));
        assert_eq!(substring(range(2, 0)), Some(("😀".to_owned(), range(1, 2))));
    }

    #[test]
    fn the_character_before_the_caret_is_what_the_accent_menu_asks_for() {
        clear();
        // "ha" with the caret behind the a: the system asks for the unit before it.
        set("ha", 2..2);
        let caret = selected_range().unwrap();
        assert_eq!(substring(range(caret.location - 1, 1)), Some(("a".to_owned(), range(1, 1))));
        // And replaces exactly that letter.
        assert_eq!(replacement_range(range(caret.location - 1, 1), false), Some(1..2));
    }

    #[test]
    fn a_replacement_range_must_be_one_the_application_can_apply() {
        clear();
        // Nothing to apply it to.
        assert_eq!(replacement_range(range(0, 1), false), None);

        set("a😀b", 6..6);
        assert_eq!(replacement_range(range(0, 1), false), Some(0..1));
        assert_eq!(replacement_range(range(1, 2), false), Some(1..5));
        assert_eq!(replacement_range(range(4, 0), false), Some(6..6));
        // Out of bounds, a split emoji, and "no range".
        assert_eq!(replacement_range(range(3, 2), false), None);
        assert_eq!(replacement_range(range(2, 1), false), None);
        assert_eq!(replacement_range(range(isize::MAX as usize, 0), false), None);
    }

    #[test]
    fn during_a_composition_only_text_before_the_caret_can_be_replaced() {
        clear();
        // "abc|def": the marked text overlays the caret.
        set("abcdef", 3..3);
        assert_eq!(replacement_range(range(1, 2), true), Some(1..3));
        assert_eq!(replacement_range(range(0, 3), true), Some(0..3));
        // Crossing the caret, or behind it, may be the marked text: not ours to replace.
        assert_eq!(replacement_range(range(2, 2), true), None);
        assert_eq!(replacement_range(range(3, 3), true), None);
        assert_eq!(replacement_range(range(4, 1), true), None);
        // An empty range at the caret is the caret.
        assert_eq!(replacement_range(range(3, 0), true), Some(3..3));
        // The same ranges are fine without a composition.
        assert_eq!(replacement_range(range(3, 3), false), Some(3..6));
    }

    #[test]
    fn a_commit_replaces_the_range_it_names() {
        clear();
        commit(Some(1..2), "á");
        assert_eq!(take_replacement(), Some(replacement(1..2, "á")));
        // Taken once.
        assert_eq!(take_replacement(), None);
    }

    #[test]
    fn a_commit_without_a_range_is_an_insertion() {
        clear();
        commit(None, "x");
        assert_eq!(take_replacement(), None);
    }

    #[test]
    fn a_commit_replaces_what_its_composition_stood_in_for() {
        clear();
        start_composition(Some(4..9));
        commit(None, "日本");
        assert_eq!(take_replacement(), Some(replacement(4..9, "日本")));
        // The composition is over: the next commit does not inherit its range.
        commit(None, "x");
        assert_eq!(take_replacement(), None);
    }

    #[test]
    fn a_commit_names_its_own_range_before_the_one_of_the_composition() {
        clear();
        start_composition(Some(4..9));
        commit(Some(0..1), "x");
        assert_eq!(take_replacement(), Some(replacement(0..1, "x")));
        commit(None, "y");
        assert_eq!(take_replacement(), None);
    }

    #[test]
    fn a_cancelled_composition_leaves_no_range_behind() {
        clear();
        start_composition(Some(4..9));
        cancel_composition();
        commit(None, "x");
        assert_eq!(take_replacement(), None);
    }

    #[test]
    fn a_commit_does_not_leave_a_stale_replacement() {
        clear();
        commit(Some(1..2), "á");
        // The application never took it; the next commit overwrites it.
        commit(None, "x");
        assert_eq!(take_replacement(), None);
    }
}
