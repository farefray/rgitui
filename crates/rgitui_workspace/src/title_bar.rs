use gpui::prelude::*;
use gpui::{div, px, App, ClickEvent, Decorations, MouseButton, SharedString, Window};
use rgitui_i18n::t;
use rgitui_theme::{ActiveTheme, Color, StyledExt};
use rgitui_ui::{
    Button, ButtonSize, ButtonStyle, Icon, IconName, IconSize, Label, LabelSize, Tooltip,
    WindowControl, WindowControlType,
};

use crate::keymap;
use crate::CommandId;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A worktree sharing its branch's name needs only a role label, not a second
/// copy of that name. Detached commit ids are not branch names.
fn distinct_worktree_name<'a>(name: &'a str, head: &str, detached: bool) -> Option<&'a str> {
    (detached || name != head).then_some(name)
}

/// Application chrome and the active checkout. Repository identity lives in
/// the stable tabs below, independently of branch/worktree navigation.
#[derive(IntoElement)]
pub struct TitleBar {
    branch_name: SharedString,
    checkout_path: Option<SharedString>,
    worktree_name: Option<SharedString>,
    has_changes: bool,
    head_detached: bool,
    repo_state_label: Option<SharedString>,
    on_branch_click: Option<ClickHandler>,
    on_exit_worktree: Option<ClickHandler>,
}

impl TitleBar {
    pub fn new(branch_name: impl Into<SharedString>) -> Self {
        Self {
            branch_name: branch_name.into(),
            checkout_path: None,
            worktree_name: None,
            has_changes: false,
            head_detached: false,
            repo_state_label: None,
            on_branch_click: None,
            on_exit_worktree: None,
        }
    }

    pub fn checkout_path(mut self, path: impl Into<SharedString>) -> Self {
        self.checkout_path = Some(path.into());
        self
    }

    pub fn worktree(mut self, name: impl Into<SharedString>) -> Self {
        self.worktree_name = Some(name.into());
        self
    }

    pub fn on_exit_worktree(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_exit_worktree = Some(Box::new(handler));
        self
    }

    pub fn has_changes(mut self, has_changes: bool) -> Self {
        self.has_changes = has_changes;
        self
    }

    pub fn head_detached(mut self, detached: bool) -> Self {
        self.head_detached = detached;
        self
    }

    pub fn repo_state(mut self, label: impl Into<SharedString>) -> Self {
        self.repo_state_label = Some(label.into());
        self
    }

