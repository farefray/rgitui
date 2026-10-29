use gpui::prelude::*;
use gpui::{div, App, SharedString, Window};
use rgitui_git::FileChangeKind;
use rgitui_i18n::t;
use rgitui_theme::Color;
use rgitui_ui::{Badge, Tooltip};

pub(crate) fn file_change_color(kind: FileChangeKind) -> Color {
    match kind {
        FileChangeKind::Added => Color::Added,
        FileChangeKind::Modified => Color::Modified,
        FileChangeKind::Deleted => Color::Deleted,
        FileChangeKind::Renamed => Color::Renamed,
        FileChangeKind::Copied => Color::Info,
        FileChangeKind::TypeChange => Color::Warning,
        FileChangeKind::Untracked => Color::Untracked,
        FileChangeKind::Conflicted => Color::Conflict,
    }
}

fn file_change_label(kind: FileChangeKind) -> SharedString {
    match kind {
        FileChangeKind::Added => t!("file_status.added"),
        FileChangeKind::Modified => t!("file_status.modified"),
        FileChangeKind::Deleted => t!("file_status.deleted"),
        FileChangeKind::Renamed => t!("file_status.renamed"),
        FileChangeKind::Copied => t!("file_status.copied"),
        FileChangeKind::TypeChange => t!("file_status.type_changed"),
        FileChangeKind::Untracked => t!("file_status.untracked"),
        FileChangeKind::Conflicted => t!("file_status.conflicted"),
    }
}

/// A textual status cue, not just a color. Narrow sidebar rows use Git's
/// familiar status code and expose the same translated label on hover.
#[derive(IntoElement)]
pub(crate) struct FileStatusBadge {
    kind: FileChangeKind,
    compact: bool,
}

impl FileStatusBadge {
    pub(crate) fn new(kind: FileChangeKind) -> Self {
        Self {
            kind,
            compact: false,
        }
    }

    pub(crate) fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

impl RenderOnce for FileStatusBadge {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let label = file_change_label(self.kind);
        let text = if self.compact {
            self.kind.short_code().into()
        } else {
            label.clone()
        };
        div()
            .id("file-status")
            .flex_shrink_0()
            .tooltip(Tooltip::text(label))
            .child(
                Badge::new(text)
                    .color(file_change_color(self.kind))
                    .compact(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_statuses_have_distinct_text_cues() {
        let kinds = [
            FileChangeKind::Added,
            FileChangeKind::Modified,
            FileChangeKind::Deleted,
            FileChangeKind::Renamed,
            FileChangeKind::Copied,
            FileChangeKind::TypeChange,
            FileChangeKind::Untracked,
            FileChangeKind::Conflicted,
        ];
        let labels: std::collections::HashSet<_> = kinds
            .iter()
            .map(|kind| file_change_label(*kind).to_string())
            .collect();
        let codes: std::collections::HashSet<_> =
            kinds.iter().map(|kind| kind.short_code()).collect();
        assert_eq!(labels.len(), kinds.len());
        assert_eq!(codes.len(), kinds.len());
    }
}
