use std::collections::HashSet;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    div, px, ClickEvent, Context, ElementId, Entity, EventEmitter, FocusHandle, FontWeight, Render,
    ScrollHandle, SharedString, Window,
};
use rgitui_i18n::{key, t};
use rgitui_theme::{ActiveTheme, Color, StyledExt};
use rgitui_ui::{
    Button, ButtonStyle, Icon, IconName, IconSize, Label, LabelSize, TextInput, TextInputEvent,
    Tooltip,
};

use crate::keymap;
use crate::CommandId;

#[derive(Debug, Clone)]
pub enum RepoOpenerEvent {
    OpenRepo(PathBuf),
    Dismissed,
    ShowCloneDialog,
}

/// A repository already open in the workspace. Paths identify activation targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRepoEntry {
    pub path: PathBuf,
    pub name: String,
    pub worktree: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Open,
    Switch,
}

fn recent_entry(path: &std::path::Path) -> OpenRepoEntry {
    OpenRepoEntry {
        path: path.to_path_buf(),
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        worktree: None,
        active: false,
    }
}

/// Preserve workspace order, then recent order; open entries own duplicate paths.
/// No filesystem canonicalization here: workspace supplies normalized paths.
fn switcher_entries(open: &[OpenRepoEntry], recent: &[PathBuf]) -> Vec<OpenRepoEntry> {
    let mut seen = HashSet::new();
    open.iter()
        .cloned()
        .chain(recent.iter().map(|path| recent_entry(path)))
        .filter(|entry| seen.insert(entry.path.clone()))
        .collect()
}

fn filter_entries(entries: &[OpenRepoEntry], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.name.to_lowercase().contains(&query)
                || entry.path.to_string_lossy().to_lowercase().contains(&query)
                || entry
                    .worktree
                    .as_ref()
                    .is_some_and(|name| name.to_lowercase().contains(&query))
        })
        .map(|(index, _)| index)
        .collect()
}

fn initial_selection(mode: Mode, result_count: usize) -> Option<usize> {
    (mode == Mode::Switch && result_count > 0).then_some(0)
}

fn bounded_selection(row: usize, result_count: usize) -> Option<usize> {
    (result_count > 0).then(|| row.min(result_count - 1))
}

pub struct RepoOpener {
    editor: Entity<TextInput>,
    recent_repos: Vec<PathBuf>,
    open_repos: Vec<OpenRepoEntry>,
    entries: Vec<OpenRepoEntry>,
    mode: Mode,
    scroll_handle: ScrollHandle,
    focus_pending: bool,
    filtered_indices: Vec<usize>,
    selected_index: Option<usize>,
    visible: bool,
    focus_handle: FocusHandle,
}

impl EventEmitter<RepoOpenerEvent> for RepoOpener {}

