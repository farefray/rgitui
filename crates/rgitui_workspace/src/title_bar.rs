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

/// Whether this window draws its own title bar.
///
/// Native/server decorations own their chrome. Fullscreen keeps checkout
/// context, but has neither custom controls nor a move/zoom region.
pub(crate) fn uses_custom_chrome(window: &Window) -> bool {
    cfg!(any(target_os = "linux", target_os = "freebsd"))
        && matches!(window.window_decorations(), Decorations::Client { .. })
        && !window.is_fullscreen()
}

/// A `key — what it does` hint, with the keystroke read from the keymap.
///
/// Returns nothing when the command is unbound, so removing a binding in
/// `keymap.json` removes the hint rather than leaving a lie on screen.
fn render_keyboard_hint(
    colors: &rgitui_theme::ThemeColors,
    command: CommandId,
    label_text: SharedString,
    cx: &App,
) -> Option<gpui::Div> {
    let key = keymap::shortcut(command, cx)?;
    Some(
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
            ),
    )
}

/// The command palette and shortcut help hints. Shown in the window chrome when
/// rgitui draws it, otherwise at the end of the repository tab strip.
pub(crate) fn keyboard_hints(cx: &App) -> gpui::Div {
    let colors = cx.colors();
    div()
        .h_flex()
        .flex_shrink_0()
        .gap(px(12.))
        .items_center()
        .children(render_keyboard_hint(
            colors,
            CommandId::CommandPalette,
            t!("title_bar.commands"),
            cx,
        ))
        .children(render_keyboard_hint(
            colors,
            CommandId::Shortcuts,
            t!("title_bar.help"),
            cx,
        ))
}

/// Client-side window chrome: window controls, a move region and keyboard
/// hints. Native decorations already provide the first two, so the workspace
/// renders this only when [`uses_custom_chrome`] holds; the checkout context
/// lives in [`CheckoutBreadcrumb`] beside the actions it scopes.
#[derive(IntoElement, Default)]
pub struct TitleBar;

impl TitleBar {
    pub fn new() -> Self {
        Self
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.colors();
        div()
            .h_flex()
            .w_full()
            .h(px(34.))
            .flex_shrink_0()
            .bg(colors.title_bar_background)
            .border_b_1()
            .border_color(colors.border)
            .px(px(12.))
            .gap(px(8.))
            .items_center()
            // Window controls on the left (Linux CSD convention)
            .child(
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
            )
            .child(
                Label::new("rgitui")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .weight(gpui::FontWeight::BOLD),
            )
            // Only this blank sibling is draggable: window-control events must
            // never bubble into a move handler on the whole title bar.
            .child(
                div()
                    .id("title-bar-drag-area")
                    .flex_1()
                    .h_full()
                    // Keep a usable target when a maximized window is restored
                    // to the small startup bounds.
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
                    }),
            )
            .child(keyboard_hints(cx))
    }
}

/// The active checkout as a `repository › branch` breadcrumb at the start of
/// the toolbar, beside the actions it scopes. Repository identity also lives
/// in the stable tabs above, independently of branch/worktree navigation.
#[derive(IntoElement)]
pub struct CheckoutBreadcrumb {
    repo_name: SharedString,
    branch_name: SharedString,
    checkout_path: Option<SharedString>,
    worktree_name: Option<SharedString>,
    has_changes: bool,
    head_detached: bool,
    repo_state_label: Option<SharedString>,
    on_repo_click: Option<ClickHandler>,
    on_branch_click: Option<ClickHandler>,
    on_exit_worktree: Option<ClickHandler>,
}

impl CheckoutBreadcrumb {
    pub fn new(repo_name: impl Into<SharedString>, branch_name: impl Into<SharedString>) -> Self {
        Self {
            repo_name: repo_name.into(),
            branch_name: branch_name.into(),
            checkout_path: None,
            worktree_name: None,
            has_changes: false,
            head_detached: false,
            repo_state_label: None,
            on_repo_click: None,
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

    pub fn on_repo_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_repo_click = Some(Box::new(handler));
        self
    }

    pub fn on_branch_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_branch_click = Some(Box::new(handler));
        self
    }

