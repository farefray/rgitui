use gpui::prelude::*;
use gpui::SharedString;
use gpui::{div, px, ClickEvent, Context, EventEmitter, Render, Window};
use rgitui_i18n::{key, t, Message};
use rgitui_settings::SettingsState;
use rgitui_theme::{contrast_text_on, ActiveTheme, Color, StyledExt};
use rgitui_ui::{Icon, IconButton, IconName, IconSize, Label, LabelSize, Tooltip, VerticalDivider};

use crate::keymap;
use crate::CommandId;

type TooltipFactory = Box<dyn Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView>;

struct ToolbarButtonState {
    disabled: bool,
    loading: bool,
    /// Tooltip text, already translated.
    tooltip_text: SharedString,
    /// The command the button runs, if it is one a keystroke can reach. The
    /// tooltip shows that command's current binding rather than a literal.
    command: Option<CommandId>,
    /// A count drawn on the icon's corner (commits to pull or push).
    count: Option<(usize, Color)>,
}

impl ToolbarButtonState {
    fn new(tooltip_text: SharedString, command: Option<CommandId>) -> Self {
        Self {
            disabled: false,
            loading: false,
            tooltip_text,
            command,
            count: None,
        }
    }

    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    fn count(mut self, count: usize, color: Color) -> Self {
        self.count = (count > 0).then_some((count, color));
        self
    }
}

/// Events emitted by the toolbar.
#[derive(Debug, Clone)]
pub enum ToolbarEvent {
    Undo,
    Fetch,
    Pull,
    Push,
    Branch,
    StashSave,
    StashPop,
    CreatePr,
    Refresh,
    Settings,
    Search,
    OpenFileExplorer,
    OpenTerminal,
    OpenEditor,
}

/// The main toolbar with quick action buttons.
pub struct Toolbar {
    can_push: bool,
    can_pull: bool,
    has_stashes: bool,
    has_changes: bool,
    is_fetching: bool,
    is_pulling: bool,
    is_pushing: bool,
    has_github_token: bool,
    ahead: usize,
    behind: usize,
    /// What Undo would reverse, or `None` when there is nothing to undo.
    undo: Option<Message>,
}

impl EventEmitter<ToolbarEvent> for Toolbar {}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Toolbar {
    pub fn new() -> Self {
        Self {
            can_push: true,
            can_pull: true,
            has_stashes: false,
            has_changes: false,
            is_fetching: false,
            is_pulling: false,
            is_pushing: false,
            has_github_token: false,
            ahead: 0,
            behind: 0,
            undo: None,
        }
    }

    pub fn set_state(
        &mut self,
        can_push: bool,
        can_pull: bool,
        has_stashes: bool,
        has_changes: bool,
        has_github_token: bool,
        cx: &mut Context<Self>,
    ) {
        if self.can_push == can_push
            && self.can_pull == can_pull
            && self.has_stashes == has_stashes
            && self.has_changes == has_changes
            && self.has_github_token == has_github_token
        {
            return;
        }
        self.can_push = can_push;
        self.can_pull = can_pull;
        self.has_stashes = has_stashes;
        self.has_changes = has_changes;
        self.has_github_token = has_github_token;
        cx.notify();
    }

    pub fn set_ahead_behind(&mut self, ahead: usize, behind: usize, cx: &mut Context<Self>) {
        if self.ahead == ahead && self.behind == behind {
            return;
        }
        self.ahead = ahead;
        self.behind = behind;
        cx.notify();
    }

    /// The workspace's undo history is shared by every tab; it hands the
    /// active toolbar the label of the entry Undo would reverse.
    pub fn set_undo(&mut self, undo: Option<Message>, cx: &mut Context<Self>) {
        if self.undo == undo {
            return;
        }
        self.undo = undo;
        cx.notify();
    }

    pub fn set_fetching(&mut self, fetching: bool, cx: &mut Context<Self>) {
        if self.is_fetching == fetching {
            return;
        }
        self.is_fetching = fetching;
        cx.notify();
    }

    pub fn set_pulling(&mut self, pulling: bool, cx: &mut Context<Self>) {
        if self.is_pulling == pulling {
            return;
        }
        self.is_pulling = pulling;
        cx.notify();
    }

    pub fn set_pushing(&mut self, pushing: bool, cx: &mut Context<Self>) {
        if self.is_pushing == pushing {
            return;
        }
        self.is_pushing = pushing;
        cx.notify();
    }

