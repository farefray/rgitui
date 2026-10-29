use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{div, px, ClickEvent, Context, SharedString};
use rgitui_git::{GitOperationKind, GitOperationUpdate};
use rgitui_i18n::t;
use rgitui_theme::{ActiveTheme, Color, StyledExt};
use rgitui_ui::{
    ButtonSize, ButtonStyle, Icon, IconButton, IconName, IconSize, Label, LabelSize, Tooltip,
};

use crate::ToastKind;

use super::{
    GitOperationTarget, OperationOutput, RepositoryTarget, Workspace,
    OPERATION_OUTPUT_AUTO_HIDE_SECS,
};

impl OperationOutput {
    /// The timestamp identifies a particular collapsed lifetime, not just a result.
    fn should_auto_hide(&self, scheduled_timestamp: Instant) -> bool {
        self.timestamp == scheduled_timestamp && !self.expanded && !self.is_error
    }

    /// Expanding pauses expiry. Collapsing starts a fresh full timeout, even if
    /// the previous timer already fired while the details were open.
    fn toggle_expanded(&mut self, now: Instant) -> Option<Instant> {
        self.expanded = !self.expanded;
        if self.expanded {
            return None;
        }

        // Also invalidate the old timer on clocks with coarse resolution.
        self.timestamp = now.max(self.timestamp + Duration::from_nanos(1));
        (!self.is_error).then_some(self.timestamp)
    }
}

