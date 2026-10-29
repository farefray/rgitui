use gpui::prelude::*;
use gpui::{div, px, App, SharedString, Window};
use rgitui_theme::{ActiveTheme, Color, StyledExt};

use crate::{Label, LabelSize};

const TOTAL_BLOCKS: usize = 5;

/// Keep both sides visible for mixed changes, even when one is a tiny minority.
fn stat_blocks(added: usize, removed: usize) -> (usize, usize) {
    match (added, removed) {
        (0, 0) => (0, 0),
        (_, 0) => (TOTAL_BLOCKS, 0),
        (0, _) => (0, TOTAL_BLOCKS),
        _ => {
            let total = added as u128 + removed as u128;
            let green = ((added as u128 * TOTAL_BLOCKS as u128 + total / 2) / total)
                .clamp(1, (TOTAL_BLOCKS - 1) as u128) as usize;
            (green, TOTAL_BLOCKS - green)
        }
    }
}

/// A compact diff stat display showing additions/deletions with a colored bar.
///
/// Renders as: `+3 -2 [=====]` where the bar uses green for additions and red
/// for deletions, proportional to their share of total changes.
#[derive(IntoElement)]
pub struct DiffStat {
    added: usize,
    removed: usize,
}

impl DiffStat {
    pub fn new(added: usize, removed: usize) -> Self {
        Self { added, removed }
    }
}

impl RenderOnce for DiffStat {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let added_color = Color::Added.color(cx);
        let deleted_color = Color::Deleted.color(cx);
        let neutral_color = cx.colors().text_disabled;

        let (green_blocks, red_blocks) = stat_blocks(self.added, self.removed);

        let additions_str: SharedString = format!("+{}", self.added).into();
        let deletions_str: SharedString = format!("-{}", self.removed).into();

        let mut bar = div().h_flex().gap(px(1.));

        for i in 0..TOTAL_BLOCKS {
            let color = if green_blocks + red_blocks == 0 {
                neutral_color
            } else if i < green_blocks {
                added_color
            } else if i < green_blocks + red_blocks {
                deleted_color
            } else {
                neutral_color
            };
            bar = bar.child(div().w(px(4.)).h(px(10.)).rounded(px(1.)).bg(color));
        }

        div()
            .h_flex()
            .gap(px(4.))
            .flex_shrink_0()
            .items_center()
            .child(
                Label::new(additions_str)
                    .size(LabelSize::XSmall)
                    .color(if self.added == 0 {
                        Color::Muted
                    } else {
                        Color::Added
                    }),
            )
            .child(
                Label::new(deletions_str)
                    .size(LabelSize::XSmall)
                    .color(if self.removed == 0 {
                        Color::Muted
                    } else {
                        Color::Deleted
                    }),
            )
            .child(bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_changes_and_empty_stats() {
        assert_eq!(stat_blocks(0, 0), (0, 0));
        assert_eq!(stat_blocks(5, 0), (5, 0));
        assert_eq!(stat_blocks(0, 5), (0, 5));
    }

    #[test]
    fn mixed_changes_always_show_both_sides() {
        assert_eq!(stat_blocks(1000, 1), (4, 1));
        assert_eq!(stat_blocks(1, 1000), (1, 4));
        assert_eq!(stat_blocks(3, 2), (3, 2));
        assert_eq!(stat_blocks(2, 3), (2, 3));
        assert_eq!(stat_blocks(usize::MAX, usize::MAX), (3, 2));
    }
}