    /// One clickable crumb: a small caption above its value, like the labelled
    /// actions that follow it in the toolbar.
    fn crumb(
        id: &'static str,
        caption: SharedString,
        value: gpui::Div,
        tooltip: SharedString,
        on_click: Option<ClickHandler>,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let hover_bg = cx.colors().ghost_element_hover;
        let active_bg = cx.colors().ghost_element_active;
        let mut crumb = div()
            .id(id)
            .v_flex()
            .min_w_0()
            .justify_center()
            .px(px(8.))
            .py(px(3.))
            .gap(px(1.))
            .rounded(px(6.))
            .tooltip(Tooltip::text(tooltip))
            .child(
                Label::new(caption)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .truncate(),
            )
            .child(
                value.child(
                    Icon::new(IconName::ChevronDown)
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                ),
            );
        if let Some(handler) = on_click {
            crumb = crumb
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .active(move |s| s.bg(active_bg))
                .on_click(handler);
        }
        crumb
    }
}

impl RenderOnce for CheckoutBreadcrumb {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.colors().clone();

        let branch_color = if self.head_detached {
            Color::Warning
        } else {
            Color::Accent
        };
        let branch_caption = match self.worktree_name.as_ref() {
            Some(name) => {
                match distinct_worktree_name(name, &self.branch_name, self.head_detached) {
                    Some(name) => t!("workspace.inspecting.plain", name = name),
                    None => t!("title_bar.worktree"),
                }
            }
            None => t!("title_bar.branch"),
        };
        let branch_tooltip = match self.worktree_name.as_ref() {
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
        let branch_tooltip = match self.checkout_path.as_ref() {
            Some(path) if self.worktree_name.is_some() => format!(
                "{}\n{}",
                branch_tooltip,
                t!("workspace.worktree.context.tooltip", path = path)
            )
            .into(),
            Some(path) => format!("{branch_tooltip}\n{path}").into(),
            None => branch_tooltip,
        };
        let repo_tooltip = self
            .checkout_path
            .clone()
            .unwrap_or_else(|| self.repo_name.clone());

        let repo_value = div().h_flex().min_w_0().max_w(px(220.)).gap(px(5.)).child(
            Label::new(self.repo_name.clone())
                .weight(gpui::FontWeight::SEMIBOLD)
                .truncate(),
        );

        let branch_value = div()
            .h_flex()
            .min_w_0()
            .max_w(px(if self.worktree_name.is_some() {
                360.
            } else {
                280.
            }))
            .gap(px(5.))
            .child(
                Icon::new(IconName::GitBranch)
                    .size(IconSize::Small)
                    .color(branch_color),
            )
            .child(
                Label::new(self.branch_name.clone())
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

        div()
            .h_flex()
            .min_w_0()
            .gap(px(2.))
            .child(Self::crumb(
                "breadcrumb-repository",
                t!("title_bar.repository"),
                repo_value,
                repo_tooltip,
                self.on_repo_click,
                cx,
            ))
            .child(
                Icon::new(IconName::ChevronRight)
                    .size(IconSize::Small)
                    .color(Color::Placeholder),
            )
            .child(Self::crumb(
                "breadcrumb-branch",
                branch_caption,
                branch_value,
                branch_tooltip,
                self.on_branch_click,
                cx,
            ))
            .when_some(
                self.on_exit_worktree
                    .filter(|_| self.worktree_name.is_some()),
                |el, handler| {
                    el.child(
                        Button::new("exit-worktree-inspection", t!("workspace.worktree.exit"))
                            .size(ButtonSize::Compact)
                            .style(ButtonStyle::Subtle)
                            .tooltip(t!("workspace.worktree.exit.tooltip"))
                            .on_click(handler),
                    )
                },
            )
            .when_some(self.repo_state_label, |el, state_label| {
                el.child(
                    div()
                        .h_flex()
                        .h(px(22.))
                        .min_h(gpui::rems(1.375))
                        .flex_shrink_0()
                        .ml(px(4.))
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
                )
            })
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