    pub fn on_branch_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_branch_click = Some(Box::new(handler));
        self
    }

    fn render_separator(colors: &rgitui_theme::ThemeColors) -> gpui::Div {
        div()
            .w(px(1.))
            .h(px(14.))
            .rounded(px(0.5))
            .bg(colors.border_variant)
    }

    /// A `key — what it does` hint, with the keystroke read from the keymap.
    ///
    /// Returns nothing when the command is unbound, so removing a binding in
    /// `keymap.json` removes the hint rather than leaving a lie in the title bar.
    fn render_keyboard_hint(
        colors: &rgitui_theme::ThemeColors,
        command: CommandId,
        label_text: &'static str,
        cx: &App,
    ) -> Option<gpui::Div> {
        let key = keymap::shortcut(command, cx)?;
        Some(Self::keyboard_hint(colors, key, label_text))
    }

    fn keyboard_hint(
        colors: &rgitui_theme::ThemeColors,
        key: String,
        label_text: &'static str,
    ) -> gpui::Div {
        div()
            .h_flex()
            .gap(px(3.))
            .items_center()
            .child(
                div()
                    .h(px(16.))
                    .px(px(4.))
                    .rounded(px(3.))
                    .bg(colors.ghost_element_hover)
                    .flex()
                    .items_center()
                    .child(
                        Label::new(SharedString::from(key))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                            .weight(gpui::FontWeight::MEDIUM),
                    ),
            )
            .child(
                Label::new(label_text)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.colors().clone();
        // The title bar can paint before settings finish loading, so fall
        // back to English rather than requiring the global.

        let branch_color = if self.head_detached {
            Color::Warning
        } else {
            Color::Accent
        };

        let hover_bg = colors.ghost_element_active;
        let branch_bg = colors.element_background;
        let worktree_label = self.worktree_name.as_ref().map(|name| {
            match distinct_worktree_name(name, &self.branch_name, self.head_detached) {
                Some(name) => t!("workspace.inspecting.plain", name = name),
                None => t!("title_bar.worktree"),
            }
        });
        let tooltip = match self.worktree_name.as_ref() {
            Some(name) if self.head_detached => t!("workspace.inspecting.detached", name = name),
            Some(name) if name != &self.branch_name => {
                t!(
                    "workspace.inspecting.branch",
                    name = name,
                    branch = self.branch_name
                )
            }
            Some(name) => t!("workspace.inspecting.plain", name = name),
            None if self.head_detached => {
                t!("workspace.detached.title", commit = self.branch_name)
            }
            None => self.branch_name.clone(),
        };
        let tooltip = match self.checkout_path.as_ref() {
            Some(path) if self.worktree_name.is_some() => format!(
                "{}\n{}",
                tooltip,
                t!("workspace.worktree.context.tooltip", path = path)
            )
            .into(),
            Some(path) => format!("{tooltip}\n{path}").into(),
            None => tooltip,
        };

        let mut branch_pill = div()
            .id("title-branch-pill")
            .h_flex()
            .h(px(28.))
            .min_h(gpui::rems(1.75))
            .min_w_0()
            .max_w(px(if self.worktree_name.is_some() {
                460.
            } else {
                280.
            }))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded(px(4.))
            .border_1()
            .border_color(colors.border_variant)
            .bg(branch_bg)
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .tooltip(Tooltip::text(tooltip))
            .when_some(worktree_label, |el, label| {
                el.child(
                    Label::new(label)
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                )
            })
            .child(
                Icon::new(IconName::GitBranch)
                    .size(IconSize::Small)
                    .color(branch_color),
            )
            .child(
                Label::new(self.branch_name.clone())
                    .size(LabelSize::Small)
                    .color(branch_color)
                    .weight(gpui::FontWeight::SEMIBOLD)
                    .truncate(),
            )
            .when(self.head_detached, |el| {
                el.child(
                    Label::new(t!("title_bar.detached"))
                        .size(LabelSize::XSmall)
                        .color(Color::Warning),
                )
            })
            .when(self.has_changes && !self.head_detached, |el| {
                el.child(
                    div()
                        .w(px(6.))
                        .h(px(6.))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(colors.vc_modified),
                )
            });

        if let Some(handler) = self.on_branch_click {
            branch_pill = branch_pill.on_click(handler);
        }

        let mut bar = div()
            .h_flex()
            .w_full()
            .h(px(38.))
            .min_h(gpui::rems(2.375))
            .flex_shrink_0()
            .bg(colors.title_bar_background)
            .border_b_1()
            .border_color(colors.border)
            .px(px(12.))
            .gap(px(8.))
            .items_center();

        // Native/server decorations own their chrome. Fullscreen keeps checkout
        // context, but has neither custom controls nor a move/zoom region.
        let use_custom_chrome = cfg!(any(target_os = "linux", target_os = "freebsd"))
            && matches!(window.window_decorations(), Decorations::Client { .. })
            && !window.is_fullscreen();

        if use_custom_chrome {
            // Window controls on the left (Linux CSD convention)
            bar = bar.child(
                div()
                    .h_flex()
                    .gap(px(4.))
                    .items_center()
                    .child(WindowControl::new(
                        "minimize",
                        WindowControlType::Minimize,
                        cx,
                    ))
                    .child(WindowControl::new(
                        "maximize",
                        if window.is_maximized() {
                            WindowControlType::Restore
                        } else {
                            WindowControlType::Maximize
                        },
                        cx,
                    ))
                    .child(WindowControl::new("close", WindowControlType::Close, cx)),
            );
        }

        bar = bar
            .child(
                div()
                    .h_flex()
                    .gap(px(5.))
                    .items_center()
                    .child(
                        Icon::new(IconName::GitCommit)
                            .size(IconSize::Small)
                            .color(Color::Accent),
                    )
                    .child(
                        Label::new("rgitui")
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .weight(gpui::FontWeight::BOLD),
                    ),
            )
            .child(Self::render_separator(&colors))
            .child(branch_pill);

        if let Some(handler) = self
            .on_exit_worktree
            .filter(|_| self.worktree_name.is_some())
        {
            bar = bar.child(
                Button::new("exit-worktree-inspection", t!("workspace.worktree.exit"))
                    .size(ButtonSize::Compact)
                    .style(ButtonStyle::Subtle)
                    .tooltip(t!("workspace.worktree.exit.tooltip"))
                    .on_click(handler),
            );
        }

        if let Some(state_label) = self.repo_state_label {
            bar = bar.child(
                div()
                    .h_flex()
                    .h(px(22.))
                    .min_h(gpui::rems(1.375))
                    .flex_shrink_0()
                    .px(px(6.))
                    .gap(px(4.))
                    .items_center()
                    .rounded(px(3.))
                    .bg(colors.ghost_element_selected)
                    .child(
                        Icon::new(IconName::FileConflict)
                            .size(IconSize::XSmall)
                            .color(Color::Warning),
                    )
                    .child(
                        Label::new(state_label)
                            .size(LabelSize::XSmall)
                            .color(Color::Warning)
                            .weight(gpui::FontWeight::SEMIBOLD),
                    ),
            );
        }

        // Only this blank sibling is draggable: branch and window-control events
        // must never bubble into a move handler on the whole title bar.
        bar = bar.child(div().id("title-bar-drag-area").flex_1().h_full().when(
            use_custom_chrome,
            |spacer| {
                spacer
                    // Keep a usable target when a maximized window is restored to
                    // the small startup bounds, even with long repo/branch names.
                    .min_w(px(32.))
                    .on_mouse_down(MouseButton::Left, |event, window, cx| {
                        if window.is_fullscreen() {
                            return;
                        }
                        cx.stop_propagation();
                        if event.click_count == 2 {
                            window.zoom_window();
                        } else {
                            // Start synchronously on mouse-down so Wayland can use the
                            // real pointer-button serial for the compositor move grab.
                            window.start_window_move();
                        }
                    })
            },
        ));

        bar.child(
            div()
                .h_flex()
                .gap(px(12.))
                .items_center()
                .children(Self::render_keyboard_hint(
                    &colors,
                    CommandId::CommandPalette,
                    "Commands",
                    cx,
                ))
                .children(Self::render_keyboard_hint(
                    &colors,
                    CommandId::Shortcuts,
                    "Help",
                    cx,
                )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::distinct_worktree_name;

    #[test]
    fn matching_worktree_and_branch_share_one_name() {
        assert_eq!(distinct_worktree_name("feature", "feature", false), None);
        assert_eq!(distinct_worktree_name("main", "main", false), None);
    }

    #[test]
    fn different_worktree_name_remains_meaningful_context() {
        assert_eq!(
            distinct_worktree_name("review", "feature/auth", false),
            Some("review")
        );
        // Do not conflate a branch basename with a full ref name or fold case.
        assert_eq!(
            distinct_worktree_name("auth", "feature/auth", false),
            Some("auth")
        );
        assert_eq!(distinct_worktree_name("Main", "main", false), Some("Main"));
    }

    #[test]
    fn detached_commit_is_not_a_matching_branch() {
        assert_eq!(
            distinct_worktree_name("abc1234", "abc1234", true),
            Some("abc1234")
        );
    }
}
