use anyhow::Result;
use gpui::prelude::*;
use gpui::Context;
use rgitui_git::GitProject;
use rgitui_i18n::{t, Message};
use rgitui_settings::{LayoutSettings, StoredWorkspace};

use crate::command_palette::CommandContext;
use crate::ToastKind;

use super::{BottomPanelMode, ProjectTab, RightPanelMode, Workspace};

/// Keep the same selection when a preceding item is removed; closing the
/// selected item chooses its right neighbour, or the last remaining item.
fn selection_after_close(active: usize, closed: usize, len: usize) -> usize {
    if len <= 1 {
        0
    } else if closed < active {
        active - 1
    } else {
        active.min(len - 2)
    }
}

fn previous_after_activation<T: Copy + PartialEq>(
    current: Option<T>,
    target: T,
    previous: Option<T>,
) -> Option<T> {
    current.filter(|current| *current != target).or(previous)
}

/// Duplicate basenames need their parent path; unique names remain compact.
fn repository_names(paths: &[std::path::PathBuf], names: &[String]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if names.iter().filter(|other| *other == name).count() > 1 {
                format!(
                    "{} — {}",
                    name,
                    paths[index].parent().unwrap_or(&paths[index]).display()
                )
            } else {
                name.clone()
            }
        })
        .collect()
}

fn repository_label(name: &str, worktree: Option<&str>) -> String {
    match worktree {
        Some(worktree) => format!("{name} · {worktree}"),
        None => name.to_owned(),
    }
}

/// Identity comes from the already-open validation handle, not extra filesystem probes.
#[derive(Debug, PartialEq, Eq)]
struct RepositoryIdentity {
    checkout: std::path::PathBuf,
    common_dir: std::path::PathBuf,
    linked: bool,
}

impl RepositoryIdentity {
    fn from_repo(repo: &git2::Repository) -> Self {
        Self {
            checkout: repo.workdir().unwrap_or_else(|| repo.path()).to_path_buf(),
            common_dir: repo.commondir().to_path_buf(),
            linked: repo.is_worktree(),
        }
    }

    fn project_open_path(&self) -> &std::path::Path {
        if self.linked {
            &self.common_dir
        } else {
            &self.checkout
        }
    }
}

/// Keep unavailable slots in place, drop explicitly closed entries, append new repos.
fn merge_session_order(
    saved: &[std::path::PathBuf],
    live: &[std::path::PathBuf],
) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    for path in saved.iter().filter(|path| live.contains(path)).chain(live) {
        if !result.contains(path) {
            result.push(path.clone());
        }
    }
    result
}

fn session_active_index(repos: &[std::path::PathBuf], selected: Option<&std::path::Path>) -> usize {
    selected
        .and_then(|selected| repos.iter().position(|path| path == selected))
        .unwrap_or(0)
}

