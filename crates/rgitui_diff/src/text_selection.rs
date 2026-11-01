use std::ops::Range;

/// Byte positions in original (not tab-expanded) diff text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TextPoint {
    pub row: usize,
    pub byte: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TextSelection {
    pub lane: &'static str,
    pub anchor: TextPoint,
    pub head: TextPoint,
}

impl TextSelection {
    pub fn bounds(self) -> (TextPoint, TextPoint) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn copy_from<'a>(self, rows: impl Iterator<Item = (usize, &'a str)>) -> String {
        rows.filter_map(|(row, text)| self.range_on(row, text).map(|range| &text[range]))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn range_on(self, row: usize, text: &str) -> Option<Range<usize>> {
        let (start, end) = self.bounds();
        if row < start.row || row > end.row {
            return None;
        }
        let from = if row == start.row { start.byte } else { 0 };
        let to = if row == end.row { end.byte } else { text.len() };
        Some(clamp_boundary(text, from)..clamp_boundary(text, to))
    }
}

pub(super) fn clamp_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_partial_line_in_either_direction() {
        let selection = TextSelection {
            lane: "unified",
            anchor: TextPoint { row: 2, byte: 7 },
            head: TextPoint { row: 2, byte: 2 },
        };
        assert_eq!(selection.range_on(2, "hello world"), Some(2..7));
        assert_eq!(selection.range_on(1, "hello world"), None);
    }

    #[test]
    fn multiline_selection_keeps_partial_endpoints() {
        let selection = TextSelection {
            lane: "unified",
            anchor: TextPoint { row: 1, byte: 2 },
            head: TextPoint { row: 3, byte: 3 },
        };
        assert_eq!(selection.range_on(1, "first"), Some(2..5));
        assert_eq!(selection.range_on(2, "middle"), Some(0..6));
        assert_eq!(selection.range_on(3, "last"), Some(0..3));
    }

    #[test]
    fn copying_keeps_tabs_unicode_and_empty_lines_without_diff_prefixes() {
        let selection = TextSelection {
            lane: "sbs-right",
            anchor: TextPoint { row: 1, byte: 1 },
            head: TextPoint { row: 4, byte: 5 },
        };
        let rows = [(1, "\treturn 界"), (2, ""), (4, "\tfunc main()")];
        assert_eq!(selection.copy_from(rows.into_iter()), "return 界\n\n\tfunc");
        let reverse = TextSelection {
            anchor: selection.head,
            head: selection.anchor,
            ..selection
        };
        assert_eq!(
            reverse.copy_from(rows.into_iter()),
            selection.copy_from(rows.into_iter())
        );
    }

    #[test]
    fn empty_and_unicode_lines_are_safe() {
        let selection = TextSelection {
            lane: "unified",
            anchor: TextPoint { row: 0, byte: 1 },
            head: TextPoint { row: 0, byte: 99 },
        };
        assert_eq!(selection.range_on(0, "é界"), Some(0..5));
        assert_eq!(selection.range_on(0, ""), Some(0..0));
        assert_eq!(clamp_boundary("aé界", 4), 3);
    }
}