impl Workspace {
    /// Legacy rebase/bisect/previous-branch APIs still assume the owning checkout.
    /// Refuse them in inspection instead of silently changing another checkout.
    pub(super) fn require_main_checkout(&mut self, cx: &mut Context<Self>) -> bool {
        if self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| tab.inspecting_worktree.is_some())
        {
            self.show_toast(
                rgitui_i18n::t!("workspace.worktree.main_only"),
                ToastKind::Warning,
                cx,
            );
            false
        } else {
            true
        }
    }

    /// Capture the origin before showing the dialog: GPUI events are delivered
    /// later, when another repository or checkout may already be selected.
    pub(crate) fn show_confirmation(
        &mut self,
        action: crate::ConfirmAction,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        let target = crate::confirm_dialog::ConfirmationTarget {
            project_id: tab.project.entity_id(),
            worktree_path: tab.effective_repo_path(cx),
        };
        self.dialogs.confirm_dialog.update(cx, |dialog, cx| {
            dialog.show_visible(action, target, cx);
        });
    }

    /// Unbound editors must not survive a context change and submit into another
    /// checkout. Destructive confirmations retain an explicit origin instead.
    pub(super) fn dismiss_checkout_dialogs(&mut self, cx: &mut Context<Self>) {
        self.dialogs.branch_dialog.update(cx, |dialog, cx| {
            if dialog.is_visible() {
                dialog.dismiss(cx);
            }
        });
        self.dialogs.tag_dialog.update(cx, |dialog, cx| {
            if dialog.is_visible() {
                dialog.dismiss(cx);
            }
        });
        self.dialogs.rename_dialog.update(cx, |dialog, cx| {
            if dialog.is_visible() {
                dialog.dismiss(cx);
            }
        });
        self.dialogs.stash_save_dialog.update(cx, |dialog, cx| {
            if dialog.is_visible() {
                dialog.dismiss(cx);
            }
        });
        self.dialogs.stash_branch_dialog.update(cx, |dialog, cx| {
            if dialog.is_visible() {
                dialog.dismiss(cx);
            }
        });
        self.dialogs.worktree_dialog.update(cx, |dialog, cx| {
            if dialog.is_visible() {
                dialog.dismiss(cx);
            }
        });
        self.overlays.interactive_rebase.update(cx, |dialog, cx| {
            if dialog.is_visible() {
                dialog.dismiss(cx);
            }
        });
        self.dialogs.create_pr_dialog.update(cx, |dialog, cx| {
            if dialog.is_visible() {
                dialog.cancel(cx);
            }
        });
    }

    /// Navigation must refuse to close a project while a mutation is running.
    pub(super) fn project_has_active_operations(&self, project_id: gpui::EntityId) -> bool {
        self.operations
            .active_operations
            .iter()
            .any(|op| op.project_id == project_id)
    }

    /// Resolve only a live original project and a checkout still in its snapshot.
    /// Never fall back to the active tab or a newly opened copy of the same path.
    pub(super) fn resolve_repository_target(
        &self,
        target: &RepositoryTarget,
        cx: &gpui::App,
    ) -> Option<usize> {
        self.tabs.iter().position(|tab| {
            if tab.project.entity_id() != target.project_id {
                return false;
            }
            let project = tab.project.read(cx);
            project.repo_path() == target.worktree_path
                || project.worktree_at(&target.worktree_path).is_some()
        })
    }

    pub(super) fn schedule_operation_output_auto_hide(
        &mut self,
        timestamp: Instant,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(
                    OPERATION_OUTPUT_AUTO_HIDE_SECS,
                ))
                .await;
            cx.update(|cx| {
                this.update(cx, |this, cx| {
                    if let Some(output) = &this.operations.last_operation_output {
                        if output.should_auto_hide(timestamp) {
                            this.operations.last_operation_output = None;
                            cx.notify();
                        }
                    }
                })
                .ok();
            })
        })
        .detach();
    }

    pub(super) fn render_operation_output_notification(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let output = self.operations.last_operation_output.as_ref()?;
        let colors = cx.colors().clone();

        let accent = if output.is_error {
            cx.status().error
        } else {
            cx.status().success
        };
        let icon = if output.is_error {
            IconName::FileConflict
        } else {
            IconName::Check
        };
        let icon_color = if output.is_error {
            Color::Error
        } else {
            Color::Success
        };

        let operation_label = output.operation.translated();
        let full_output = output.output.translated();
        let first_line: SharedString = full_output.lines().next().unwrap_or("").to_string().into();
        let expandable = !full_output.is_empty();
        let expanded = expandable && output.expanded;

        let mut card = div()
            .id("operation-output-notification")
            .v_flex()
            .w_full()
            .flex_shrink_0()
            .occlude()
            .bg(colors.elevated_surface_background)
            .border_1()
            .rounded(px(8.))
            .border_color(accent)
            .elevation_3(cx);

        let mut header = div()
            .h_flex()
            .w_full()
            .min_h(px(30.))
            .px(px(10.))
            .py(px(4.))
            .gap(px(6.))
            .items_center()
            .child(Icon::new(icon).size(IconSize::XSmall).color(icon_color))
            .child(
                div()
                    .id("operation-output-summary")
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .tooltip(Tooltip::text(operation_label.clone()))
                    .child(
                        Label::new(operation_label)
                            .size(LabelSize::Small)
                            .weight(gpui::FontWeight::SEMIBOLD)
                            .truncate(),
                    )
                    .child(
                        Label::new(first_line)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                            .truncate(),
                    ),
            );

        if expandable {
            header = header.child(
                IconButton::new(
                    "output-expand",
                    if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                )
                .size(ButtonSize::Compact)
                .style(ButtonStyle::Transparent)
                .color(Color::Muted)
                .tooltip(if expanded {
                    t!("workspace.operation.hide_details")
                } else {
                    t!("workspace.operation.show_details")
                })
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    let restart_at = this
                        .operations
                        .last_operation_output
                        .as_mut()
                        .and_then(|output| output.toggle_expanded(Instant::now()));
                    if let Some(timestamp) = restart_at {
                        this.schedule_operation_output_auto_hide(timestamp, cx);
                    }
                    cx.notify();
                })),
            );
        }

        header = header.child(
            IconButton::new("output-dismiss", IconName::X)
                .size(ButtonSize::Compact)
                .style(ButtonStyle::Transparent)
                .color(Color::Muted)
                .tooltip(t!("workspace.operation.dismiss"))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.operations.last_operation_output = None;
                    cx.notify();
                })),
        );

        card = card.child(header);

        if expanded {
            card = card.child(
                div()
                    .id("operation-output-content")
                    .w_full()
                    .max_h(px(160.))
                    .overflow_scroll()
                    .px(px(10.))
                    .pb(px(6.))
                    .child(
                        div()
                            .font_family("Lilex")
                            .text_xs()
                            .text_color(colors.text_muted)
                            .whitespace_nowrap()
                            .child(full_output),
                    ),
            );
        }

        Some(card.into_any_element())
    }

    pub(super) fn retry_git_operation(
        &mut self,
        update: &GitOperationUpdate,
        target: &GitOperationTarget,
        cx: &mut Context<Self>,
    ) {
        if update.id != target.operation_id {
            return;
        }
        let Some(tab_index) = self.resolve_repository_target(&target.repository, cx) else {
            self.show_toast(
                rgitui_i18n::t!("workspace.safety.target_closed").to_string(),
                ToastKind::Warning,
                cx,
            );
            return;
        };
        let project = self.tabs[tab_index].project.clone();
        let worktree_path = target.repository.worktree_path.clone();
        // These legacy methods operate only on the main checkout.
        if matches!(
            update.kind,
            GitOperationKind::Checkout | GitOperationKind::Merge
        ) && project.read(cx).repo_path() != worktree_path
        {
            return;
        }

        let handled = project.update(cx, |proj, cx| match update.kind {
            GitOperationKind::Fetch => {
                if let Some(remote_name) = update.remote_name.as_deref() {
                    proj.fetch_at(remote_name, &worktree_path, cx).detach();
                    true
                } else {
                    proj.fetch_default_at(&worktree_path, cx).detach();
                    true
                }
            }
            GitOperationKind::Pull => {
                if let Some(remote_name) = update.remote_name.as_deref() {
                    proj.pull_at(remote_name, &worktree_path, cx).detach();
                    true
                } else {
                    proj.pull_default_at(&worktree_path, cx).detach();
                    true
                }
            }
            GitOperationKind::Push | GitOperationKind::ForcePush => {
                let force = update.kind == GitOperationKind::ForcePush;
                if let Some(remote_name) = update.remote_name.as_deref() {
                    proj.push_at(remote_name, force, &worktree_path, cx)
                        .detach();
                    true
                } else {
                    proj.push_default_at(force, &worktree_path, cx).detach();
                    true
                }
            }
            GitOperationKind::Checkout => {
                if let Some(branch_name) = update.branch_name.as_deref() {
                    proj.checkout_branch_at(branch_name, &worktree_path, cx)
                        .detach();
                    true
                } else {
                    false
                }
            }
            GitOperationKind::Merge => {
                if let Some(branch_name) = update.branch_name.as_deref() {
                    proj.merge_branch_at(branch_name, &worktree_path, cx)
                        .detach();
                    true
                } else {
                    false
                }
            }
            GitOperationKind::RemoveRemote => {
                if let Some(remote_name) = update.remote_name.as_deref() {
                    proj.remove_remote(remote_name, cx).detach();
                    true
                } else {
                    false
                }
            }
            _ => false,
        });

        if !handled {
            self.show_toast(
                "This operation cannot be retried automatically.",
                ToastKind::Info,
                cx,
            );
        } else if self.operations.last_failed_git_operation_target.as_ref() == Some(target) {
            self.operations.last_failed_git_operation = None;
            self.operations.last_failed_git_operation_target = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgitui_i18n::Message;

    fn success(timestamp: Instant) -> OperationOutput {
        OperationOutput {
            operation: Message::verbatim("Fetch completed in repository"),
            output: Message::verbatim("A single output line can also be expanded"),
            is_error: false,
            timestamp,
            expanded: false,
        }
    }

    #[test]
    fn only_the_current_collapsed_success_expires() {
        assert_eq!(OPERATION_OUTPUT_AUTO_HIDE_SECS, 10);
        let started = Instant::now();
        let mut output = success(started);
        assert!(output.should_auto_hide(started));

        // A previous result's timer must not dismiss a newer result.
        output.timestamp = started + Duration::from_secs(1);
        assert!(!output.should_auto_hide(started));
        assert!(output.should_auto_hide(output.timestamp));

        output.is_error = true;
        assert!(!output.should_auto_hide(output.timestamp));
    }

    #[test]
    fn collapse_restarts_timeout_after_original_timer_fired_while_expanded() {
        let started = Instant::now();
        let mut output = success(started);
        assert_eq!(output.toggle_expanded(started), None);
        assert!(output.expanded);
        // Simulate the original timer firing while details remain open.
        assert!(!output.should_auto_hide(started));

        let collapsed = started + Duration::from_secs(OPERATION_OUTPUT_AUTO_HIDE_SECS + 5);
        assert_eq!(output.toggle_expanded(collapsed), Some(collapsed));
        assert!(!output.expanded);
        assert_eq!(output.timestamp, collapsed);
        assert!(!output.should_auto_hide(started));
        assert!(output.should_auto_hide(collapsed));
    }

    #[test]
    fn rapid_expansion_cycles_invalidate_all_previous_timers() {
        let started = Instant::now();
        let mut output = success(started);
        assert_eq!(output.toggle_expanded(started), None);
        // Even two clicks in the same clock tick must create a new lifetime.
        let first_restart = output.toggle_expanded(started).unwrap();
        assert!(first_restart > started);
        assert!(!output.should_auto_hide(started));

        assert_eq!(output.toggle_expanded(first_restart), None);
        assert!(!output.should_auto_hide(first_restart));
        let second_restart = output.toggle_expanded(first_restart).unwrap();
        assert!(second_restart > first_restart);
        assert!(!output.should_auto_hide(started));
        assert!(!output.should_auto_hide(first_restart));
        assert!(output.should_auto_hide(second_restart));
    }

    #[test]
    fn collapsing_error_details_does_not_schedule_expiry() {
        let started = Instant::now();
        let mut output = success(started);
        output.is_error = true;
        assert_eq!(output.toggle_expanded(started), None);
        assert!(output.expanded);
        assert_eq!(
            output.toggle_expanded(started + Duration::from_secs(1)),
            None
        );
        assert!(!output.expanded);
        assert!(!output.should_auto_hide(output.timestamp));
    }
}