impl Workspace {
    pub(super) fn project_tab_index(&self, project: gpui::EntityId) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.project.entity_id() == project)
    }

    /// Every navigation entry point passes through here. Indices are only a
    /// presentation detail; delayed callbacks must use `activate_project`.
    pub(super) fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(target) = self.tabs.get(index).map(|tab| tab.project.entity_id()) else {
            return;
        };
        let current = self.active_project().map(|project| project.entity_id());
        if current != Some(target) {
            self.dismiss_checkout_dialogs(cx);
        }
        self.previous_project = previous_after_activation(current, target, self.previous_project);
        self.active_tab = index;
        self.repo_tab_scroll.scroll_to_item(index);
        super::events::update_sidebar_for_active_worktree(self, cx);
        self.update_command_context(cx);
        if !self.restoring_workspace && cx.try_global::<rgitui_settings::SettingsState>().is_some()
        {
            let path = self.tabs[index].project.read(cx).repo_path().to_path_buf();
            cx.global_mut::<rgitui_settings::SettingsState>()
                .add_recent_repo(path);
        }
        self.persist_workspace_snapshot(cx);
        cx.notify();
    }

    pub(super) fn activate_project(&mut self, project: gpui::EntityId, cx: &mut Context<Self>) {
        if let Some(index) = self.project_tab_index(project) {
            self.activate_tab(index, cx);
        }
    }

    pub(super) fn previous_repository(&mut self, cx: &mut Context<Self>) {
        if let Some(project) = self.previous_project {
            self.activate_project(project, cx);
        }
    }

    pub(super) fn open_repo_entries(
        &self,
        cx: &gpui::App,
    ) -> Vec<crate::repo_opener::OpenRepoEntry> {
        let paths: Vec<_> = self
            .tabs
            .iter()
            .map(|tab| tab.project.read(cx).repo_path().to_path_buf())
            .collect();
        let names = repository_names(
            &paths,
            &self
                .tabs
                .iter()
                .map(|tab| tab.name.clone())
                .collect::<Vec<_>>(),
        );
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let path = paths[index].clone();
                let worktree = tab
                    .inspecting_worktree
                    .as_ref()
                    .map(|worktree| worktree.name.clone());
                crate::repo_opener::OpenRepoEntry {
                    path,
                    name: names[index].clone(),
                    worktree,
                    active: index == self.active_tab,
                }
            })
            .collect()
    }

    pub(super) fn repo_tab_labels(&self, cx: &gpui::App) -> Vec<String> {
        self.open_repo_entries(cx)
            .iter()
            .map(|entry| repository_label(&entry.name, entry.worktree.as_deref()))
            .collect()
    }

    pub(super) fn toggle_repository_switcher(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.open_repo_entries(cx);
        self.overlays.repo_opener.update(cx, |opener, cx| {
            opener.set_open_repos(entries, cx);
            opener.toggle_switcher(window, cx);
        });
    }

    /// Open a repository as a new tab.
    pub fn open_repo(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) -> Result<()> {
        // Normalise UNC/WSL2 paths on Windows before any validation or open.
        let path = rgitui_git::normalize_repo_path(path);
        log::info!("open_repo: path={}", path.display());
        // Check if already open
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.project.read(cx).repo_path() == path)
        {
            log::debug!("open_repo: already open at tab {}", idx);
            self.activate_tab(idx, cx);
            return Ok(());
        }

        // Validate new paths before consulting cached worktrees: a cached row
        // can outlive its checkout. Reuse this handle's resolved metadata for
        // aliases and grouping, even before the first worktree refresh.
        let repo = git2::Repository::open(&path).map_err(|e| {
            anyhow::anyhow!("Failed to open repository at {}: {}", path.display(), e)
        })?;
        let identity = RepositoryIdentity::from_repo(&repo);
        if let Some(index) = self.tabs.iter().position(|tab| {
            tab.common_git_dir == identity.common_dir
                || tab.project.read(cx).repo_path() == identity.checkout
        }) {
            self.reconcile_opened_path(&path, index, cx);
            if identity.checkout != self.tabs[index].project.read(cx).repo_path() {
                self.inspect_validated_checkout(index, identity.checkout, cx);
            }
            self.activate_tab(index, cx);
            return Ok(());
        }
        // Opening the common git directory resolves the main checkout, including
        // linked-first opens. This replaces, rather than adds to, the existing
        // GitProject::open call.
        let project_path = identity.project_open_path().to_path_buf();

        let commit_limit = cx
            .try_global::<rgitui_settings::SettingsState>()
            .map(|s| s.settings().commit_limit)
            .unwrap_or(1000);

        let mut open_error: Option<String> = None;
        let project = cx.new(
            |cx| match GitProject::open(project_path.clone(), commit_limit, cx) {
                Ok(p) => p,
                Err(e) => {
                    log::error!("Failed to open git project at {}: {}", path.display(), e);
                    open_error = Some(format!("Failed to open repository: {}", e));
                    GitProject::empty_at(project_path.clone())
                }
            },
        );
        if let Some(err) = open_error {
            anyhow::bail!("{}", err);
        }
        // Refresh is orchestrated by the caller to prioritize the active tab.

        let graph = cx.new(rgitui_graph::GraphView::new);
        let diff_viewer = cx.new(rgitui_diff::DiffViewer::new);
        let blame_view = cx.new(crate::BlameView::new);
        let file_history_view = cx.new(crate::FileHistoryView::new);
        let reflog_view = cx.new(crate::ReflogView::new);
        let bisect_view = cx.new(crate::BisectView::new);
        let submodule_view = cx.new(crate::SubmoduleView::new);
        let detail_panel = cx.new(crate::DetailPanel::new);
        let sidebar = cx.new(crate::Sidebar::new);
        let commit_panel = cx.new(crate::CommitPanel::new);
        commit_panel.update(cx, |panel, cx| {
            panel.switch_checkout(project.read(cx).repo_path().to_path_buf(), cx);
        });
        let toolbar = cx.new(|_cx| crate::Toolbar::new());
        let global_search_view = cx.new(crate::GlobalSearchView::new);

        // Set the repo name on the sidebar header
        let repo_display_name = project.read(cx).repo_name().to_string();
        sidebar.update(cx, |s, cx| {
            s.set_repo_name(repo_display_name, cx);
        });

        let caches = super::ViewCaches::new();
        let diff_prefetch = super::events::DiffPrefetchScheduler::new(caches.diff.clone());

        // Drop routing subscriptions with the owning tab, not the workspace.
        let subscriptions = vec![
            // Also covers reopening a previously unavailable repository through
            // the opener, whose caller refreshes only that project.
            cx.subscribe(&project, |workspace, project, event, cx| {
                if matches!(
                    event,
                    rgitui_git::GitProjectEvent::RepositoryChanged
                        | rgitui_git::GitProjectEvent::StatusChanged
                ) {
                    workspace.restore_pending_worktree(project.entity_id(), cx);
                }
            }),
            super::events::subscribe_project(
                cx,
                super::events::ProjectSubscriptions {
                    project: &project,
                    graph: &graph,
                    sidebar: &sidebar,
                    diff_viewer: &diff_viewer,
                    detail_panel: &detail_panel,
                    toolbar: &toolbar,
                    diff_cache: caches.diff.clone(),
                    diff_prefetch: diff_prefetch.clone(),
                },
            ),
            super::events::subscribe_sidebar(cx, &project, &sidebar, &diff_viewer, &detail_panel),
            super::events::subscribe_graph(
                cx,
                &project,
                &graph,
                &diff_viewer,
                &detail_panel,
                caches.diff.clone(),
                diff_prefetch,
            ),
            super::events::subscribe_detail_panel(cx, &project, &diff_viewer, &detail_panel),
            super::events::subscribe_diff_viewer(cx, &project, &diff_viewer, caches.clone()),
            super::events::subscribe_commit_panel(cx, &project, &self.ai.clone(), &commit_panel),
            super::events::subscribe_toolbar(cx, &project, &toolbar),
            super::events::subscribe_blame_view(cx, &blame_view, &graph),
            super::events::subscribe_file_history_view(cx, &file_history_view, &graph),
            super::events::subscribe_reflog_view(cx, &project, &reflog_view, &graph),
            super::events::subscribe_bisect_view(cx, &project, &bisect_view, &graph),
            super::events::subscribe_submodule_view(cx, &submodule_view, &project),
            super::events::subscribe_global_search(cx, &global_search_view),
        ];

        // Initial sync
        {
            let (
                commits,
                has_more,
                init_status,
                branches,
                tags,
                remotes,
                stashes,
                worktrees,
                staged_count,
                authors,
                selected_worktree_path,
            ) = {
                let proj = project.read(cx);
                let commits = proj.recent_commits_arc();
                let has_more = proj.has_more_commits();
                let init_status = proj.status_arc();
                let branches = proj.branches().to_vec();
                let tags = proj.tags().to_vec();
                let remotes = proj.remotes().to_vec();
                let stashes = proj.stashes().to_vec();
                let worktrees = proj.worktrees().to_vec();
                let staged_count = init_status.staged.len();
                let mut seen = std::collections::HashSet::new();
                let authors: Vec<(String, String)> = commits
                    .iter()
                    .filter(|c| seen.insert(c.author.email.clone()))
                    .map(|c| (c.author.name.clone(), c.author.email.clone()))
                    .collect();
                (
                    commits,
                    has_more,
                    init_status,
                    branches,
                    tags,
                    remotes,
                    stashes,
                    worktrees,
                    staged_count,
                    authors,
                    proj.repo_path().to_path_buf(),
                )
            };
            crate::avatar_resolver::resolve_avatars(authors, cx);
            let worktree_graph_infos = super::events::build_worktree_graph_infos(&worktrees);
            let head_detached = project.read(cx).is_head_detached();
            graph.update(cx, |g, cx| {
                g.set_commits(commits, cx);
                g.set_head_detached(head_detached, cx);
                g.set_all_loaded(!has_more);
                g.set_worktree_statuses(worktree_graph_infos, cx);
            });

            sidebar.update(cx, |s, cx| {
                s.update_branches(branches, cx);
                s.update_tags(tags, cx);
                s.update_remotes(remotes, cx);
                s.update_stashes(stashes, cx);
                s.update_worktrees(worktrees, cx);
                s.update_status(init_status.staged.clone(), init_status.unstaged.clone(), cx);
                s.set_selected_worktree_by_path(Some(&selected_worktree_path), cx);
            });

            commit_panel.update(cx, |cp, cx| cp.set_staged_count(staged_count, cx));
        }

        // Compute ahead/behind for all branches in the background after the
        // initial fast refresh. This avoids blocking the first render with
        // expensive graph walks (particularly impactful on repos with many branches).
        project.update(cx, |proj, cx| {
            proj.refresh_ahead_behind(cx);
        });

        let github_data = self.github_data.clone();
        let issues_panel = cx.new({
            let github_data = github_data.clone();
            move |cx| crate::IssuesPanel::new(cx, github_data)
        });
        let workspace_weak = cx.entity().downgrade();
        let prs_workspace = workspace_weak.clone();
        let prs_panel =
            cx.new(move |cx| crate::PrsPanel::new(cx, prs_workspace, github_data.clone()));
        let project_weak = project.downgrade();
        let branch_health_panel =
            cx.new(|cx| crate::BranchHealthPanel::new(cx, project_weak.clone()));
        let stashes_panel =
            cx.new(|cx| crate::StashesPanel::new(cx, project_weak, workspace_weak.clone()));

        // Configure issues and PRs panels with GitHub remote info and token.
        self.configure_github_panels(&project, &issues_panel, &prs_panel, cx);

        let name = project.read(cx).repo_name().to_string();
        self.dismiss_checkout_dialogs(cx);
        self.tabs.push(ProjectTab {
            common_git_dir: identity.common_dir,
            name,
            project,
            graph,
            diff_viewer,
            blame_view,
            file_history_view,
            reflog_view,
            bisect_view,
            submodule_view,
            detail_panel,
            sidebar,
            commit_panel,
            toolbar,
            issues_panel,
            prs_panel,
            branch_health_panel,
            stashes_panel,
            global_search_view,
            right_panel_mode: RightPanelMode::Details,
            bottom_panel_mode: BottomPanelMode::Diff,
            caches,
            inspecting_worktree: None,
            _subscriptions: subscriptions,
        });
        let index = self.tabs.len() - 1;
        self.reconcile_opened_path(&path, index, cx);
        if identity.linked {
            self.inspect_validated_checkout(index, identity.checkout, cx);
        }
        // Reinsert a recovered repository at its saved slot, not at the end.
        let root = self.tabs[index].project.read(cx).repo_path();
        let slot = self.session_repo_order.iter().position(|path| path == root);
        let insert_at = slot
            .and_then(|slot| {
                self.tabs[..index].iter().position(|tab| {
                    self.session_repo_order
                        .iter()
                        .position(|path| path == tab.project.read(cx).repo_path())
                        .is_some_and(|other| other > slot)
                })
            })
            .unwrap_or(index);
        if insert_at != index {
            let tab = self.tabs.remove(index);
            self.tabs.insert(insert_at, tab);
            if self.active_tab >= insert_at {
                self.active_tab += 1;
            }
        }
        log::info!("open_repo: opened as tab {}", insert_at);
        self.activate_tab(insert_at, cx);
        Ok(())
    }

    fn reconcile_opened_path(&mut self, input: &std::path::Path, index: usize, cx: &Context<Self>) {
        let tab = &self.tabs[index];
        let root = tab.project.read(cx).repo_path().to_path_buf();
        for path in &mut self.session_repo_order {
            if path == input {
                *path = root.clone();
            }
        }
        if self.saved_active_repo_path.as_deref() == Some(input) {
            self.saved_active_repo_path = Some(root.clone());
        }
        if let Some(missing) = self
            .unavailable_repos
            .iter()
            .position(|(path, _)| path == input || *path == root)
        {
            let (_, selected) = self.unavailable_repos.remove(missing);
            if let Some(selected) = selected {
                self.pending_worktrees
                    .insert(tab.project.entity_id(), selected);
            }
        }
    }

    fn inspect_validated_checkout(
        &mut self,
        index: usize,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let project = self.tabs[index].project.read(cx);
        let name = project
            .worktree_at(&path)
            .map(|wt| wt.name.clone())
            .unwrap_or_else(|| {
                path.file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned()
            });
        // The checkout was validated above; don't depend on cached enumeration.
        self.pending_worktrees
            .remove(&self.tabs[index].project.entity_id());
        super::events::set_inspecting_worktree_for_tab(
            self,
            index,
            Some(super::InspectingWorktree { name, path }),
            cx,
        );
    }

    /// Build the current command context from the active tab's project state.
    fn build_command_context(&self, cx: &Context<Self>) -> CommandContext {
        let idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        let Some(tab) = self.tabs.get(idx) else {
            return CommandContext::none();
        };
        let proj = tab.project.read(cx);
        let inspected = tab
            .inspecting_worktree
            .as_ref()
            .and_then(|wt| proj.worktree_at(&wt.path));
        // An inspected checkout owns its status and sequencer state. Never
        // enable actions using changes or a merge in the main checkout.
        let status = if tab.inspecting_worktree.is_some() {
            inspected.and_then(|wt| wt.status.as_ref())
        } else {
            Some(proj.status())
        };
        let has_changes =
            status.is_some_and(|status| !status.staged.is_empty() || !status.unstaged.is_empty());
        let has_staged = status.is_some_and(|status| !status.staged.is_empty());
        let repo_state = inspected
            .map(|wt| wt.state)
            .unwrap_or_else(|| proj.repo_state());
        let has_token = tab.prs_panel.read(cx).github_token().is_some();
        CommandContext::from_parts(
            !proj.remotes().is_empty(),
            has_changes,
            repo_state,
            !proj.stashes().is_empty(),
            has_staged,
            has_token,
        )
        .with_multi_commit_selection(tab.graph.read(cx).selected_commit_count() > 1)
        .with_previous_branch(proj.previous_branch().is_some())
        .with_ai_ready(
            cx.try_global::<rgitui_settings::SettingsState>()
                .is_some_and(|state| {
                    let settings = state.settings();
                    settings.ai.enabled
                        && rgitui_ai::ai_credentials_ready(&settings.ai, state.has_ai_api_key())
                }),
        )
    }

    /// Update the command palette's context with fresh data from the active tab.
    /// Call this after refresh operations so predicates stay accurate.
    pub fn update_command_context(&mut self, cx: &mut Context<Self>) {
        let ctx = self.build_command_context(cx);
        self.overlays.command_palette.update(cx, |cp, _cx| {
            cp.set_context(ctx);
        });
    }

    /// Refresh active first, then inactive tabs sequentially. A restored
    /// 15-repository session must not launch 14 heavyweight refreshes at once.
    pub fn refresh_all_tabs_prioritized(&self, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let active = self.active_tab.min(self.tabs.len() - 1);

        // Collect project entities for inactive tabs
        let inactive_projects: Vec<gpui::WeakEntity<rgitui_git::GitProject>> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != active)
            .map(|(_, tab)| tab.project.downgrade())
            .collect();

        // Start active tab refresh; when it completes, start the rest
        let active_project = self.tabs[active].project.clone();
        let active_project_id = active_project.entity_id();
        let task = active_project.update(cx, |proj, cx| proj.refresh(cx));

        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            if let Err(e) = task.await {
                log::error!("Active tab refresh failed: {}", e);
            }

            // Update command context so predicates (has_stashes, has_changes, etc.)
            // reflect the refreshed state before user opens command palette.
            if let Some(ws) = this.upgrade() {
                cx.update(|cx| {
                    ws.update(cx, |ws, cx| {
                        ws.restore_pending_worktree(active_project_id, cx);
                        ws.update_command_context(cx);
                    });
                });
            }

            // Bound background work and stop refreshing tabs closed meanwhile.
            for project in inactive_projects {
                let project_id = project.entity_id();
                let open = this
                    .update(cx, |ws, _| ws.project_tab_index(project_id).is_some())
                    .unwrap_or(false);
                if !open {
                    continue;
                }
                let Some(proj) = project.upgrade() else {
                    continue;
                };
                let task = cx.update(|cx| proj.update(cx, |proj, cx| proj.refresh(cx)));
                if let Err(error) = task.await {
                    log::error!("Background tab refresh failed: {}", error);
                }
                let _ = this.update(cx, |ws, cx| ws.restore_pending_worktree(project_id, cx));
            }
        })
        .detach();
    }

    fn restore_pending_worktree(&mut self, project: gpui::EntityId, cx: &mut Context<Self>) {
        let Some(index) = self.project_tab_index(project) else {
            return;
        };
        if !self.tabs[index].project.read(cx).has_loaded() {
            return;
        }
        let Some(path) = self.pending_worktrees.remove(&project) else {
            return;
        };
        let inspecting = self.tabs[index]
            .project
            .read(cx)
            .worktree_at(&path)
            .filter(|worktree| !worktree.is_current)
            .map(|worktree| super::InspectingWorktree {
                name: worktree.name.clone(),
                path,
            });
        super::events::set_inspecting_worktree_for_tab(self, index, inspecting, cx);
        self.persist_workspace_snapshot(cx);
        cx.notify();
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let project_id = self.tabs[index].project.entity_id();
        if self.project_has_active_operations(project_id) {
            self.show_toast(
                t!("workspace.safety.operations_running"),
                ToastKind::Warning,
                cx,
            );
            return;
        }
        if index == self.active_tab {
            self.dismiss_checkout_dialogs(cx);
        }
        let panel = &self.tabs[index].commit_panel;
        if self
            .operations
            .ai_target
            .as_ref()
            .is_some_and(|target| target.panel.upgrade().as_ref() == Some(panel))
        {
            self.operations.ai_target = None;
            self.ai.update(cx, |ai, cx| ai.cancel(cx));
        }
        let selected = selection_after_close(self.active_tab, index, self.tabs.len());
        let removed = self.tabs.remove(index).project.entity_id();
        self.pending_worktrees.remove(&removed);
        if self.previous_project == Some(removed) {
            self.previous_project = None;
        }
        // Rebase the index before activation so a preceding close cannot turn
        // the old numeric slot into a spurious previous-repository entry.
        self.active_tab = selected;
        if self.tabs.is_empty() {
            self.update_command_context(cx);
        } else {
            self.activate_tab(selected, cx);
        }
        self.save_workspace_state(cx);
        cx.notify();
    }

    pub fn go_home(&mut self, cx: &mut Context<Self>) {
        if !self.operations.active_operations.is_empty() {
            self.show_toast(
                t!("workspace.safety.operations_running"),
                ToastKind::Warning,
                cx,
            );
            return;
        }
        self.dismiss_checkout_dialogs(cx);
        self.operations.ai_target = None;
        self.ai.update(cx, |ai, cx| ai.cancel(cx));
        // Mark clean exit before clearing workspace
        self.mark_clean_exit(cx);
        self.tabs.clear();
        self.active_tab = 0;
        self.previous_project = None;
        self.pending_worktrees.clear();
        self.unavailable_repos.clear();
        self.session_repo_order.clear();
        self.saved_active_repo_path = None;
        self.update_command_context(cx);
        self.save_layout(cx);
        self.clear_active_workspace_state(cx);
        cx.notify();
    }

    pub fn restore_workspace_snapshot(
        &mut self,
        snapshot: StoredWorkspace,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if !self.operations.active_operations.is_empty() {
            anyhow::bail!("{}", t!("workspace.safety.operations_running"));
        }
        self.dismiss_checkout_dialogs(cx);
        self.operations.ai_target = None;
        self.ai.update(cx, |ai, cx| ai.cancel(cx));
        log::info!(
            "restoring workspace '{}' with {} repos",
            snapshot.name,
            snapshot.repos.len()
        );

        self.restoring_workspace = true;
        self.tabs.clear();
        self.active_tab = 0;
        self.previous_project = None;
        self.pending_worktrees.clear();
        self.unavailable_repos.clear();
        self.session_repo_order = snapshot.repos.clone();
        self.saved_active_repo_path = snapshot
            .selected_repo_path()
            .map(std::path::Path::to_path_buf);
        self.active_workspace_id = Some(snapshot.id.clone());
        self.apply_layout_settings(&snapshot.layout);

        let mut opened_any = false;
        let mut selected_project = None;
        for repo_path in &snapshot.repos {
            match self.open_repo(repo_path.clone(), cx) {
                Ok(()) => {
                    opened_any = true;
                    // Map even legacy/non-normalized saved spellings to the
                    // resolved root without losing their original slot.
                    self.reconcile_opened_path(repo_path, self.active_tab, cx);
                    if snapshot.selected_repo_path() == Some(repo_path.as_path()) {
                        selected_project = self.active_project().map(|project| project.entity_id());
                    }
                    if let Some(path) = snapshot.selected_worktrees.get(repo_path) {
                        if let Some(project) = self.active_project() {
                            self.pending_worktrees
                                .insert(project.entity_id(), path.clone());
                        }
                    }
                }
                Err(error) => {
                    self.unavailable_repos.push((
                        repo_path.clone(),
                        snapshot.selected_worktrees.get(repo_path).cloned(),
                    ));
                    log::error!(
                        "Failed to restore repo '{}' from workspace '{}': {}",
                        repo_path.display(),
                        snapshot.name,
                        error
                    );
                }
            }
        }

        if !self.unavailable_repos.is_empty() {
            self.show_toast(
                t!("workspace.restore.unavailable_repositories"),
                ToastKind::Warning,
                cx,
            );
        }
        if !opened_any {
            self.restoring_workspace = false;
            self.update_command_context(cx);
            self.persist_workspace_snapshot(cx);
            cx.notify();
            anyhow::bail!(
                "Workspace '{}' has no available repositories",
                snapshot.name
            );
        }

        self.restoring_workspace = false;
        let selected = selected_project
            .and_then(|project| self.project_tab_index(project))
            .unwrap_or(0);
        self.activate_tab(selected, cx);
        self.previous_project = None;

        self.refresh_all_tabs_prioritized(cx);

        self.set_status_message(
            Message::verbatim(format!("Opened workspace '{}'", snapshot.name)),
            cx,
        );
        cx.notify();
        Ok(())
    }

    pub(super) fn restore_last_workspace(&mut self, cx: &mut Context<Self>) {
        let snapshot = cx
            .try_global::<rgitui_settings::SettingsState>()
            .and_then(|settings| settings.active_workspace().cloned());

        if let Some(snapshot) = snapshot {
            if let Err(error) = self.restore_workspace_snapshot(snapshot, cx) {
                self.show_toast(error.to_string(), ToastKind::Error, cx);
            }
        } else {
            self.show_toast("No saved workspace available.", ToastKind::Info, cx);
        }
    }

    pub(super) fn current_layout_settings(&self) -> LayoutSettings {
        LayoutSettings {
            sidebar_width: self.layout.sidebar_width,
            detail_panel_width: self.layout.detail_panel_width,
            diff_viewer_height: self.layout.diff_viewer_height,
            commit_input_height: self.layout.commit_input_height,
            graph_hidden: self.layout.graph_hidden,
        }
    }

    pub(super) fn apply_layout_settings(&mut self, layout: &LayoutSettings) {
        self.layout.sidebar_width = layout.sidebar_width;
        self.layout.detail_panel_width = layout.detail_panel_width;
        self.layout.diff_viewer_height = layout.diff_viewer_height;
        self.layout.commit_input_height = layout.commit_input_height.max(300.0);
        self.layout.graph_hidden = layout.graph_hidden;
    }

    pub(super) fn persist_workspace_snapshot(&mut self, cx: &mut Context<Self>) {
        if self.restoring_workspace {
            return;
        }
        let selected_worktrees = self
            .tabs
            .iter()
            .filter_map(|tab| {
                let selected = tab
                    .inspecting_worktree
                    .as_ref()
                    .map(|wt| wt.path.clone())
                    .or_else(|| {
                        self.pending_worktrees
                            .get(&tab.project.entity_id())
                            .cloned()
                    })?;
                Some((tab.project.read(cx).repo_path().to_path_buf(), selected))
            })
            .chain(
                self.unavailable_repos
                    .iter()
                    .filter_map(|(repo, worktree)| {
                        worktree
                            .as_ref()
                            .map(|worktree| (repo.clone(), worktree.clone()))
                    }),
            )
            .collect();
        let live: Vec<std::path::PathBuf> = self
            .tabs
            .iter()
            .map(|t| t.project.read(cx).repo_path().to_path_buf())
            .chain(self.unavailable_repos.iter().map(|(repo, _)| repo.clone()))
            .collect();
        let repos = merge_session_order(&self.session_repo_order, &live);
        self.session_repo_order = repos.clone();
        if let Some(tab) = self.tabs.get(self.active_tab) {
            self.saved_active_repo_path = Some(tab.project.read(cx).repo_path().to_path_buf());
        }
        let active_index = session_active_index(&repos, self.saved_active_repo_path.as_deref());

        if cx.try_global::<rgitui_settings::SettingsState>().is_none() {
            return;
        }

        let settings = cx.global_mut::<rgitui_settings::SettingsState>();
        if let Some(workspace_id) = settings.save_workspace_snapshot(
            self.active_workspace_id.as_deref(),
            repos,
            active_index,
            self.current_layout_settings(),
        ) {
            if let Some(snapshot) = settings
                .settings_mut()
                .workspaces
                .iter_mut()
                .find(|snapshot| snapshot.id == workspace_id)
            {
                snapshot.selected_worktrees = selected_worktrees;
            }
            self.active_workspace_id = Some(workspace_id);
        }

        if let Err(error) = settings.save() {
            log::error!("Failed to persist workspace snapshot: {}", error);
        }
    }

    pub(super) fn clear_active_workspace_state(&mut self, cx: &mut Context<Self>) {
        self.active_workspace_id = None;
        self.session_repo_order.clear();
        self.saved_active_repo_path = None;
        self.status_message = None;
        self.operations.active_git_operation = None;
        self.operations.active_git_operation_target = None;
        self.operations.last_failed_git_operation = None;
        self.operations.last_failed_git_operation_target = None;
        self.operations.active_operations.clear();
        self.operations.last_operation_output = None;

        if cx.try_global::<rgitui_settings::SettingsState>().is_some() {
            let settings = cx.global_mut::<rgitui_settings::SettingsState>();
            settings.clear_active_workspace();
            if let Err(error) = settings.save() {
                log::error!("Failed to clear active workspace: {}", error);
            }
        }
    }

    /// Persist the current set of open repo paths and layout to settings.
    pub(super) fn save_workspace_state(&mut self, cx: &mut Context<Self>) {
        self.save_layout(cx);
        if self.tabs.is_empty() && self.unavailable_repos.is_empty() {
            self.clear_active_workspace_state(cx);
        } else {
            self.persist_workspace_snapshot(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_metadata_resolves_aliases_and_linked_first_main_checkout() {
        let fixture = rgitui_test_support::TempRepo::with_commits(1);
        let main = RepositoryIdentity::from_repo(fixture.repo());
        for alias in [
            fixture.path().join("."),
            fixture.path().join(".git"),
            fixture.path().join(".git/.."),
        ] {
            let repo = git2::Repository::open(alias).unwrap();
            assert_eq!(RepositoryIdentity::from_repo(&repo), main);
        }
        let head = fixture.repo().head().unwrap().peel_to_commit().unwrap();
        fixture.repo().branch("feature", &head, false).unwrap();
        let reference = fixture.repo().find_reference("refs/heads/feature").unwrap();
        let mut options = git2::WorktreeAddOptions::new();
        options.reference(Some(&reference));
        let path = fixture.path().join("feature");
        fixture
            .repo()
            .worktree("feature", &path, Some(&options))
            .unwrap();
        let linked_repo = git2::Repository::open(&path).unwrap();
        let linked = RepositoryIdentity::from_repo(&linked_repo);
        assert!(linked.linked);
        assert_eq!(linked.common_dir, main.common_dir);
        assert_ne!(linked.checkout, main.checkout);
        let opened_main = git2::Repository::open(linked.project_open_path()).unwrap();
        assert_eq!(RepositoryIdentity::from_repo(&opened_main), main);
        drop(linked_repo);
        std::fs::remove_dir_all(&path).unwrap();
        // Registry metadata may remain, but validation must reject the checkout.
        assert!(git2::Repository::open(&path).is_err());
    }

    #[test]
    fn session_merge_preserves_missing_slots_and_active_identity() {
        let saved = ["/a", "/b", "/c"].map(std::path::PathBuf::from);
        // Available A/C plus unavailable B, and then a newly opened D.
        let live = ["/a", "/c", "/b", "/d"].map(std::path::PathBuf::from);
        let merged = merge_session_order(&saved, &live);
        assert_eq!(
            merged,
            ["/a", "/b", "/c", "/d"].map(std::path::PathBuf::from)
        );
        assert_eq!(
            session_active_index(&merged, Some(std::path::Path::new("/c"))),
            2
        );
        // All unavailable: the original B selection survives saving/restarting.
        let unavailable = merge_session_order(&saved, &saved);
        assert_eq!(
            session_active_index(&unavailable, Some(std::path::Path::new("/b"))),
            1
        );
        // A recovered B keeps its original slot; explicitly closed A disappears.
        assert_eq!(
            merge_session_order(&merged, &live[1..]),
            ["/b", "/c", "/d"].map(std::path::PathBuf::from)
        );
    }

    #[test]
    fn closing_any_of_fifteen_repositories_preserves_other_active_identity() {
        for active in 0..15 {
            for closed in 0..15 {
                let mut ids: Vec<_> = (0..15).collect();
                let selected = selection_after_close(active, closed, ids.len());
                ids.remove(closed);
                assert!(selected < ids.len());
                if closed != active {
                    assert_eq!(ids[selected], active, "active={active}, closed={closed}");
                } else {
                    assert_eq!(ids[selected], if active == 14 { 13 } else { active + 1 });
                }
            }
        }
    }

    #[test]
    fn closing_before_active_preserves_repository_identity() {
        let mut ids = vec![10, 20, 30, 40];
        let selected = selection_after_close(2, 0, ids.len());
        ids.remove(0);
        assert_eq!(ids[selected], 30);
    }

    #[test]
    fn closing_active_selects_neighbour_and_last_close_is_safe() {
        assert_eq!(selection_after_close(1, 1, 4), 1);
        assert_eq!(selection_after_close(3, 3, 4), 2);
        assert_eq!(selection_after_close(0, 0, 1), 0);
        assert_eq!(selection_after_close(0, 3, 4), 0);
    }

    #[test]
    fn previous_repository_toggles_identity_without_losing_history_on_reselection() {
        let previous = previous_after_activation(Some(10), 20, None);
        assert_eq!(previous, Some(10));
        assert_eq!(previous_after_activation(Some(20), 20, previous), Some(10));
        assert_eq!(previous_after_activation(Some(20), 10, previous), Some(20));
        assert_eq!(previous_after_activation::<u64>(None, 10, None), None);
    }

    #[test]
    fn repo_labels_disambiguate_duplicates_and_include_checkout() {
        let names = repository_names(
            &[
                "/team/a/repo".into(),
                "/team/b/repo".into(),
                "/team/other".into(),
            ],
            &["repo".into(), "repo".into(), "other".into()],
        );
        assert_ne!(names[0], names[1]);
        assert!(names[0].contains("/team/a"));
        assert!(names[1].contains("/team/b"));
        assert_eq!(names[2], "other");
        assert_eq!(repository_label("repo", Some("feature")), "repo · feature");
        assert_eq!(repository_label("repo", None), "repo");
    }
}