impl RepoOpener {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            let mut ti = TextInput::new(cx);
            ti.set_placeholder_message(key!("repo_opener.enter_repo_path.placeholder"));
            ti
        });
        cx.subscribe(
            &editor,
            |this: &mut Self, _, event: &TextInputEvent, cx| match event {
                TextInputEvent::Submit => {
                    this.try_open(cx);
                }
                TextInputEvent::Blurred => {}
                TextInputEvent::Changed(_) => {
                    this.update_filter(cx);
                    cx.notify();
                }
            },
        )
        .detach();

        Self {
            editor,
            recent_repos: Vec::new(),
            open_repos: Vec::new(),
            entries: Vec::new(),
            mode: Mode::Open,
            scroll_handle: ScrollHandle::new(),
            focus_pending: false,
            filtered_indices: Vec::new(),
            selected_index: None,
            visible: false,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn set_open_repos(&mut self, repos: Vec<OpenRepoEntry>, cx: &mut Context<Self>) {
        self.open_repos = repos;
        if self.visible && self.mode == Mode::Switch {
            self.rebuild_entries();
            self.update_filter(cx);
            cx.notify();
        }
    }

    /// Switch among open repositories first, then recent repositories.
    pub fn toggle_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mode(Mode::Switch, cx);
        self.focus_editor(window, cx);
    }

    /// The original raw-path opener (Ctrl+O), not the repository switcher.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mode(Mode::Open, cx);
        self.focus_editor(window, cx);
    }

    fn focus_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible && self.focus_pending {
            self.focus_pending = false;
            self.editor
                .update(cx, |editor, cx| editor.focus(window, cx));
        }
    }

    fn rebuild_entries(&mut self) {
        self.entries = match self.mode {
            Mode::Open => self
                .recent_repos
                .iter()
                .map(|path| recent_entry(path))
                .collect(),
            Mode::Switch => switcher_entries(&self.open_repos, &self.recent_repos),
        };
    }

    fn toggle_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.visible && self.mode == mode {
            self.dismiss(cx);
            return;
        }
        self.mode = mode;
        self.visible = true;
        self.focus_pending = true;
        self.editor.update(cx, |editor, cx| {
            editor.set_placeholder_message(match mode {
                Mode::Open => key!("repo_opener.enter_repo_path.placeholder"),
                Mode::Switch => key!("repo_opener.search.placeholder"),
            });
            editor.clear(cx);
        });
        self.recent_repos = cx
            .global::<rgitui_settings::SettingsState>()
            .settings()
            .recent_repos
            .clone();
        self.rebuild_entries();
        self.update_filter(cx);
        self.scroll_handle.scroll_to_item(0);
        cx.notify();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn toggle_visible(&mut self, cx: &mut Context<Self>) {
        // Mouse callers without a Window get autofocus on the next render.
        self.toggle_mode(Mode::Open, cx);
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.visible = false;
        self.focus_pending = false;
        self.editor.update(cx, |e, cx| e.clear(cx));
        cx.emit(RepoOpenerEvent::Dismissed);
        cx.notify();
    }

    fn update_filter(&mut self, cx: &mut Context<Self>) {
        self.filtered_indices = filter_entries(&self.entries, self.editor.read(cx).text());
        // A new query selects the first result only in switcher mode. In raw-path
        // mode Enter must continue to open the typed path, not a recent match.
        self.selected_index = initial_selection(self.mode, self.filtered_indices.len());
        self.scroll_handle.scroll_to_item(0);
    }

    fn try_open(&mut self, cx: &mut Context<Self>) {
        let path = if let Some(selected_index) = self.selected_index {
            match self.filtered_indices.get(selected_index) {
                Some(&idx) => self.entries[idx].path.clone(),
                None => return,
            }
        } else {
            if self.mode == Mode::Switch {
                return;
            }
            let query = self.editor.read(cx).text().to_string();
            if query.is_empty() {
                return;
            }
            if let Some(stripped) = query.strip_prefix('~') {
                if let Some(home) = dirs::home_dir() {
                    home.join(stripped.trim_start_matches('/'))
                } else {
                    PathBuf::from(&query)
                }
            } else {
                PathBuf::from(&query)
            }
        };

        self.visible = false;
        self.editor.update(cx, |e, cx| e.clear(cx));
        cx.emit(RepoOpenerEvent::OpenRepo(path));
        cx.notify();
    }

    fn browse_folder(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let folder = async {
                rfd::AsyncFileDialog::new()
                    .set_title(t!("repo_opener.select_folder").to_string())
                    .pick_folder()
                    .await
                    .map(|handle| handle.path().to_path_buf())
            }
            .await;
            if let Some(path) = folder {
                cx.update(|cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.visible = false;
                        this.editor.update(cx, |e, cx| e.clear(cx));
                        cx.emit(RepoOpenerEvent::OpenRepo(path));
                        cx.notify();
                    });
                });
            }
        })
        .detach();
    }

    /// Runs a keyboard command scoped to `RepoOpener` or to the shared `List`
    /// group.
    ///
    /// Enter is propagated so the path field's own submission opens the
    /// highlighted repository exactly once.
    fn dispatch_command(&mut self, cmd: CommandId, _window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            CommandId::Cancel => self.dismiss(cx),
            CommandId::SelectPrev => self.select_row(
                match self.selected_index {
                    Some(index) if index > 0 => index - 1,
                    Some(index) => index,
                    None => self.filtered_indices.len().saturating_sub(1),
                },
                cx,
            ),
            CommandId::SelectNext => self.select_row(
                match self.selected_index {
                    Some(index) if index + 1 < self.filtered_indices.len() => index + 1,
                    Some(index) => index,
                    None => 0,
                },
                cx,
            ),
            CommandId::SelectFirst => self.select_row(0, cx),
            CommandId::SelectLast => {
                self.select_row(self.filtered_indices.len().saturating_sub(1), cx)
            }
            _ => cx.propagate(),
        }
    }

    /// Moves the highlight within the filtered list.
    fn select_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.selected_index = bounded_selection(row, self.filtered_indices.len());
        if let Some(row) = self.selected_index {
            self.scroll_handle.scroll_to_item(row);
        }
        cx.notify();
    }
}