    fn fetch_enabled(&self) -> bool {
        !self.is_fetching
    }

    fn pull_enabled(&self) -> bool {
        self.can_pull && !self.is_pulling
    }

    fn push_enabled(&self) -> bool {
        self.can_push && !self.is_pushing
    }

    fn build_tooltip(tooltip_text: SharedString, command: Option<CommandId>) -> TooltipFactory {
        match command {
            Some(command) => Box::new(keymap::command_tooltip(tooltip_text, command)),
            None => Box::new(Tooltip::text(tooltip_text)),
        }
    }

    /// A labelled action: icon above a short label, borderless until hovered.
    ///
    /// Hand-rolled rather than the shared `Button`, which cannot stack its
    /// label under the icon, draw a count on the icon, or keep the accent icon
    /// of a running (non-interactive) operation instead of the disabled color.
    fn action_button(
        &self,
        id: &'static str,
        icon: IconName,
        label: SharedString,
        state: ToolbarButtonState,
        height: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let hover_bg = cx.colors().ghost_element_hover;
        let active_bg = cx.colors().ghost_element_active;

        let is_inactive = state.disabled || state.loading;
        let icon_color = if state.loading {
            Color::Accent
        } else if state.disabled {
            Color::Disabled
        } else {
            Color::Default
        };
        let text_color = if state.loading {
            Color::Muted
        } else if state.disabled {
            Color::Disabled
        } else {
            Color::Default
        };

        let tooltip_fn = Self::build_tooltip(state.tooltip_text, state.command);
        let count = state
            .count
            .filter(|_| !state.loading)
            .map(|(count, color)| {
                let background = color.color(cx);
                div()
                    .absolute()
                    .top(px(-6.))
                    .left(px(11.))
                    .h(px(15.))
                    .min_w(px(15.))
                    .px(px(4.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(background)
                    .text_color(contrast_text_on(background))
                    .text_size(px(10.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(SharedString::from(count.to_string()))
            });

        div()
            .id(id)
            .v_flex()
            .h(px(height))
            .min_w(px(54.))
            .flex_shrink_0()
            .px(px(8.))
            .gap(px(4.))
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .when(!is_inactive, move |el| {
                el.hover(move |s| s.bg(hover_bg))
                    .active(move |s| s.bg(active_bg))
                    .cursor_pointer()
            })
            .tooltip(move |window, cx| tooltip_fn(window, cx))
            .child(
                div()
                    .relative()
                    .child(
                        Icon::new(icon)
                            .size(IconSize::Custom(gpui::rems(1.125)))
                            .color(icon_color),
                    )
                    .children(count),
            )
            .child(
                Label::new(label)
                    .size(LabelSize::XSmall)
                    .weight(gpui::FontWeight::MEDIUM)
                    .color(text_color),
            )
    }

    fn render_actions(&mut self, height: f32, cx: &mut Context<Self>) -> gpui::Div {
        let fetch_label = if self.is_fetching {
            t!("toolbar.fetching")
        } else {
            t!("toolbar.fetch")
        };
        let pull_label = if self.is_pulling {
            t!("toolbar.pulling")
        } else {
            t!("toolbar.pull")
        };
        let push_label = if self.is_pushing {
            t!("toolbar.pushing")
        } else {
            t!("toolbar.push")
        };
        let undo_tooltip = match &self.undo {
            Some(label) => t!("toolbar.undo.tooltip", action = label.translated()),
            None => t!("workspace.undo.nothing"),
        };

        div()
            .h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(2.))
            .child(
                self.action_button(
                    "tb-undo",
                    IconName::Undo,
                    t!("toolbar.undo"),
                    ToolbarButtonState::new(undo_tooltip, Some(CommandId::Undo))
                        .disabled(self.undo.is_none()),
                    height,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if this.undo.is_some() {
                        cx.emit(ToolbarEvent::Undo);
                    }
                })),
            )
            .child(VerticalDivider::new())
            // Network operations group
            .child(
                self.action_button(
                    "tb-fetch",
                    IconName::Refresh,
                    fetch_label,
                    ToolbarButtonState::new(
                        rgitui_i18n::translate(key!("common.fetch_remote.tooltip")),
                        Some(CommandId::Fetch),
                    )
                    .disabled(!self.fetch_enabled())
                    .loading(self.is_fetching),
                    height,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if this.fetch_enabled() {
                        cx.emit(ToolbarEvent::Fetch);
                    }
                })),
            )
            .child(
                self.action_button(
                    "tb-pull",
                    IconName::ArrowDown,
                    pull_label,
                    ToolbarButtonState::new(
                        rgitui_i18n::translate(key!("common.pull_remote.tooltip")),
                        Some(CommandId::Pull),
                    )
                    .disabled(!self.pull_enabled())
                    .loading(self.is_pulling)
                    .count(self.behind, Color::Info),
                    height,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if this.pull_enabled() {
                        cx.emit(ToolbarEvent::Pull);
                    }
                })),
            )
            .child(
                self.action_button(
                    "tb-push",
                    IconName::ArrowUp,
                    push_label,
                    ToolbarButtonState::new(
                        rgitui_i18n::translate(key!("common.push_remote.tooltip")),
                        Some(CommandId::Push),
                    )
                    .disabled(!self.push_enabled())
                    .loading(self.is_pushing)
                    .count(self.ahead, Color::Success),
                    height,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if this.push_enabled() {
                        cx.emit(ToolbarEvent::Push);
                    }
                })),
            )
            .child(VerticalDivider::new())
            // Branch and stash operations group
            .child(
                self.action_button(
                    "tb-branch",
                    IconName::GitBranch,
                    t!("toolbar.branch"),
                    ToolbarButtonState::new(
                        t!("toolbar.branch.tooltip"),
                        Some(CommandId::CreateBranch),
                    ),
                    height,
                    cx,
                )
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ToolbarEvent::Branch))),
            )
            .child(
                self.action_button(
                    "tb-stash",
                    IconName::Stash,
                    t!("toolbar.stash"),
                    ToolbarButtonState::new(
                        t!("toolbar.stash.tooltip"),
                        Some(CommandId::StashSave),
                    )
                    .disabled(!self.has_changes),
                    height,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if this.has_changes {
                        cx.emit(ToolbarEvent::StashSave);
                    }
                })),
            )
            .child(
                self.action_button(
                    "tb-pop",
                    IconName::StashPop,
                    t!("toolbar.pop"),
                    ToolbarButtonState::new(t!("toolbar.pop.tooltip"), Some(CommandId::StashPop))
                        .disabled(!self.has_stashes),
                    height,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if this.has_stashes {
                        cx.emit(ToolbarEvent::StashPop);
                    }
                })),
            )
            .child(VerticalDivider::new())
            // PR creation and the terminal: leaving the app for related work.
            .child(
                self.action_button(
                    "tb-pr",
                    IconName::GitPullRequest,
                    t!("toolbar.create_pr"),
                    ToolbarButtonState::new(
                        t!("toolbar.create_pr.tooltip"),
                        Some(CommandId::CreatePr),
                    )
                    .disabled(!self.has_github_token),
                    height,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if this.has_github_token {
                        cx.emit(ToolbarEvent::CreatePr);
                    }
                })),
            )
            .child(
                self.action_button(
                    "tb-terminal",
                    IconName::Terminal,
                    t!("toolbar.terminal"),
                    ToolbarButtonState::new(t!("toolbar.terminal.tooltip"), None),
                    height,
                    cx,
                )
                .on_click(
                    cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ToolbarEvent::OpenTerminal)),
                ),
            )
    }

    fn render_utilities(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(2.))
            .child(
                IconButton::new("tb-explorer", IconName::Folder)
                    .color(Color::Muted)
                    .tooltip(t!("toolbar.explorer.tooltip"))
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                        cx.emit(ToolbarEvent::OpenFileExplorer)
                    })),
            )
            .child(
                IconButton::new("tb-editor", IconName::ExternalLink)
                    .color(Color::Muted)
                    .tooltip(t!("toolbar.editor.tooltip"))
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ToolbarEvent::OpenEditor)),
                    ),
            )
            .child(VerticalDivider::new())
            .child(
                IconButton::new("tb-search", IconName::Search)
                    .color(Color::Muted)
                    .tooltip_fn(keymap::command_tooltip(
                        t!("toolbar.search_commits.tooltip"),
                        CommandId::Search,
                    ))
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ToolbarEvent::Search)),
                    ),
            )
            .child(
                IconButton::new("tb-refresh", IconName::Refresh)
                    .color(Color::Muted)
                    .tooltip_fn(keymap::command_tooltip(
                        t!("toolbar.refresh.tooltip"),
                        CommandId::Refresh,
                    ))
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ToolbarEvent::Refresh)),
                    ),
            )
            .child(
                IconButton::new("tb-settings", IconName::Settings)
                    .color(Color::Muted)
                    .tooltip_fn(keymap::command_tooltip(
                        t!("toolbar.settings.tooltip"),
                        CommandId::Settings,
                    ))
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ToolbarEvent::Settings)),
                    ),
            )
    }
}

