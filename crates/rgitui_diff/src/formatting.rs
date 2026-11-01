//! Render-only tab expansion. Keep `StyledLine` and all diff/staging data raw.
//!
//! GPUI's `StyledText` does not implement editor tab stops. Expand tabs just
//! before shaping text, and use this same mapping for syntax/selection ranges
//! and hit testing. All offsets are UTF-8 byte offsets, not character indices.

use std::{borrow::Cow, ops::Range};
use unicode_width::UnicodeWidthChar;

/// A compact, conventional code-view tab width. Stops start at the code column,
/// not the diff marker or line-number gutter.
pub(crate) const TAB_WIDTH: usize = 4;

/// Remove one line terminator for a single diff row, never source whitespace.
/// Unlike `trim_end`, this preserves trailing spaces/tabs and whitespace-only
/// lines. Keep the original `DiffLine` unchanged for patch generation.
pub(crate) fn line_content(raw: &str) -> &str {
    raw.strip_suffix('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .unwrap_or(raw)
}

#[derive(Debug)]
struct TabExpansion {
    raw_start: usize,
    display_start: usize,
    display_end: usize,
}

/// Display text plus a reversible mapping back to the original source.
/// Tab-free lines borrow their source and allocate no mapping.
#[derive(Debug)]
pub(crate) struct DisplayText<'a> {
    pub(crate) text: Cow<'a, str>,
    raw: &'a str,
    tabs: Vec<TabExpansion>,
}

impl DisplayText<'_> {
    /// Map a source offset to the displayed text. Out-of-bounds offsets are
    /// clamped; an offset inside a UTF-8 character rounds down to its start.
    pub(crate) fn raw_to_display(&self, offset: usize) -> usize {
        let offset = floor_char_boundary(self.raw, offset);
        let count = self.tabs.partition_point(|tab| tab.raw_start < offset);
        let extra = count.checked_sub(1).map_or(0, |index| {
            let tab = &self.tabs[index];
            tab.display_end - (tab.raw_start + 1)
        });
        offset + extra
    }

    /// Map a shaped-text hit-test offset back to the source. Inside the spaces
    /// representing a tab, snap to the tab's source byte; at their end, snap
    /// past the tab. This makes every raw character boundary round-trip.
    pub(crate) fn display_to_raw(&self, offset: usize) -> usize {
        let offset = floor_char_boundary(&self.text, offset);
        let count = self.tabs.partition_point(|tab| tab.display_start <= offset);
        let Some(index) = count.checked_sub(1) else {
            return offset;
        };
        let tab = &self.tabs[index];
        if offset < tab.display_end {
            tab.raw_start
        } else {
            offset - (tab.display_end - (tab.raw_start + 1))
        }
    }

    /// Map syntax, word-diff, or selection spans to the text sent to GPUI.
    pub(crate) fn raw_range_to_display(&self, range: Range<usize>) -> Range<usize> {
        self.raw_to_display(range.start)..self.raw_to_display(range.end)
    }
}

fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

pub(crate) fn expand_tabs(raw: &str) -> DisplayText<'_> {
    if !raw.contains('\t') {
        return DisplayText {
            text: Cow::Borrowed(raw),
            raw,
            tabs: Vec::new(),
        };
    }

    let mut text = String::with_capacity(raw.len());
    let mut tabs = Vec::new();
    let mut column = 0;
    for (raw_start, ch) in raw.char_indices() {
        if ch == '\t' {
            let spaces = TAB_WIDTH - column % TAB_WIDTH;
            let display_start = text.len();
            text.extend(std::iter::repeat_n(' ', spaces));
            tabs.push(TabExpansion {
                raw_start,
                display_start,
                display_end: text.len(),
            });
            column += spaces;
        } else {
            text.push(ch);
            if ch == '\n' || ch == '\r' {
                column = 0;
            } else {
                column += ch.width().unwrap_or(0);
            }
        }
    }
    DisplayText {
        text: Cow::Owned(text),
        raw,
        tabs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_indentation_and_inline_tabs_to_stops() {
        let display = expand_tabs("\t\tif x\t==\ty {\t");
        assert_eq!(display.text, "        if x    ==  y { ");
    }

    #[test]
    fn borrows_tab_free_text_without_trimming() {
        let raw = "  café  \r\n";
        let display = expand_tabs(raw);
        assert!(matches!(display.text, Cow::Borrowed(_)));
        assert_eq!(display.text, raw);
        assert_eq!(display.raw_to_display(6), 5); // within é
        assert_eq!(display.display_to_raw(usize::MAX), raw.len());
    }

    #[test]
    fn maps_syntax_word_and_selection_ranges_after_tabs() {
        let raw = "\tif\tx";
        let display = expand_tabs(raw);
        assert_eq!(display.text, "    if  x");
        assert_eq!(display.raw_range_to_display(1..3), 4..6);
        assert_eq!(display.raw_range_to_display(0..1), 0..4);
        assert_eq!(display.raw_range_to_display(3..4), 6..8);
        assert_eq!(display.raw_range_to_display(4..5), 8..9);
        assert_eq!(display.display_to_raw(0), 0);
        assert_eq!(display.display_to_raw(3), 0);
        assert_eq!(display.display_to_raw(4), 1);
        assert_eq!(display.display_to_raw(7), 3);
        assert_eq!(display.display_to_raw(8), 4);
    }

    #[test]
    fn every_source_boundary_round_trips_including_unicode_and_adjacent_tabs() {
        for raw in ["", "\t", "\t\t", "a\t\tb\t", "é\t中\te\u{301}\tx  \t"] {
            let display = expand_tabs(raw);
            for offset in raw.char_indices().map(|(i, _)| i).chain([raw.len()]) {
                let rendered = display.raw_to_display(offset);
                assert!(display.text.is_char_boundary(rendered));
                assert_eq!(display.display_to_raw(rendered), offset, "{raw:?}");
            }
            for offset in 0..=display.text.len() {
                assert!(raw.is_char_boundary(display.display_to_raw(offset)));
            }
            assert_eq!(display.raw_to_display(usize::MAX), display.text.len());
            assert_eq!(display.display_to_raw(usize::MAX), raw.len());
        }
    }

    #[test]
    fn accounts_for_wide_and_combining_characters_and_newlines() {
        assert_eq!(expand_tabs("中\tx").text, "中  x");
        assert_eq!(expand_tabs("e\u{301}\tx").text, "e\u{301}   x");
        assert_eq!(expand_tabs("abc\t\n\tx").text, "abc \n    x");
    }

    #[test]
    fn removes_only_one_line_terminator_not_trailing_source_whitespace() {
        for (raw, expected) in [
            ("\treturn value\t  \r\n", "\treturn value\t  "),
            ("\t \n", "\t "),
            ("\t \r\n", "\t "),
            ("\t ", "\t "),
            ("x\n\n", "x\n"),
            ("x\r", "x\r"),
            ("", ""),
        ] {
            assert_eq!(line_content(raw), expected);
        }
    }

    #[test]
    fn source_and_trailing_whitespace_are_unchanged() {
        let raw = "\treturn value\t  \r\n".to_owned();
        let original = raw.clone();
        let display = expand_tabs(&raw);
        assert_eq!(raw, original);
        assert!(display.text.ends_with("  \r\n"));
        assert_eq!(display.raw, original);
    }
}
