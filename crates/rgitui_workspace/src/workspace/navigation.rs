//! Center navigation is independent of panel entities: switching views never
//! rebuilds the graph, diff, or commit composer.
use gpui::{Context, Window};

use super::{BottomPanelMode, FocusedPanel, ProjectTab, Workspace};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CenterView {
    #[default]
    Graph,
    Content,
}

impl CenterView {
    pub(super) fn visibility(self, graph_focused: bool, graph_hidden: bool) -> (bool, bool) {
        if graph_focused {
            (self == Self::Graph, self == Self::Content)
        } else {
            (!graph_hidden, true)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum InspectionContext {
    #[default]
    WorkingChanges,
    Commit(git2::Oid),
    Stash(git2::Oid),
}

fn change_inspection(
    current: &mut InspectionContext,
    generation: &mut u64,
    next: InspectionContext,
) -> u64 {
    *generation = generation.wrapping_add(1);
    *current = next;
    *generation
}

/// Both identity/path and a monotonic selection are needed: leaving a checkout
/// and returning to it must not resurrect the request from the first visit.
pub(super) fn request_is_current<Id: PartialEq>(
    origin: &super::state::safety::RepositoryTarget<Id>,
    requested: u64,
    project: &Id,
    checkout: &std::path::Path,
    current: u64,
) -> bool {
    origin.matches(project, checkout) && requested == current
}

/// Pure transition shared by the UI entry points and regression tests.
fn navigate(center: &mut CenterView, open: &mut bool, target: CenterView, close: bool) {
    *center = target;
    if close {
        *open = false;
    } else if target == CenterView::Content {
        *open = true;
    }
}

/// Visibility changes retain the payload request. Only replacing/closing the
/// content or explicitly starting another load should invalidate its result.
fn reveal_content(
    center: &mut CenterView,
    open: &mut bool,
    mode: &mut BottomPanelMode,
    generation: &mut u64,
    next: BottomPanelMode,
) -> bool {
    let replaced = !*open || *mode != next;
    if replaced {
        *generation = generation.wrapping_add(1);
    }
    *mode = next;
    navigate(center, open, CenterView::Content, false);
    replaced
}

impl ProjectTab {
    pub(super) fn inspecting_working_changes(&self) -> bool {
        self.inspection == InspectionContext::WorkingChanges
    }

    /// Unlocalized target label; the layout supplies the localized Diff fallback.
    pub(super) fn diff_tab_label(&self, cx: &gpui::App) -> Option<String> {
        self.diff_viewer.read(cx).file_path().map(str::to_owned)
    }

    pub(super) fn reveal_content(&mut self, mode: BottomPanelMode) {
        if reveal_content(
            &mut self.center_view,
            &mut self.content_open,
            &mut self.bottom_panel_mode,
            &mut self.navigation_generation,
            mode,
        ) {
            self.pending_auxiliary = None;
        }
    }

    fn invalidate_content_requests(&mut self) {
        self.pending_auxiliary = None;
        self.navigation_generation = self.navigation_generation.wrapping_add(1);
    }

    /// Unlike clicking a retained tab, a fresh load supersedes any older load
    /// of the same view (including one started before returning to the graph).
    pub(super) fn begin_content_request(&mut self, mode: BottomPanelMode) -> u64 {
        self.reveal_content(mode);
        self.invalidate_content_requests();
        self.navigation_generation
    }

    pub(super) fn reveal_graph(&mut self) {
        navigate(
            &mut self.center_view,
            &mut self.content_open,
            CenterView::Graph,
            false,
        );
    }

    pub(super) fn inspect(&mut self, context: InspectionContext) -> u64 {
        self.right_panel_mode = super::RightPanelMode::Details;
        change_inspection(
            &mut self.inspection,
            &mut self.inspection_generation,
            context,
        )
    }

    pub(super) fn reset_checkout_navigation(&mut self) {
        self.invalidate_content_requests();
        self.inspect(InspectionContext::WorkingChanges);
        self.reveal_graph();
        self.content_open = false;
        self.bottom_panel_mode = BottomPanelMode::Diff;
    }
}

impl Workspace {
    pub(super) fn graph_is_visible(&self) -> bool {
        self.center_visibility().0
    }

    pub(super) fn content_is_visible(&self) -> bool {
        self.center_visibility().1
    }

    fn center_visibility(&self) -> (bool, bool) {
        self.tabs
            .get(self.active_tab)
            .map_or((false, false), |tab| {
                tab.center_view
                    .visibility(self.layout.graph_focused, self.layout.graph_hidden)
            })
    }

    pub(super) fn show_graph_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.reveal_graph();
        }
        if !self.layout.graph_focused {
            self.set_graph_hidden(false, cx);
        }
        self.focus_panel(FocusedPanel::Graph, window, cx);
        cx.notify();
    }

    pub(super) fn show_content_view(
        &mut self,
        mode: BottomPanelMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = self.tabs.get_mut(self.active_tab).and_then(|tab| {
            tab.reveal_content(mode);
            if matches!(mode, BottomPanelMode::Blame | BottomPanelMode::FileHistory) {
                let key = tab.current_view_cache_key(cx)?;
                tab.pending_auxiliary = Some((key.clone(), mode, tab.navigation_generation));
                Some((key, tab.caches.clone()))
            } else {
                None
            }
        });
        if let Some((key, caches)) = pending {
            Self::prefetch_blame_and_history(key, caches, cx);
            self.refresh_pending_auxiliary(cx);
        }
        self.focus_panel(FocusedPanel::DiffViewer, window, cx);
        cx.notify();
    }

    pub(super) fn close_content_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.invalidate_content_requests();
            navigate(
                &mut tab.center_view,
                &mut tab.content_open,
                CenterView::Graph,
                true,
            );
            // Closing is stronger than hiding: invalidate outstanding file loads.
            tab.diff_viewer.update(cx, |viewer, cx| viewer.clear(cx));
        }
        self.focus_panel(FocusedPanel::Graph, window, cx);
        cx.notify();
    }

    pub(super) fn set_graph_focused_layout(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout.graph_focused == enabled {
            return;
        }
        self.layout.graph_focused = enabled;
        // graph_hidden remains the classic layout preference.
        self.schedule_layout_save(cx);
        self.focus_panel(FocusedPanel::Graph, window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_retains_content_and_close_returns_to_graph() {
        let mut center = CenterView::Graph;
        let mut open = false;
        navigate(&mut center, &mut open, CenterView::Content, false);
        assert_eq!((center, open), (CenterView::Content, true));
        navigate(&mut center, &mut open, CenterView::Graph, false);
        assert_eq!((center, open), (CenterView::Graph, true));
        navigate(&mut center, &mut open, CenterView::Content, false);
        navigate(&mut center, &mut open, CenterView::Graph, true);
        assert_eq!((center, open), (CenterView::Graph, false));
    }

    #[test]
    fn classic_preference_does_not_hide_graph_first_graph() {
        assert_eq!(CenterView::Graph.visibility(true, true), (true, false));
        assert_eq!(CenterView::Content.visibility(true, false), (false, true));
        for center in [CenterView::Graph, CenterView::Content] {
            assert_eq!(center.visibility(false, false), (true, true));
            assert_eq!(center.visibility(false, true), (false, true));
        }
    }

    #[test]
    fn stale_response_requires_origin_checkout_and_selection() {
        let origin = super::super::state::safety::RepositoryTarget {
            project_id: 7,
            worktree_path: "main".into(),
        };
        assert!(request_is_current(
            &origin,
            2,
            &7,
            std::path::Path::new("main"),
            2
        ));
        assert!(!request_is_current(
            &origin,
            2,
            &8,
            std::path::Path::new("main"),
            2
        ));
        assert!(!request_is_current(
            &origin,
            2,
            &7,
            std::path::Path::new("linked"),
            2
        ));
        // Same path after leaving/returning, or same commit selected again.
        assert!(!request_is_current(
            &origin,
            2,
            &7,
            std::path::Path::new("main"),
            4
        ));
    }

    #[test]
    fn late_commit_details_cannot_replace_working_inspector() {
        let mut inspection = InspectionContext::WorkingChanges;
        let mut generation = 0;
        let oid = git2::Oid::from_str("1111111111111111111111111111111111111111").unwrap();
        let requested = change_inspection(
            &mut inspection,
            &mut generation,
            InspectionContext::Commit(oid),
        );
        // Clicking the current checkout's working row does not change its path.
        change_inspection(
            &mut inspection,
            &mut generation,
            InspectionContext::WorkingChanges,
        );
        let origin = super::super::state::safety::RepositoryTarget {
            project_id: 7,
            worktree_path: "main".into(),
        };
        assert!(!request_is_current(
            &origin,
            requested,
            &7,
            std::path::Path::new("main"),
            generation
        ));
        assert_eq!(inspection, InspectionContext::WorkingChanges);
        // Re-selecting the same commit must not accept its first request either.
        change_inspection(
            &mut inspection,
            &mut generation,
            InspectionContext::Commit(oid),
        );
        assert!(!request_is_current(
            &origin,
            requested,
            &7,
            std::path::Path::new("main"),
            generation
        ));
    }

    #[test]
    fn retained_auxiliary_load_survives_graph_return_and_active_tab_click() {
        for initial_mode in [
            BottomPanelMode::Reflog,
            BottomPanelMode::Submodules,
            BottomPanelMode::Bisect,
            BottomPanelMode::Diff,
        ] {
            let mut center = CenterView::Content;
            let mut open = true;
            let mut mode = initial_mode;
            let mut generation = 1;
            // Clicking the already-active tab does not cancel its payload.
            assert!(!reveal_content(
                &mut center,
                &mut open,
                &mut mode,
                &mut generation,
                initial_mode,
            ));
            assert_eq!(generation, 1);
            navigate(&mut center, &mut open, CenterView::Graph, false);
            assert_eq!((center, open, generation), (CenterView::Graph, true, 1));
            // The callback can populate the hidden entity without navigating.
            assert!(!reveal_content(
                &mut center,
                &mut open,
                &mut mode,
                &mut generation,
                initial_mode,
            ));
            assert_eq!((center, generation), (CenterView::Content, 1));
        }
    }

    #[test]
    fn replacing_or_reopening_closed_content_gets_a_new_request_generation() {
        let mut center = CenterView::Content;
        let mut open = true;
        let mut mode = BottomPanelMode::Reflog;
        let mut generation = 1;
        assert!(reveal_content(
            &mut center,
            &mut open,
            &mut mode,
            &mut generation,
            BottomPanelMode::Diff,
        ));
        assert_eq!(generation, 2);
        navigate(&mut center, &mut open, CenterView::Graph, true);
        assert!(reveal_content(
            &mut center,
            &mut open,
            &mut mode,
            &mut generation,
            BottomPanelMode::Diff,
        ));
        assert_eq!(generation, 3);
    }

    #[test]
    fn graph_focus_cycle_never_visits_hidden_content() {
        use FocusedPanel::*;
        let mut panel = Sidebar;
        let mut visited = Vec::new();
        for _ in 0..3 {
            panel = FocusedPanel::cycle_visible(Some(panel), true, true, false);
            visited.push(panel);
        }
        assert_eq!(visited, [Graph, DetailPanel, Sidebar]);
        assert_eq!(
            FocusedPanel::cycle_visible(Some(Sidebar), false, true, false),
            DetailPanel
        );
        assert_eq!(Graph.visible_target(false, true), DiffViewer);
        assert_eq!(DiffViewer.visible_target(true, false), Graph);
    }

    #[test]
    fn repository_navigation_is_independent() {
        let mut a = CenterView::Graph;
        let b = CenterView::Graph;
        let mut open = false;
        navigate(&mut a, &mut open, CenterView::Content, false);
        assert_eq!(b, CenterView::Graph);
        assert_eq!(a, CenterView::Content);
    }
}