impl Render for Toolbar {
    /// The actions part of the toolbar row. The workspace draws the row itself
    /// (surface, border and height) and puts the checkout breadcrumb before
    /// this; the actions are centered in the space that leaves.
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compactness = cx.global::<SettingsState>().settings().compactness;
        let button_h = compactness.spacing(46.0).max(40.0);

        let actions = self.render_actions(button_h, cx);
        let utilities = self.render_utilities(cx);

        div()
            .id("repository-toolbar")
            .h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            // Keep one action row; narrow windows can scroll to utilities.
            .overflow_x_scroll()
            .gap(px(12.))
            .items_center()
            .child(div().flex_1())
            .child(actions)
            .child(div().h_flex().flex_1().justify_end().child(utilities))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_actions_respect_availability_and_loading() {
        let mut toolbar = Toolbar::new();
        assert!(toolbar.fetch_enabled());
        assert!(toolbar.pull_enabled());
        assert!(toolbar.push_enabled());

        toolbar.can_pull = false;
        toolbar.can_push = false;
        assert!(!toolbar.pull_enabled());
        assert!(!toolbar.push_enabled());
        assert!(toolbar.fetch_enabled());

        toolbar.can_pull = true;
        toolbar.can_push = true;
        toolbar.is_fetching = true;
        toolbar.is_pulling = true;
        toolbar.is_pushing = true;
        assert!(!toolbar.fetch_enabled());
        assert!(!toolbar.pull_enabled());
        assert!(!toolbar.push_enabled());
    }

