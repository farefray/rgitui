use std::time::Instant;

use gpui::prelude::*;
use gpui::{div, px, ClickEvent, Context, SharedString};
use rgitui_git::{GitOperationKind, GitOperationUpdate};
use rgitui_theme::{ActiveTheme, Color, StyledExt};
use rgitui_ui::{ButtonSize, ButtonStyle, Icon, IconButton, IconName, IconSize, Label, LabelSize};

use crate::ToastKind;

use super::{GitOperationTarget, RepositoryTarget, Workspace, OPERATION_OUTPUT_AUTO_HIDE_SECS};

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
                        if output.timestamp == timestamp && !output.expanded && !output.is_error {
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

    pub(super) fn render_operation_output_bar(
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
        let bg = if output.is_error {
            cx.status().error_background
        } else {
            cx.status().success_background
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
        let expanded = output.expanded;
        let has_multiple_lines = full_output.lines().count() > 1;

        let mut bar = div()
            .id("operation-output-bar")
            .v_flex()
            .w_full()
            .bg(bg)
            .border_b_1()
            .border_color(accent);

        let mut header = div()
            .h_flex()
            .w_full()
            .min_h(px(28.))
            .px(px(10.))
            .py(px(2.))
            .gap(px(6.))
            .items_center()
            .child(Icon::new(icon).size(IconSize::XSmall).color(icon_color))
            .child(
                Label::new(operation_label)
                    .size(LabelSize::XSmall)
                    .weight(gpui::FontWeight::SEMIBOLD),
            )
            .child(
                div().flex_1().min_w_0().overflow_hidden().child(
                    Label::new(first_line)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .truncate(),
                ),
            );

        if has_multiple_lines {
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
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if let Some(op_output) = &mut this.operations.last_operation_output {
                        op_output.expanded = !op_output.expanded;
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
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.operations.last_operation_output = None;
                    cx.notify();
                })),
        );

        bar = bar.child(header);

        if expanded {
            bar = bar.child(
                div()
                    .id("operation-output-content")
                    .w_full()
                    .max_h(px(120.))
                    .overflow_y_scroll()
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

        Some(bar.into_any_element())
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
