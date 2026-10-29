use std::rc::Rc;

use gpui::prelude::*;
use gpui::{div, px, Context, ElementId, Render, WeakEntity, Window};
use rgitui_theme::StyledExt;
use rgitui_ui::Toast;

pub use rgitui_ui::ToastLevel as ToastKind;

/// Internal state for a single active toast.
struct ToastEntry {
    id: usize,
    message: String,
    kind: ToastKind,
    /// An optional single action, e.g. `[Open Settings]` or `[Retry]`.
    action: Option<(String, Rc<rgitui_ui::ClickHandler>)>,
}

/// The only placement container for operation cards and general toasts.
/// Absolute positioning keeps feedback out of workspace layout; a bounded,
/// scrollable stack keeps large output/errors from covering the whole window.
/// Only the cards occlude input, not a full-window transparent backdrop.
pub(crate) fn notification_stack() -> gpui::Stateful<gpui::Div> {
    div()
        .id("workspace-notifications")
        .absolute()
        .bottom(gpui::rems(2.5))
        .right(px(12.))
        .v_flex()
        .gap(px(6.))
        .w(px(440.))
        .max_w(gpui::relative(0.9))
        .max_h(gpui::relative(0.6))
        .overflow_y_scroll()
        .p(px(4.))
}

/// Manages transient toasts rendered in the workspace's shared notification stack.
pub struct ToastLayer {
    toasts: Vec<ToastEntry>,
    next_id: usize,
}

impl ToastLayer {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 0,
        }
    }

    /// Show a new toast notification.
    ///
    /// How long it stays is decided by its level: an error is sticky, because
    /// one hardcoded three-second timeout meant errors vanished before they
    /// could be read.
    pub fn show_toast(
        &mut self,
        message: impl Into<String>,
        kind: ToastKind,
        cx: &mut Context<Self>,
    ) {
        self.push(message.into(), kind, None, cx);
    }

    /// Show a toast carrying a single action.
    pub fn show_toast_with_action(
        &mut self,
        message: impl Into<String>,
        kind: ToastKind,
        action_label: impl Into<String>,
        on_action: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let action = Some((
            action_label.into(),
            Rc::new(Box::new(on_action) as rgitui_ui::ClickHandler),
        ));
        self.push(message.into(), kind, action, cx);
    }

    fn push(
        &mut self,
        message: String,
        kind: ToastKind,
        action: Option<(String, Rc<rgitui_ui::ClickHandler>)>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_id;
        self.next_id += 1;

        self.toasts.push(ToastEntry {
            id,
            message,
            kind,
            action,
        });

        while self.toasts.len() > 3 {
            self.toasts.remove(0);
        }

        if let Some(after) = kind.auto_dismiss_after() {
            cx.spawn(
                async move |this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                    cx.background_executor().timer(after).await;
                    this.update(cx, |this, cx| {
                        this.dismiss_toast(id, cx);
                    })
                    .ok();
                },
            )
            .detach();
        }

        cx.notify();
    }

    fn dismiss_toast(&mut self, id: usize, cx: &mut Context<Self>) {
        self.toasts.retain(|t| t.id != id);
        cx.notify();
    }
}

impl Render for ToastLayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.toasts.is_empty() {
            return div().into_any_element();
        }

        let mut stack = div().v_flex().w_full().flex_shrink_0().gap(px(6.));

        for entry in &self.toasts {
            let toast_id = entry.id;
            let entity = cx.entity().downgrade();

            let mut toast = Toast::new(
                ElementId::NamedInteger("toast".into(), toast_id as u64),
                entry.message.clone(),
                entry.kind,
            );
            if let Some((label, handler)) = &entry.action {
                let handler = handler.clone();
                let dismiss_entity = cx.entity().downgrade();
                toast = toast.action(label.clone(), move |event, window, cx| {
                    handler(event, window, cx);
                    // Acting on a toast is also dismissing it; leaving it on
                    // screen would invite a second, now-pointless click.
                    dismiss_entity
                        .update(cx, |this, cx| this.dismiss_toast(toast_id, cx))
                        .ok();
                });
            }

            stack = stack.child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .occlude()
                    .child(toast.on_dismiss(move |_, _, cx| {
                        entity
                            .update(cx, |this, cx| this.dismiss_toast(toast_id, cx))
                            .ok();
                    })),
            );
        }

        stack.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgitui_theme::Color;
    use rgitui_ui::IconName;

    fn entry(id: usize, message: &str, kind: ToastKind) -> ToastEntry {
        ToastEntry {
            id,
            message: message.to_string(),
            kind,
            action: None,
        }
    }

    #[test]
    fn notification_stack_never_participates_in_workspace_flow() {
        let mut stack = notification_stack();
        assert_eq!(stack.style().position, Some(gpui::Position::Absolute));
        assert!(stack.style().max_size.height.is_some());
        assert!(stack.style().max_size.width.is_some());
    }

    #[test]
    fn a_toast_entry_carries_its_message_and_level() {
        let success = entry(0, "test message", ToastKind::Success);
        assert_eq!(success.message, "test message");
        assert_eq!(success.kind, ToastKind::Success);
        assert!(success.action.is_none());

        assert_eq!(
            entry(1, "error occurred", ToastKind::Error).kind,
            ToastKind::Error
        );
    }

    /// A three-second timeout on every level meant errors vanished before
    /// they could be read, and a "Generating..." notice expired mid-operation
    /// on a tool-calling generation that runs 30s or more.
    #[test]
    fn errors_stay_until_dismissed_while_lesser_levels_expire() {
        assert_eq!(ToastKind::Error.auto_dismiss_after(), None);
        assert_eq!(
            ToastKind::Warning.auto_dismiss_after(),
            Some(std::time::Duration::from_secs(6))
        );
        assert_eq!(
            ToastKind::Info.auto_dismiss_after(),
            Some(std::time::Duration::from_secs(3))
        );
        assert_eq!(
            ToastKind::Success.auto_dismiss_after(),
            Some(std::time::Duration::from_secs(3))
        );
    }

    #[test]
    fn a_warning_lingers_longer_than_an_info() {
        let warning = ToastKind::Warning.auto_dismiss_after().unwrap();
        let info = ToastKind::Info.auto_dismiss_after().unwrap();
        assert!(warning > info);
    }

    #[test]
    fn every_level_maps_to_a_distinct_colour_and_icon() {
        assert_eq!(ToastKind::Success.color(), Color::Success);
        assert_eq!(ToastKind::Error.color(), Color::Error);
        assert_eq!(ToastKind::Warning.color(), Color::Warning);
        assert_eq!(ToastKind::Info.color(), Color::Info);

        assert_eq!(ToastKind::Success.icon(), IconName::CheckCircle);
        assert_eq!(ToastKind::Error.icon(), IconName::XCircle);
        assert_eq!(ToastKind::Warning.icon(), IconName::AlertTriangle);
        assert_eq!(ToastKind::Info.icon(), IconName::Info);
    }
}