impl Render for RepoOpener {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().id("repo-opener").into_any_element();
        }

        self.focus_editor(window, cx);
        let switching = self.mode == Mode::Switch;
        let colors = cx.colors().clone();

        let mut modal = div()
            .id("repo-opener-modal")
            .track_focus(&self.focus_handle)
            .map(|el| {
                keymap::bind_actions(el, "RepoOpener List", &["Menu"], cx, Self::dispatch_command)
            })
            .v_flex()
            .w(px(640.))
            .max_h(px(480.))
            .elevation_3(cx)
            .rounded(px(10.))
            .overflow_hidden()
            .on_click(|_: &ClickEvent, _, cx| {
                cx.stop_propagation();
            })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            });

        modal = modal.child(
            div()
                .h_flex()
                .w_full()
                .h(px(48.))
                .px(px(16.))
                .items_center()
                .border_b_1()
                .border_color(colors.border_variant)
                .justify_between()
                .child(
                    div()
                        .h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(
                            Icon::new(IconName::Folder)
                                .size(IconSize::Small)
                                .color(Color::Muted),
                        )
                        .child(
                            Label::new(if switching {
                                t!("repo_opener.switch_repository")
                            } else {
                                t!("common.open_repository")
                            })
                            .size(LabelSize::Large)
                            .weight(FontWeight::SEMIBOLD),
                        ),
                )
                .child(
                    div()
                        .id("repo-opener-close-btn")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(28.))
                        .h(px(28.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(colors.ghost_element_hover))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.dismiss(cx);
                        }))
                        .child(
                            Icon::new(IconName::X)
                                .size(IconSize::Small)
                                .color(Color::Muted),
                        ),
                ),
        );

        modal = modal.child(
            div()
                .px(px(16.))
                .py(px(12.))
                .v_flex()
                .gap(px(8.))
                .child(
                    Label::new(if switching {
                        t!("repo_opener.search")
                    } else {
                        t!("repo_opener.repo_path")
                    })
                    .size(LabelSize::Small)
                    .weight(FontWeight::MEDIUM)
                    .color(Color::Muted),
                )
                .child(
                    div()
                        .h_flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .h_flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    Icon::new(IconName::Folder)
                                        .size(IconSize::Small)
                                        .color(Color::Muted),
                                )
                                .child(div().flex_1().child(self.editor.clone())),
                        )
                        .when(!switching, |el| {
                            el.child(
                                Button::new("browse-folder", t!("common.browse"))
                                    .style(ButtonStyle::Subtle)
                                    .icon(IconName::Folder)
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.browse_folder(cx);
                                    })),
                            )
                        })
                        .when(!switching, |el| {
                            el.child(
                                Button::new("clone-repo", t!("common.clone"))
                                    .style(ButtonStyle::Subtle)
                                    .icon(IconName::Plus)
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.visible = false;
                                        this.editor.update(cx, |e, cx| e.clear(cx));
                                        cx.emit(RepoOpenerEvent::ShowCloneDialog);
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
        );

        if !self.entries.is_empty() || switching {
            modal = modal.child(
                div()
                    .h_flex()
                    .w_full()
                    .px(px(16.))
                    .pt(px(4.))
                    .pb(px(8.))
                    .border_t_1()
                    .border_color(colors.border_variant)
                    .items_center()
                    .child(
                        Label::new(if switching {
                            t!("repo_opener.open_then_recent")
                        } else {
                            t!("common.recent_repositories")
                        })
                        .size(LabelSize::XSmall)
                        .weight(FontWeight::SEMIBOLD)
                        .color(Color::Muted),
                    ),
            );

            let mut results = div()
                .id("repo-opener-results")
                .v_flex()
                .w_full()
                .px(px(8.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll_handle)
                .min_h_0()
                .max_h(px(260.));

            for (display_idx, &repo_idx) in self.filtered_indices.iter().enumerate() {
                let entry = &self.entries[repo_idx];
                let repo_path = &entry.path;
                let repo_name: SharedString = entry.name.clone().into();
                let repo_path_display: SharedString =
                    repo_path.to_string_lossy().to_string().into();
                let is_selected = self.selected_index == Some(display_idx);
                let path_clone = repo_path.clone();

                let hover_bg = colors.ghost_element_hover;
                let selected_bg = colors.element_selected;

                let row = div()
                    .id(ElementId::NamedInteger(
                        "repo-opener-item".into(),
                        display_idx as u64,
                    ))
                    .h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .px(px(16.))
                    .py(px(12.))
                    .gap(px(12.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .tooltip(Tooltip::text(repo_path_display.clone()))
                    .when(is_selected, move |el| el.bg(selected_bg))
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let path = path_clone.clone();
                        this.visible = false;
                        this.editor.update(cx, |e, cx| e.clear(cx));
                        cx.emit(RepoOpenerEvent::OpenRepo(path));
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::Folder).size(IconSize::Small).color(
                        if is_selected {
                            Color::Accent
                        } else {
                            Color::Muted
                        },
                    ))
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(
                                div()
                                    .h_flex()
                                    .gap(px(8.))
                                    .child(
                                        Label::new(repo_name)
                                            .size(LabelSize::Small)
                                            .weight(FontWeight::MEDIUM)
                                            .color(Color::Default),
                                    )
                                    .when(switching && entry.active, |el| {
                                        el.child(
                                            Label::new(t!("repo_opener.active"))
                                                .size(LabelSize::XSmall)
                                                .color(Color::Accent),
                                        )
                                    }),
                            )
                            .when_some(entry.worktree.as_ref(), |el, worktree| {
                                el.child(
                                    Label::new(t!(
                                        "repo_opener.worktree",
                                        name = worktree.as_str()
                                    ))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                                )
                            })
                            .child(
                                div().w_full().whitespace_normal().overflow_hidden().child(
                                    Label::new(repo_path_display)
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                ),
                            ),
                    );

                results = results.child(row);
            }

            if self.filtered_indices.is_empty() {
                results = results.child(
                    div()
                        .w_full()
                        .py(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .v_flex()
                        .gap(px(8.))
                        .child(
                            Icon::new(IconName::Search)
                                .size(IconSize::Large)
                                .color(Color::Placeholder),
                        )
                        .child(
                            Label::new(t!("repo_opener.no_match_repos"))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                );
            }

            modal = modal.child(results);
        } else {
            modal = modal.child(
                div()
                    .w_full()
                    .py(px(32.))
                    .border_t_1()
                    .border_color(colors.border_variant)
                    .flex()
                    .items_center()
                    .justify_center()
                    .v_flex()
                    .gap(px(8.))
                    .child(
                        Icon::new(IconName::Folder)
                            .size(IconSize::Large)
                            .color(Color::Placeholder),
                    )
                    .child(
                        Label::new(t!("repo_opener.no_recent_repos"))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            );
        }

        modal = modal.child(
            div()
                .h_flex()
                .justify_end()
                .gap(px(8.))
                .px(px(16.))
                .py(px(12.))
                .border_t_1()
                .border_color(colors.border_variant)
                .bg(colors.surface_background)
                .child(
                    Button::new("cancel-open", t!("common.cancel"))
                        .style(ButtonStyle::Subtle)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.dismiss(cx);
                        })),
                )
                .child(
                    Button::new(
                        "open-repo",
                        if switching {
                            t!("repo_opener.switch")
                        } else {
                            t!("repo_opener.open")
                        },
                    )
                    .disabled(switching && self.selected_index.is_none())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.try_open(cx);
                    })),
                ),
        );

        div()
            .id("repo-opener-backdrop")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.0,
                a: 0.4,
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.dismiss(cx);
            }))
            .child(modal)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_entry(path: &str, name: &str, worktree: Option<&str>, active: bool) -> OpenRepoEntry {
        OpenRepoEntry {
            path: PathBuf::from(path),
            name: name.to_owned(),
            worktree: worktree.map(str::to_owned),
            active,
        }
    }

    #[test]
    fn open_repositories_precede_deduplicated_recents() {
        let open = vec![
            open_entry("/work/app", "App", Some("feature"), true),
            open_entry("/work/lib", "Library", None, false),
        ];
        let recent = [
            "/old/repo",
            "/work/app",
            "/old/repo",
            "/work/lib",
            "/last/repo",
        ]
        .map(PathBuf::from);
        let entries = switcher_entries(&open, &recent);
        assert_eq!(&entries[..2], open.as_slice());
        assert_eq!(entries[2].path, PathBuf::from("/old/repo"));
        assert_eq!(entries[3].path, PathBuf::from("/last/repo"));
        assert_eq!(entries.len(), 4);
        assert!(!entries[2].active);
    }

    #[test]
    fn duplicate_open_paths_keep_first_metadata_but_not_distinct_worktrees() {
        let open = vec![
            open_entry("/work/app", "App", Some("main"), true),
            open_entry("/work/app", "Duplicate", None, false),
            open_entry("/trees/app", "App", Some("feature"), false),
        ];
        assert_eq!(
            switcher_entries(&open, &[]),
            vec![open[0].clone(), open[2].clone()]
        );
    }

    #[test]
    fn search_matches_name_full_path_and_worktree_case_insensitively() {
        let entries = vec![
            open_entry("/work/app", "Frontend", Some("Release-42"), true),
            open_entry("/archive/service", "Backend", None, false),
            open_entry("/trees/app", "Frontend", Some("feature"), false),
        ];
        assert_eq!(filter_entries(&entries, " FRONTEND "), vec![0, 2]);
        assert_eq!(filter_entries(&entries, "/ARCHIVE/"), vec![1]);
        assert_eq!(filter_entries(&entries, "release-42"), vec![0]);
        assert_eq!(filter_entries(&entries, ""), vec![0, 1, 2]);
        assert!(filter_entries(&entries, "missing").is_empty());
    }

    #[test]
    fn switcher_selects_first_filtered_result_but_raw_path_mode_does_not() {
        let entries = vec![
            open_entry("/work/app", "App", None, true),
            open_entry("/work/lib", "Library", None, false),
        ];
        let matches = filter_entries(&entries, "library");
        let selected = initial_selection(Mode::Switch, matches.len()).unwrap();
        assert_eq!(entries[matches[selected]].path, PathBuf::from("/work/lib"));
        assert_eq!(initial_selection(Mode::Switch, 0), None);
        assert_eq!(initial_selection(Mode::Open, 2), None);
        assert_eq!(initial_selection(Mode::Open, 0), None);
    }

    #[test]
    fn selection_is_bounded_for_empty_single_and_fifteen_repo_lists() {
        assert_eq!(bounded_selection(0, 0), None);
        assert_eq!(bounded_selection(usize::MAX, 0), None);
        assert_eq!(bounded_selection(12, 1), Some(0));
        assert_eq!(bounded_selection(0, 15), Some(0));
        assert_eq!(bounded_selection(7, 15), Some(7));
        assert_eq!(bounded_selection(15, 15), Some(14));
        assert_eq!(initial_selection(Mode::Switch, 15), Some(0));
    }

    #[test]
    fn recent_only_and_empty_switchers_are_supported() {
        assert!(switcher_entries(&[], &[]).is_empty());
        let entries = switcher_entries(&[], &[PathBuf::from("/recent/repo")]);
        assert_eq!(
            entries,
            vec![open_entry("/recent/repo", "repo", None, false)]
        );
    }

    #[test]
    fn test_repo_opener_event_debug() {
        let event = RepoOpenerEvent::Dismissed;
        assert_eq!(format!("{:?}", event), "Dismissed");

        let event = RepoOpenerEvent::OpenRepo(PathBuf::from("/tmp/repo"));
        assert_eq!(format!("{:?}", event), "OpenRepo(\"/tmp/repo\")");
    }

    #[test]
    fn test_repo_opener_event_match() {
        let event = RepoOpenerEvent::OpenRepo(PathBuf::from("/tmp/repo"));
        if let RepoOpenerEvent::OpenRepo(path) = event {
            assert_eq!(path, PathBuf::from("/tmp/repo"));
        } else {
            panic!("Expected OpenRepo");
        }

        let event = RepoOpenerEvent::Dismissed;
        if let RepoOpenerEvent::Dismissed = event {
            // expected
        } else {
            panic!("Expected Dismissed");
        }
    }
}