    #[test]
    fn counts_are_drawn_only_when_there_is_something_to_count() {
        let state = ToolbarButtonState::new("Push".into(), None);
        assert_eq!(state.count, None);
        let state = ToolbarButtonState::new("Push".into(), None).count(0, Color::Success);
        assert_eq!(state.count, None);
        let state = ToolbarButtonState::new("Push".into(), None).count(3, Color::Success);
        assert_eq!(state.count, Some((3, Color::Success)));
    }

    #[test]
    fn test_toolbar_event_debug() {
        let event = ToolbarEvent::Undo;
        assert_eq!(format!("{:?}", event), "Undo");

        let event = ToolbarEvent::Fetch;
        assert_eq!(format!("{:?}", event), "Fetch");

        let event = ToolbarEvent::Pull;
        assert_eq!(format!("{:?}", event), "Pull");

        let event = ToolbarEvent::Push;
        assert_eq!(format!("{:?}", event), "Push");

        let event = ToolbarEvent::Branch;
        assert_eq!(format!("{:?}", event), "Branch");

        let event = ToolbarEvent::StashSave;
        assert_eq!(format!("{:?}", event), "StashSave");

        let event = ToolbarEvent::StashPop;
        assert_eq!(format!("{:?}", event), "StashPop");

        let event = ToolbarEvent::CreatePr;
        assert_eq!(format!("{:?}", event), "CreatePr");

        let event = ToolbarEvent::Refresh;
        assert_eq!(format!("{:?}", event), "Refresh");

        let event = ToolbarEvent::Settings;
        assert_eq!(format!("{:?}", event), "Settings");

        let event = ToolbarEvent::Search;
        assert_eq!(format!("{:?}", event), "Search");

        let event = ToolbarEvent::OpenFileExplorer;
        assert_eq!(format!("{:?}", event), "OpenFileExplorer");

        let event = ToolbarEvent::OpenTerminal;
        assert_eq!(format!("{:?}", event), "OpenTerminal");

        let event = ToolbarEvent::OpenEditor;
        assert_eq!(format!("{:?}", event), "OpenEditor");
    }

    #[test]
    fn test_toolbar_event_match() {
        let event = ToolbarEvent::Push;
        if let ToolbarEvent::Push = event {
            // expected
        } else {
            panic!("Expected Push");
        }
    }
}
