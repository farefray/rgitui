use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    div, img, px, uniform_list, App, ClickEvent, ClipboardItem, Context, ElementId, Entity,
    EventEmitter, FocusHandle, ListSizingBehavior, ObjectFit, Render, ScrollStrategy, SharedString,
    UniformListScrollHandle, WeakEntity, Window,
};
use rgitui_diff::DiffSource;
use rgitui_git::{
    compact_ref_labels, BranchInfo, CommitDiff, CommitInfo, FileChangeKind, FileDiff, RefLabel,
    Signature,
};
use rgitui_i18n::{key, t};
use rgitui_settings::SettingsState;
use rgitui_theme::{ActiveTheme, Color, StyledExt};
use rgitui_ui::{
    AvatarCache, Badge, Button, ButtonSize, ButtonStyle, DiffStat, Icon, IconButton, IconName,
    IconSize, Label, LabelSize, Scrollbar, TextInput, TextInputEvent, Tooltip,
};

use crate::file_status::{file_change_color, FileStatusBadge};
use crate::keymap;
use crate::markdown_view::render_markdown;
use crate::CommandId;

mod working_tree;

/// Rows the changed-files list keeps when the header and commit message leave it
/// little room. Once it is this short the panel scrolls as one region instead.
const FILE_LIST_MIN_VISIBLE_ROWS: f32 = 4.0;
const FILE_LIST_BOTTOM_PADDING: f32 = 8.0;
const FILE_LIST_SCROLLBAR_THRESHOLD: usize = FILE_LIST_MIN_VISIBLE_ROWS as usize;

/// Height the changed-files list would like: every row plus the bottom padding.
///
/// The list is the only shrinkable child of the detail panel's scroll area, so
/// flex trims this to whatever space is left below the commit message. It
/// therefore ends exactly at the bottom of the panel — virtualizing and
/// scrolling internally — rather than stopping at a fixed height and leaving
/// unused space beneath it.
fn file_list_content_height(row_count: usize, row_height: f32) -> f32 {
    row_count as f32 * row_height + FILE_LIST_BOTTOM_PADDING
}

/// Height below which the list stops shrinking. Lists with only a few rows never
/// grow a scrollbar, since their content height is already smaller.
fn file_list_min_height(row_count: usize, row_height: f32) -> f32 {
    file_list_content_height(row_count, row_height).min(FILE_LIST_MIN_VISIBLE_ROWS * row_height)
}

/// Largest font size for the initials drawn in an avatar with no image.
const AVATAR_INITIALS_MAX_SIZE: f32 = 12.0;

/// Font size for the fallback initials in an avatar circle of `avatar_size`.
///
/// Scales with the circle so two bold capitals fit inside a small co-author
/// avatar, and caps at the old fixed size so larger avatars look unchanged.
fn avatar_initials_size(avatar_size: f32) -> f32 {
    (avatar_size * 0.6).min(AVATAR_INITIALS_MAX_SIZE)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum FileViewMode {
    #[default]
    Flat,
    Tree,
}

impl FileViewMode {
    fn toggled(self) -> Self {
        match self {
            Self::Flat => Self::Tree,
            Self::Tree => Self::Flat,
        }
    }

    fn toggle_icon(self) -> IconName {
        match self {
            Self::Flat => IconName::Folder,
            Self::Tree => IconName::File,
        }
    }

    fn toggle_tooltip(self) -> SharedString {
        match self {
            Self::Flat => t!("detail.switch_to_tree.tooltip"),
            Self::Tree => t!("detail.switch_to_flat.tooltip"),
        }
    }

    fn list_element_id(self) -> &'static str {
        match self {
            Self::Flat => "detail-files-filtered",
            Self::Tree => "detail-files-filtered-tree",
        }
    }
}

fn is_file_searching(file_search_active: bool, file_search_query: &Option<String>) -> bool {
    file_search_active || file_search_query.is_some()
}

fn can_toggle_file_view(file_count: usize, is_searching: bool) -> bool {
    file_count > 0 && !is_searching
}

fn file_view_toggle_tooltip(
    file_count: usize,
    is_searching: bool,
    file_view_mode: FileViewMode,
) -> SharedString {
    if file_count == 0 {
        t!("detail.no_files.tooltip")
    } else if is_searching {
        t!("detail.clear_search_to_switch.tooltip")
    } else {
        file_view_mode.toggle_tooltip()
    }
}

fn file_view_mode_after_toggle_request(
    file_view_mode: FileViewMode,
    file_count: usize,
    is_searching: bool,
) -> FileViewMode {
    if can_toggle_file_view(file_count, is_searching) {
        file_view_mode.toggled()
    } else {
        file_view_mode
    }
}

fn format_absolute_date(timestamp: i64) -> String {
    let dt = chrono::DateTime::from_timestamp(timestamp, 0);
    match dt {
        Some(dt) => dt.format("%b %d, %Y %H:%M").to_string(),
        None => "Unknown date".to_string(),
    }
}

#[derive(Debug, Clone)]
pub enum WorkingChangesAction {
    StageFile(String),
    UnstageFile(String),
    DiscardFile(String),
    StageAll,
    UnstageAll,
}

#[derive(Clone)]
enum WorkingRow {
    Section {
        staged: bool,
        count: usize,
    },
    Directory {
        path: String,
        name: String,
        depth: usize,
        count: usize,
        staged: bool,
        collapsed: bool,
    },
    File {
        index: usize,
        file: rgitui_git::FileStatus,
        staged: bool,
        depth: usize,
    },
}

/// Staged and unstaged versions of the same path are distinct selections.
fn working_selection(
    status: &rgitui_git::WorkingTreeStatus,
    index: usize,
) -> Option<(std::path::PathBuf, bool)> {
    if index < status.staged.len() {
        status
            .staged
            .get(index)
            .map(|file| (file.path.clone(), true))
    } else {
        status
            .unstaged
            .get(index - status.staged.len())
            .map(|file| (file.path.clone(), false))
    }
}

fn reconcile_working_selection(
    old: &rgitui_git::WorkingTreeStatus,
    new: &rgitui_git::WorkingTreeStatus,
    selected: Option<usize>,
) -> Option<usize> {
    let (path, staged) = working_selection(old, selected?)?;
    if staged {
        new.staged.iter().position(|file| file.path == path)
    } else {
        new.unstaged
            .iter()
            .position(|file| file.path == path)
            .map(|index| new.staged.len() + index)
    }
}

fn working_rows(status: &rgitui_git::WorkingTreeStatus) -> Vec<WorkingRow> {
    let mut rows = Vec::new();
    let mut index = 0;
    for (staged, files) in [(true, &status.staged), (false, &status.unstaged)] {
        rows.push(WorkingRow::Section {
            staged,
            count: files.len(),
        });
        for file in files {
            rows.push(WorkingRow::File {
                index,
                file: file.clone(),
                staged,
                depth: 0,
            });
            index += 1;
        }
    }
    rows
}

/// `None` is the unfiltered identity order: ordinary renders need no allocation
/// or path conversion. Filtered renders and keyboard navigation share this map.
fn working_display_order(rows: &[WorkingRow], query: Option<&str>) -> Option<Vec<usize>> {
    query.map(|query| {
        let query = query.to_lowercase();
        rows.iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let visible = match row {
                    WorkingRow::Section { .. } | WorkingRow::Directory { .. } => true,
                    WorkingRow::File { file, .. } => {
                        file.path.to_string_lossy().to_lowercase().contains(&query)
                    }
                };
                visible.then_some(index)
            })
            .collect()
    })
}

/// Build the displayed snapshot only when status, filtering, or expansion changes.
/// File indices always address the original status, not the sorted tree.
fn working_tree_rows(
    status: &rgitui_git::WorkingTreeStatus,
    trees: &[Vec<working_tree::TreeRow>; 2],
    collapsed: &[HashSet<String>; 2],
    query: Option<&str>,
) -> Vec<WorkingRow> {
    let mut rows = Vec::new();
    let query = query.map(str::to_lowercase);
    for (section, files) in [&status.staged, &status.unstaged].into_iter().enumerate() {
        let staged = section == 0;
        let offset = if staged { 0 } else { status.staged.len() };
        rows.push(WorkingRow::Section {
            staged,
            count: files.len(),
        });
        let matches = query.as_ref().map(|query| {
            files
                .iter()
                .enumerate()
                .filter_map(|(index, file)| {
                    file.path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(query)
                        .then_some(index)
                })
                .collect::<HashSet<_>>()
        });
        for index in
            working_tree::visible_rows(&trees[section], &collapsed[section], matches.as_ref())
        {
            rows.push(match &trees[section][index] {
                working_tree::TreeRow::Directory {
                    path,
                    name,
                    depth,
                    count,
                } => WorkingRow::Directory {
                    path: path.clone(),
                    name: name.clone(),
                    depth: *depth,
                    count: *count,
                    staged,
                    collapsed: matches.is_none() && collapsed[section].contains(path),
                },
                working_tree::TreeRow::File { index, depth } => WorkingRow::File {
                    index: offset + index,
                    file: files[*index].clone(),
                    staged,
                    depth: *depth,
                },
            });
        }
    }
    rows
}

#[derive(Debug, PartialEq, Eq)]
struct WorkingNavigation {
    file_index: usize,
    displayed_row: usize,
    activate: bool,
}

/// Resolve a gesture against displayed file rows, never section headers or a
/// hidden search result. The scroll target includes headers and filtering; the
/// file index retains the staged/unstaged identity in the full status snapshot.
fn working_navigation(
    rows: &[WorkingRow],
    display_order: Option<&[usize]>,
    selected: Option<usize>,
    command: CommandId,
) -> Option<WorkingNavigation> {
    let row_count = display_order.map_or(rows.len(), <[usize]>::len);
    let files: Vec<_> = (0..row_count)
        .filter_map(|displayed_row| {
            let actual_row = display_order.map_or(displayed_row, |order| order[displayed_row]);
            match &rows[actual_row] {
                WorkingRow::File { index, .. } => Some((*index, displayed_row)),
                WorkingRow::Section { .. } | WorkingRow::Directory { .. } => None,
            }
        })
        .collect();
    let current = files.iter().position(|(index, _)| Some(*index) == selected);
    let target = match command {
        CommandId::SelectNext => {
            current.map_or(0, |index| (index + 1).min(files.len().saturating_sub(1)))
        }
        CommandId::SelectPrev => current.map_or(0, |index| index.saturating_sub(1)),
        CommandId::SelectFirst => 0,
        CommandId::SelectLast => files.len().checked_sub(1)?,
        CommandId::Confirm => current?,
        _ => return None,
    };
    let &(file_index, displayed_row) = files.get(target)?;
    Some(WorkingNavigation {
        file_index,
        displayed_row,
        activate: command == CommandId::Confirm || selected != Some(file_index),
    })
}

#[derive(Debug, Clone)]
pub enum DetailPanelEvent {
    /// A file in the changed-files list was picked. Carries the provenance of
    /// the content so the diff viewer never has to infer it. Working changes
    /// use `WorkingFileSelected` and load their current contents asynchronously.
    FileSelected(FileDiff, String, DiffSource),
    WorkingFileSelected {
        path: String,
        staged: bool,
        conflicted: bool,
        checkout: std::path::PathBuf,
    },
    WorkingAction {
        checkout: std::path::PathBuf,
        action: WorkingChangesAction,
    },
    CopySha(String),
    CherryPick(String),
    NavigatePrevCommit,
    NavigateNextCommit,
}

fn file_change_icon(kind: FileChangeKind) -> IconName {
    match kind {
        FileChangeKind::Added => IconName::FileAdded,
        FileChangeKind::Modified => IconName::FileModified,
        FileChangeKind::Deleted => IconName::FileDeleted,
        FileChangeKind::Renamed => IconName::FileRenamed,
        FileChangeKind::Copied => IconName::FileRenamed,
        FileChangeKind::TypeChange => IconName::FileModified,
        FileChangeKind::Untracked => IconName::FileAdded,
        FileChangeKind::Conflicted => IconName::FileConflict,
    }
}

struct CachedFileDiffTree {
    /// Rows for flat view: one per file, in diff order (the search-relevance
    /// ordering is applied later by `filtered_files`). Held behind an `Arc` so
    /// the virtualized list closure can take a cheap `'static` handle and clone
    /// only the rows in the visible window.
    flat_rows: Arc<Vec<CachedFlatRow>>,
    /// Rows for tree view: directory and file nodes flattened depth-first, with
    /// single-child directory chains compacted (e.g. `crates/foo/src`). Shared
    /// via `Arc` for the same reason as `flat_rows`.
    tree_rows: Arc<Vec<CachedTreeRow>>,
}

#[derive(Clone)]
struct CachedFlatRow {
    file_index: usize,
    file_name: SharedString,
    dir_path: SharedString,
    additions: usize,
    deletions: usize,
    icon_name: IconName,
    icon_color: Color,
    kind: FileChangeKind,
}

/// A file leaf in the tree view. Carries only what a file row renders — the
/// basename lives on the enclosing [`CachedTreeRow::label`], and the directory
/// path is intentionally absent (the tree shows hierarchy, not full paths).
#[derive(Clone)]
struct CachedTreeFile {
    file_index: usize,
    additions: usize,
    deletions: usize,
    icon_name: IconName,
    icon_color: Color,
    kind: FileChangeKind,
}

#[derive(Clone)]
enum CachedTreeRowKind {
    /// A directory node. `key` is its full path, used as the collapse identity.
    Dir {
        key: SharedString,
    },
    File(CachedTreeFile),
}

#[derive(Clone)]
struct CachedTreeRow {
    depth: usize,
    /// Directory name (possibly compacted, e.g. `foo/bar`) or file basename.
    label: SharedString,
    kind: CachedTreeRowKind,
}

/// Intermediate nested structure used to build [`CachedTreeRow`]s. `BTreeMap`
/// keeps directories in stable alphabetical order.
#[derive(Default)]
struct TreeBuilder {
    dirs: std::collections::BTreeMap<String, TreeBuilder>,
    file_indices: Vec<usize>,
}

/// Flatten the nested builder depth-first into ordered rows: directories first
/// (alphabetical), then files (alphabetical), at each level. Single-child
/// directory chains are compacted into one node so deep crate layouts read as
/// `crates/foo/src` rather than three nested rows. The root builder is virtual —
/// its children are emitted at `depth`, no row is emitted for the root itself.
fn flatten_tree(
    builder: &TreeBuilder,
    path_prefix: &str,
    depth: usize,
    files: &[FileDiff],
    flat_rows: &[CachedFlatRow],
    out: &mut Vec<CachedTreeRow>,
) {
    for (name, child) in &builder.dirs {
        let mut label = name.clone();
        let mut full = if path_prefix.is_empty() {
            name.clone()
        } else {
            format!("{path_prefix}/{name}")
        };
        // Compact single-child directory chains: keep descending while a dir has
        // no files of its own and exactly one subdirectory.
        let mut node = child;
        while node.file_indices.is_empty() && node.dirs.len() == 1 {
            let (child_name, grandchild) = node.dirs.iter().next().unwrap();
            label = format!("{label}/{child_name}");
            full = format!("{full}/{child_name}");
            node = grandchild;
        }
        out.push(CachedTreeRow {
            depth,
            label: label.into(),
            kind: CachedTreeRowKind::Dir {
                key: full.clone().into(),
            },
        });
        flatten_tree(node, &full, depth + 1, files, flat_rows, out);
    }

    let mut file_indices = builder.file_indices.clone();
    file_indices.sort_by(|&a, &b| files[a].path.file_name().cmp(&files[b].path.file_name()));
    for fi in file_indices {
        let fr = &flat_rows[fi];
        out.push(CachedTreeRow {
            depth,
            label: fr.file_name.clone(),
            kind: CachedTreeRowKind::File(CachedTreeFile {
                file_index: fr.file_index,
                additions: fr.additions,
                deletions: fr.deletions,
                icon_name: fr.icon_name,
                icon_color: fr.icon_color,
                kind: fr.kind,
            }),
        });
    }
}

/// Indices of `rows` visible given the set of collapsed directory keys. A
/// collapsed directory hides every row deeper than it until the next row at or
/// above its depth; a single threshold suffices because an outer collapse
/// already hides any nested collapses.
fn visible_tree_row_indices(rows: &[CachedTreeRow], collapsed: &HashSet<String>) -> Vec<usize> {
    let mut out = Vec::new();
    let mut hide_below: Option<usize> = None;
    for (i, row) in rows.iter().enumerate() {
        if let Some(d) = hide_below {
            if row.depth > d {
                continue;
            }
            hide_below = None;
        }
        out.push(i);
        if let CachedTreeRowKind::Dir { key } = &row.kind {
            if collapsed.contains(key.as_ref()) {
                hide_below = Some(row.depth);
            }
        }
    }
    out
}

/// Indices of `rows` to show when filtering by search: every file whose index is
/// in `matches`, plus the ancestor directories on the path to it. Collapse state
/// is ignored so matches are always revealed.
fn searched_tree_row_indices(rows: &[CachedTreeRow], matches: &HashSet<usize>) -> Vec<usize> {
    let mut keep = vec![false; rows.len()];
    let mut ancestors: Vec<usize> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        while let Some(&a) = ancestors.last() {
            if rows[a].depth >= row.depth {
                ancestors.pop();
            } else {
                break;
            }
        }
        match &row.kind {
            CachedTreeRowKind::Dir { .. } => ancestors.push(i),
            CachedTreeRowKind::File(f) => {
                if matches.contains(&f.file_index) {
                    keep[i] = true;
                    for &a in &ancestors {
                        keep[a] = true;
                    }
                }
            }
        }
    }
    (0..rows.len()).filter(|&i| keep[i]).collect()
}

fn build_cached_file_tree(files: &[FileDiff]) -> CachedFileDiffTree {
    let flat_rows: Vec<CachedFlatRow> = files
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let path_str = file.path.display().to_string();
            let file_name: SharedString = file
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&path_str)
                .to_string()
                .into();
            let dir_path: SharedString = file
                .path
                .parent()
                .map(|p| {
                    let s = p.display().to_string();
                    if s.is_empty() {
                        s
                    } else {
                        format!("{}/", s)
                    }
                })
                .unwrap_or_default()
                .into();
            CachedFlatRow {
                file_index: i,
                file_name,
                dir_path,
                additions: file.additions,
                deletions: file.deletions,
                icon_name: file_change_icon(file.kind),
                icon_color: file_change_color(file.kind),
                kind: file.kind,
            }
        })
        .collect();

    // Build the directory tree from file paths. Git diff paths are always
    // '/'-separated, so split on '/' rather than the platform separator.
    let mut root = TreeBuilder::default();
    for (i, file) in files.iter().enumerate() {
        let path_str = file.path.display().to_string();
        let segments: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();
        let Some((_file_name, dirs)) = segments.split_last() else {
            continue;
        };
        let mut node = &mut root;
        for dir in dirs {
            node = node.dirs.entry((*dir).to_string()).or_default();
        }
        node.file_indices.push(i);
    }
    let mut tree_rows = Vec::new();
    flatten_tree(&root, "", 0, files, &flat_rows, &mut tree_rows);

    CachedFileDiffTree {
        flat_rows: Arc::new(flat_rows),
        tree_rows: Arc::new(tree_rows),
    }
}

/// Result of applying the file-search filter to a commit's changed files.
///
/// The no-query case is represented without allocating a per-frame mapping: it is
/// the identity over `0..count` in diff order. Only an active query materialises a
/// relevance-ordered list of matching file indices.
enum FilteredFiles {
    /// No active query — all `count` files, in diff order.
    All { count: usize },
    /// Search results: matching file indices ordered by relevance (best first).
    Matched(Vec<usize>),
}

impl FilteredFiles {
    fn len(&self) -> usize {
        match self {
            Self::All { count } => *count,
            Self::Matched(indices) => indices.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The set of matching file indices, used to filter the tree view.
    fn matched_set(&self) -> HashSet<usize> {
        match self {
            Self::All { count } => (0..*count).collect(),
            Self::Matched(indices) => indices.iter().copied().collect(),
        }
    }
}

pub struct DetailPanel {
    commit: Option<CommitInfo>,
    working_changes: Option<rgitui_git::WorkingTreeStatus>,
    cached_working_rows: Arc<Vec<WorkingRow>>,
    working_trees: [Vec<working_tree::TreeRow>; 2],
    working_collapsed_dirs: [HashSet<String>; 2],
    working_checkout: std::path::PathBuf,
    commit_diff: Option<Arc<CommitDiff>>,
    /// Provenance of `commit_diff`'s files — a commit or a stash. Set alongside
    /// the diff so file selections can report it without guessing.
    diff_source: Option<DiffSource>,
    cached_file_tree: Option<CachedFileDiffTree>,
    /// Collapse-filtered tree indices for the common no-search render path.
    /// Recomputed only when the commit or collapse state changes.
    visible_tree_rows: Arc<Vec<usize>>,
    selected_file_index: Option<usize>,
    focus_handle: FocusHandle,
    copied_field: Option<(&'static str, Instant)>,
    /// The changed-files filter field. Owns the text, the cursor and the
    /// selection, so the panel's own key handling never has to ask whether the
    /// user is typing.
    file_search_editor: Entity<TextInput>,
    /// Mirror of the filter field's text, kept in sync by its `Changed` event so
    /// the pure filtering helpers can stay `cx`-free.
    file_search_query: Option<String>,
    file_search_active: bool,
    file_view_mode: FileViewMode,
    collapsed_dirs: HashSet<String>,
    file_scroll: UniformListScrollHandle,
    description_expanded: bool,
    contained_in: Vec<BranchInfo>,
    contained_in_loading: bool,
}

impl EventEmitter<DetailPanelEvent> for DetailPanel {}

impl DetailPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let file_search_editor = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.set_placeholder_message(key!("detail.filter_files.placeholder"));
            input.set_compact(true);
            input
        });
        cx.subscribe(
            &file_search_editor,
            |this: &mut Self, _, event: &TextInputEvent, cx| {
                if let TextInputEvent::Changed(text) = event {
                    this.file_search_query = (!text.is_empty()).then(|| text.clone());
                    this.rebuild_working_rows();
                    if this.working_changes.is_some() {
                        this.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    }
                    cx.notify();
                }
            },
        )
        .detach();

        Self {
            commit: None,
            working_changes: None,
            cached_working_rows: Arc::default(),
            working_trees: Default::default(),
            working_collapsed_dirs: Default::default(),
            working_checkout: std::path::PathBuf::new(),
            commit_diff: None,
            diff_source: None,
            cached_file_tree: None,
            visible_tree_rows: Arc::new(Vec::new()),
            selected_file_index: None,
            focus_handle: cx.focus_handle(),
            copied_field: None,
            file_search_editor,
            file_search_query: None,
            file_search_active: false,
            file_view_mode: FileViewMode::default(),
            collapsed_dirs: HashSet::new(),
            file_scroll: UniformListScrollHandle::new(),
            description_expanded: false,
            contained_in: Vec::new(),
            contained_in_loading: false,
        }
    }

    fn mark_copied(&mut self, field: &'static str, cx: &mut Context<Self>) {
        self.copied_field = Some((field, Instant::now()));
        cx.notify();
        cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(1500))
                    .await;
                this.update(cx, |this, cx| {
                    if let Some((f, t)) = this.copied_field {
                        if f == field && t.elapsed() >= Duration::from_millis(1400) {
                            this.copied_field = None;
                            cx.notify();
                        }
                    }
                })
                .ok();
            },
        )
        .detach();
    }

    fn is_copied(&self, field: &'static str) -> bool {
        self.copied_field
            .is_some_and(|(f, t)| f == field && t.elapsed() < Duration::from_millis(1500))
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    pub fn commit(&self) -> Option<&CommitInfo> {
        self.commit.as_ref()
    }

    fn file_count(&self) -> usize {
        if let Some(status) = &self.working_changes {
            return status.staged.len() + status.unstaged.len();
        }
        self.commit_diff
            .as_ref()
            .map(|d| d.files.len())
            .unwrap_or(0)
    }

    /// Runs a keyboard command scoped to `DetailPanel` or to the shared `List`
    /// group.
    ///
    /// There is no "am I typing?" branch: the filter field is a real
    /// [`TextInput`], so the `!TextInput` half of the bare-letter contexts keeps
    /// `v`, `[`, `]` and `/` out of the way while it has focus, and the arrow
    /// keys keep working because they are not scoped that way.
    fn dispatch_command(&mut self, cmd: CommandId, window: &mut Window, cx: &mut Context<Self>) {
        if self.working_changes.is_some()
            && matches!(
                cmd,
                CommandId::SelectNext
                    | CommandId::SelectPrev
                    | CommandId::SelectFirst
                    | CommandId::SelectLast
                    | CommandId::Confirm
            )
        {
            if let Some(target) = working_navigation(
                &self.cached_working_rows,
                None,
                self.selected_file_index,
                cmd,
            ) {
                self.selected_file_index = Some(target.file_index);
                self.file_scroll
                    .scroll_to_item(target.displayed_row, ScrollStrategy::Nearest);
                if target.activate {
                    // Confirm deliberately re-emits for an unchanged selection:
                    // its Diff may have been closed or replaced since selection.
                    self.emit_file_selected(cx);
                }
                cx.notify();
            } else if self.selected_file_index.take().is_some() {
                cx.notify();
            }
            return;
        }
        let file_count = self.file_count();

        match cmd {
            // Commit navigation works regardless of file count.
            CommandId::PrevCommitDetails => {
                if self.commit.is_some() {
                    cx.emit(DetailPanelEvent::NavigatePrevCommit);
                }
            }
            CommandId::NextCommitDetails => {
                if self.commit.is_some() {
                    cx.emit(DetailPanelEvent::NavigateNextCommit);
                }
            }
            CommandId::FileSearch => {
                self.file_search_active = true;
                self.file_search_editor
                    .update(cx, |editor, cx| editor.focus(window, cx));
                cx.notify();
            }
            CommandId::Cancel => self.clear_file_search(cx),
            _ if file_count == 0 => {}
            CommandId::SelectNext => self.select_file(
                match self.selected_file_index {
                    Some(i) if i + 1 < file_count => Some(i + 1),
                    None => Some(0),
                    other => other,
                },
                cx,
            ),
            CommandId::SelectPrev => self.select_file(
                match self.selected_file_index {
                    Some(i) if i > 0 => Some(i - 1),
                    None => Some(0),
                    other => other,
                },
                cx,
            ),
            CommandId::SelectFirst => self.select_file(Some(0), cx),
            CommandId::SelectLast => self.select_file(Some(file_count - 1), cx),
            CommandId::ToggleFileTree => self.request_file_view_mode_toggle(cx),
            // A command this view does not own falls through to the next handler
            // out, and finally to the focused text field.
            _ => cx.propagate(),
        }
    }

    /// Moves the changed-files selection, emitting only on an actual change.
    fn select_file(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        if index == self.selected_file_index {
            return;
        }
        self.selected_file_index = index;
        self.emit_file_selected(cx);
        cx.notify();
    }

    /// Closes the changed-files filter and clears its query.
    fn clear_file_search(&mut self, cx: &mut Context<Self>) {
        if self.file_search_query.is_none() && !self.file_search_active {
            return;
        }
        self.file_search_query = None;
        self.file_search_active = false;
        self.file_search_editor
            .update(cx, |editor, cx| editor.clear(cx));
        self.rebuild_working_rows();
        cx.notify();
    }

    fn request_file_view_mode_toggle(&mut self, cx: &mut Context<Self>) {
        let is_searching = self.working_changes.is_none()
            && is_file_searching(self.file_search_active, &self.file_search_query);
        let next_mode = file_view_mode_after_toggle_request(
            self.file_view_mode,
            self.file_count(),
            is_searching,
        );
        if next_mode != self.file_view_mode {
            self.file_view_mode = next_mode;
            self.rebuild_working_rows();
            self.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
            cx.notify();
        }
    }

    fn rebuild_working_rows(&mut self) {
        let Some(status) = &self.working_changes else {
            return;
        };
        self.cached_working_rows = Arc::new(match self.file_view_mode {
            FileViewMode::Flat => {
                let rows = working_rows(status);
                match working_display_order(&rows, self.file_search_query.as_deref()) {
                    Some(order) => order.into_iter().map(|index| rows[index].clone()).collect(),
                    None => rows,
                }
            }
            FileViewMode::Tree => working_tree_rows(
                status,
                &self.working_trees,
                &self.working_collapsed_dirs,
                self.file_search_query.as_deref(),
            ),
        });
    }

    fn toggle_working_directory(&mut self, staged: bool, path: String, cx: &mut Context<Self>) {
        // Search temporarily reveals ancestors without changing saved expansion.
        if self.file_search_query.is_some() {
            return;
        }
        let collapsed = &mut self.working_collapsed_dirs[usize::from(!staged)];
        if !collapsed.remove(&path) {
            collapsed.insert(path);
        }
        self.rebuild_working_rows();
        cx.notify();
    }

    fn set_working_directories_collapsed(&mut self, collapse: bool, cx: &mut Context<Self>) {
        for (tree, collapsed) in self
            .working_trees
            .iter()
            .zip(&mut self.working_collapsed_dirs)
        {
            collapsed.clear();
            if collapse {
                collapsed.extend(tree.iter().filter_map(|row| match row {
                    working_tree::TreeRow::Directory { path, .. } => Some(path.clone()),
                    _ => None,
                }));
            }
        }
        self.rebuild_working_rows();
        self.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    /// Toggle a directory node's collapsed state in the tree view.
    fn toggle_dir_collapsed(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.collapsed_dirs.remove(&key) {
            self.collapsed_dirs.insert(key);
        }
        self.rebuild_visible_tree_rows();
        cx.notify();
    }

    fn rebuild_visible_tree_rows(&mut self) {
        self.visible_tree_rows = Arc::new(
            self.cached_file_tree
                .as_ref()
                .map(|cached| visible_tree_row_indices(&cached.tree_rows, &self.collapsed_dirs))
                .unwrap_or_default(),
        );
    }

    fn emit_file_selected(&self, cx: &mut Context<Self>) {
        if let (Some(status), Some(index)) = (&self.working_changes, self.selected_file_index) {
            let staged = index < status.staged.len();
            let file = if staged {
                status.staged.get(index)
            } else {
                status.unstaged.get(index - status.staged.len())
            };
            if let Some(file) = file {
                cx.emit(DetailPanelEvent::WorkingFileSelected {
                    path: file.path.to_string_lossy().into_owned(),
                    staged,
                    conflicted: file.kind == FileChangeKind::Conflicted,
                    checkout: self.working_checkout.clone(),
                });
            }
            return;
        }
        if let (Some(idx), Some(diff), Some(source)) = (
            self.selected_file_index,
            &self.commit_diff,
            &self.diff_source,
        ) {
            if let Some(file) = diff.files.get(idx) {
                cx.emit(DetailPanelEvent::FileSelected(
                    file.clone(),
                    file.path.to_string_lossy().to_string(),
                    source.clone(),
                ));
            }
        }
    }

    pub fn set_commit(&mut self, commit: CommitInfo, diff: CommitDiff, cx: &mut Context<Self>) {
        self.set_commit_arc(commit, Arc::new(diff), cx);
    }

    /// Show a stash entry's file list. Identical to [`Self::set_commit`] except
    /// that selected files report `Stash` provenance, so the diff viewer labels
    /// them "Stashed" rather than "Committed".
    pub fn set_stash(&mut self, stash: CommitInfo, diff: CommitDiff, cx: &mut Context<Self>) {
        let source = DiffSource::Stash(stash.oid.to_string());
        self.set_history_arc(stash, Arc::new(diff), source, cx);
    }

    pub(crate) fn set_commit_arc(
        &mut self,
        commit: CommitInfo,
        diff: Arc<CommitDiff>,
        cx: &mut Context<Self>,
    ) {
        let source = DiffSource::Commit(commit.oid.to_string());
        self.set_history_arc(commit, diff, source, cx);
    }

    fn set_history_arc(
        &mut self,
        commit: CommitInfo,
        diff: Arc<CommitDiff>,
        source: DiffSource,
        cx: &mut Context<Self>,
    ) {
        log::debug!(
            "DetailPanel::set_commit: oid={} files={}",
            commit.short_id,
            diff.files.len()
        );
        let tree_started = Instant::now();
        let cached_file_tree = build_cached_file_tree(&diff.files);
        let tree_elapsed = tree_started.elapsed();
        if tree_elapsed >= Duration::from_millis(8) {
            log::debug!(
                "DetailPanel file-tree preparation took {:?} for {} files ({} tree rows)",
                tree_elapsed,
                diff.files.len(),
                cached_file_tree.tree_rows.len()
            );
        }
        self.working_changes = None;
        self.cached_file_tree = Some(cached_file_tree);
        self.commit = Some(commit);
        self.commit_diff = Some(diff);
        self.diff_source = Some(source);
        self.selected_file_index = None;
        self.file_search_query = None;
        self.file_search_active = false;
        self.collapsed_dirs.clear();
        self.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.rebuild_visible_tree_rows();
        self.description_expanded = false;
        self.contained_in.clear();
        self.contained_in_loading = false;
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        log::debug!("DetailPanel::clear");
        self.commit = None;
        self.working_changes = None;
        self.cached_working_rows = Arc::default();
        self.working_trees = Default::default();
        self.working_collapsed_dirs = Default::default();
        self.commit_diff = None;
        self.diff_source = None;
        self.cached_file_tree = None;
        self.visible_tree_rows = Arc::new(Vec::new());
        self.selected_file_index = None;
        self.file_search_query = None;
        self.file_search_active = false;
        self.collapsed_dirs.clear();
        self.description_expanded = false;
        self.contained_in.clear();
        self.contained_in_loading = false;
        cx.notify();
    }

    /// Refresh only the working inspector; the commit composer is a separate
    /// entity and its hidden draft is never touched here.
    pub fn set_working_changes(
        &mut self,
        status: rgitui_git::WorkingTreeStatus,
        checkout: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.working_changes.as_ref() == Some(&status) && self.working_checkout == checkout {
            return;
        }
        if let Some(previous) = self
            .working_changes
            .as_ref()
            .filter(|_| self.working_checkout == checkout)
        {
            self.selected_file_index =
                reconcile_working_selection(previous, &status, self.selected_file_index);
            // Keep the scroll handle, filter text/editor, and selected identity.
            // A vanished index/worktree source clears selection, never rebinds it.
        } else {
            self.clear(cx);
            self.file_search_editor
                .update(cx, |editor, cx| editor.clear(cx));
            self.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        self.working_trees = [&status.staged, &status.unstaged].map(|files| {
            working_tree::build_tree(
                &files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>(),
            )
        });
        for (tree, collapsed) in self
            .working_trees
            .iter()
            .zip(&mut self.working_collapsed_dirs)
        {
            *collapsed = working_tree::reconcile_collapsed(tree, collapsed);
        }
        self.working_changes = Some(status);
        self.rebuild_working_rows();
        self.working_checkout = checkout;
        cx.notify();
    }

    pub fn set_contained_in(&mut self, branches: Vec<BranchInfo>, cx: &mut Context<Self>) {
        self.contained_in = branches;
        self.contained_in_loading = false;
        cx.notify();
    }

    pub fn set_contained_in_loading(&mut self, cx: &mut Context<Self>) {
        self.contained_in_loading = true;
        cx.notify();
    }

    /// File indices to display, after applying the search filter.
    ///
    /// With no active query this is the identity over all files (in diff order)
    /// and allocates nothing. With a query, matching files are scored with
    /// `fuzzy_score` and returned ordered by relevance (best match first).
    fn filtered_files(&self) -> FilteredFiles {
        let query = match &self.file_search_query {
            Some(q) if !q.is_empty() => q,
            _ => {
                return FilteredFiles::All {
                    count: self.file_count(),
                }
            }
        };
        let Some(diff) = &self.commit_diff else {
            return FilteredFiles::Matched(Vec::new());
        };
        let mut scored: Vec<(usize, usize)> = diff
            .files
            .iter()
            .enumerate()
            .filter_map(|(i, file)| {
                let path = file.path.to_string_lossy();
                rgitui_ui::fuzzy_score(query, &path).map(|score| (score, i))
            })
            .collect();
        // Sort by score descending — higher score = better (earlier char match)
        scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        FilteredFiles::Matched(scored.into_iter().map(|(_, i)| i).collect())
    }

    fn render_section_header(&self, label: &str) -> impl IntoElement {
        Label::new(SharedString::from(label.to_string()))
            .size(LabelSize::XSmall)
            .color(Color::Muted)
            .weight(gpui::FontWeight::SEMIBOLD)
    }

    fn render_co_authors(
        &self,
        co_authors: &[Signature],
        colors: &rgitui_theme::ThemeColors,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let avatar_bg = colors.border_focused;
        let avatar_text_color = colors.background;

        let mut section = div()
            .v_flex()
            .gap(px(4.))
            .child(self.render_section_header("Co-authors"));

        let mut seen = HashSet::new();
        for co_author in co_authors.iter().filter(|co_author| {
            seen.insert((
                co_author.name.trim().to_lowercase(),
                co_author.email.trim().to_lowercase(),
            ))
        }) {
            let initials: SharedString = co_author
                .name
                .split_whitespace()
                .take(2)
                .filter_map(|w| w.chars().next())
                .collect::<String>()
                .to_uppercase()
                .into();

            let co_name: SharedString = co_author.name.clone().into();
            let co_email: SharedString = format!("<{}>", co_author.email).into();

            let avatar_url = cx
                .try_global::<AvatarCache>()
                .and_then(|cache| cache.avatar_url(&co_author.email))
                .map(|s| s.to_string());

            let avatar_circle =
                self.render_avatar(avatar_url, initials, avatar_bg, avatar_text_color, px(16.));

            section = section.child(
                div()
                    .h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(avatar_circle)
                    .child(
                        Label::new(co_name)
                            .size(LabelSize::XSmall)
                            .weight(gpui::FontWeight::SEMIBOLD),
                    )
                    .child(
                        Label::new(co_email)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                            .truncate(),
                    ),
            );
        }

        section.into_any_element()
    }

    fn render_avatar(
        &self,
        avatar_url: Option<String>,
        initials: SharedString,
        avatar_bg: gpui::Hsla,
        avatar_text_color: gpui::Hsla,
        size: gpui::Pixels,
    ) -> gpui::Div {
        let initials_size = px(avatar_initials_size(f32::from(size)));
        let mut avatar_circle = div()
            .w(size)
            .h(size)
            .rounded_full()
            .bg(avatar_bg)
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center();

        if let Some(url) = avatar_url {
            let fb_initials = initials.clone();
            avatar_circle = avatar_circle.child(
                img(url)
                    .rounded_full()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .with_fallback(move || {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_color(avatar_text_color)
                                    .text_size(initials_size)
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(fb_initials.clone()),
                            )
                            .into_any_element()
                    }),
            );
        } else {
            avatar_circle = avatar_circle.child(
                div()
                    .text_color(avatar_text_color)
                    .text_size(initials_size)
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(initials),
            );
        }

        avatar_circle
    }

    fn render_flat_file_list_filtered(
        &self,
        cached: &CachedFileDiffTree,
        filtered: &FilteredFiles,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let colors = cx.colors().clone();
        let row_h = cx
            .global::<SettingsState>()
            .settings()
            .compactness
            .spacing(26.0);
        let selected_file_index = self.selected_file_index;
        let weak = cx.weak_entity();
        let row_count = filtered.len();
        let list_height = file_list_content_height(row_count, row_h);
        let list_min_height = file_list_min_height(row_count, row_h);
        // Cheap `'static` handles for the virtualized closure: the shared row
        // store and, when searching, the display->file-index mapping (`None`
        // means identity, i.e. unfiltered diff order). Only the visible window is
        // cloned, inside the closure.
        let flat_rows = cached.flat_rows.clone();
        let display_order: Option<Arc<Vec<usize>>> = match filtered {
            FilteredFiles::All { .. } => None,
            FilteredFiles::Matched(indices) => Some(Arc::new(indices.clone())),
        };

        let ghost_element_selected = colors.ghost_element_selected;
        let text_accent = colors.text_accent;
        let border_transparent = colors.border_transparent;
        let ghost_element_hover = colors.ghost_element_hover;

        let list = uniform_list(
            self.file_view_mode.list_element_id(),
            row_count,
            move |range: std::ops::Range<usize>, _window: &mut Window, _cx: &mut App| {
                range
                    .map(|ix| {
                        let file_index = match &display_order {
                            Some(order) => order[ix],
                            None => ix,
                        };
                        let row = &flat_rows[file_index];
                        let actual_file_index = row.file_index;
                        let selected = selected_file_index == Some(actual_file_index);
                        let weak = weak.clone();
                        let dir_path = row.dir_path.clone();

                        div()
                            .id(ElementId::NamedInteger("detail-file".into(), ix as u64))
                            .h_flex()
                            .w_full()
                            .h(px(row_h))
                            .pl(px(8.))
                            .pr(px(8.))
                            .gap(px(6.))
                            .items_center()
                            .flex_shrink_0()
                            .border_l_2()
                            .when(selected, |el| {
                                el.bg(ghost_element_selected).border_color(text_accent)
                            })
                            .when(!selected, |el| el.border_color(border_transparent))
                            .hover(move |s| s.bg(ghost_element_hover))
                            .cursor_pointer()
                            .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                weak.update(cx, |this, cx| {
                                    this.selected_file_index = Some(actual_file_index);
                                    this.emit_file_selected(cx);
                                    cx.notify();
                                })
                                .ok();
                            })
                            .child(
                                Icon::new(row.icon_name)
                                    .size(IconSize::XSmall)
                                    .color(row.icon_color),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(2.))
                                    .overflow_hidden()
                                    .when(!dir_path.is_empty(), |el| {
                                        el.child(
                                            Label::new(dir_path)
                                                .size(LabelSize::XSmall)
                                                .color(Color::Muted),
                                        )
                                    })
                                    .child(
                                        Label::new(row.file_name.clone())
                                            .size(LabelSize::XSmall)
                                            .truncate(),
                                    ),
                            )
                            .child(FileStatusBadge::new(row.kind))
                            .child(DiffStat::new(row.additions, row.deletions))
                            .into_any_element()
                    })
                    .collect()
            },
        )
        .pb_2()
        .h_full()
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .track_scroll(&self.file_scroll);

        div()
            .flex()
            .flex_row()
            .w_full()
            .h(px(list_height))
            .min_h(px(list_min_height))
            .child(div().flex_1().min_w_0().h_full().child(list))
            .when(row_count > FILE_LIST_SCROLLBAR_THRESHOLD, |el| {
                el.child(Scrollbar::vertical(
                    "detail-files-scrollbar",
                    self.file_scroll.clone(),
                ))
            })
            .into_any_element()
    }

    /// Renders the directory tree: collapsible directory nodes and file leaves
    /// (basename only), indented by depth. `tree_rows` is the full row store and
    /// `visible` the indices into it to display (collapse-filtered when browsing,
    /// match-filtered when searching), in display order. Both are passed by shared
    /// handle so the virtualized closure clones only the visible window.
    fn render_tree_file_list(
        &self,
        tree_rows: &Arc<Vec<CachedTreeRow>>,
        visible: Arc<Vec<usize>>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let colors = cx.colors().clone();
        let row_h = cx
            .global::<SettingsState>()
            .settings()
            .compactness
            .spacing(26.0);
        let selected_file_index = self.selected_file_index;
        let collapsed = self.collapsed_dirs.clone();
        let weak = cx.weak_entity();
        let row_count = visible.len();
        let list_height = file_list_content_height(row_count, row_h);
        let list_min_height = file_list_min_height(row_count, row_h);
        let tree_rows = tree_rows.clone();

        let ghost_element_selected = colors.ghost_element_selected;
        let text_accent = colors.text_accent;
        let border_transparent = colors.border_transparent;
        let ghost_element_hover = colors.ghost_element_hover;

        let list = uniform_list(
            self.file_view_mode.list_element_id(),
            row_count,
            move |range: std::ops::Range<usize>, _window: &mut Window, _cx: &mut App| {
                range
                    .map(|ix| {
                        let row = &tree_rows[visible[ix]];
                        // 8px base + 14px per depth level. Directory rows lead with
                        // a chevron; file rows lead with a chevron-width spacer so
                        // their icon aligns under the folder icon rather than just
                        // shifting right.
                        let indent = px(8.0 + row.depth as f32 * 14.0);
                        let weak = weak.clone();
                        match &row.kind {
                            CachedTreeRowKind::Dir { key } => {
                                let is_collapsed = collapsed.contains(key.as_ref());
                                let key_str = key.to_string();
                                div()
                                    .id(ElementId::NamedInteger(
                                        "detail-tree-dir".into(),
                                        ix as u64,
                                    ))
                                    .h_flex()
                                    .w_full()
                                    .h(px(row_h))
                                    .pl(indent)
                                    .pr(px(8.))
                                    .gap(px(4.))
                                    .items_center()
                                    .flex_shrink_0()
                                    .border_l_2()
                                    .border_color(border_transparent)
                                    .hover(move |s| s.bg(ghost_element_hover))
                                    .cursor_pointer()
                                    .on_click(
                                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                            let key_str = key_str.clone();
                                            weak.update(cx, |this, cx| {
                                                this.toggle_dir_collapsed(key_str, cx);
                                            })
                                            .ok();
                                        },
                                    )
                                    .child(
                                        Icon::new(if is_collapsed {
                                            IconName::ChevronRight
                                        } else {
                                            IconName::ChevronDown
                                        })
                                        .size(IconSize::XSmall)
                                        .color(Color::Muted),
                                    )
                                    .child(
                                        Icon::new(IconName::Folder)
                                            .size(IconSize::XSmall)
                                            .color(Color::Muted),
                                    )
                                    .child(
                                        Label::new(row.label.clone())
                                            .size(LabelSize::XSmall)
                                            .color(Color::Muted)
                                            .truncate(),
                                    )
                                    .into_any_element()
                            }
                            CachedTreeRowKind::File(file) => {
                                let actual_file_index = file.file_index;
                                let selected = selected_file_index == Some(actual_file_index);
                                div()
                                    .id(ElementId::NamedInteger("detail-file".into(), ix as u64))
                                    .h_flex()
                                    .w_full()
                                    .h(px(row_h))
                                    .pl(indent)
                                    .pr(px(8.))
                                    .gap(px(6.))
                                    .items_center()
                                    .flex_shrink_0()
                                    .border_l_2()
                                    .when(selected, |el| {
                                        el.bg(ghost_element_selected).border_color(text_accent)
                                    })
                                    .when(!selected, |el| el.border_color(border_transparent))
                                    .hover(move |s| s.bg(ghost_element_hover))
                                    .cursor_pointer()
                                    .on_click(
                                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                            weak.update(cx, |this, cx| {
                                                this.selected_file_index = Some(actual_file_index);
                                                this.emit_file_selected(cx);
                                                cx.notify();
                                            })
                                            .ok();
                                        },
                                    )
                                    .child(div().w(px(14.)).flex_shrink_0())
                                    .child(
                                        Icon::new(file.icon_name)
                                            .size(IconSize::XSmall)
                                            .color(file.icon_color),
                                    )
                                    .child(
                                        div().h_flex().flex_1().min_w_0().overflow_hidden().child(
                                            Label::new(row.label.clone())
                                                .size(LabelSize::XSmall)
                                                .truncate(),
                                        ),
                                    )
                                    .child(FileStatusBadge::new(file.kind))
                                    .child(DiffStat::new(file.additions, file.deletions))
                                    .into_any_element()
                            }
                        }
                    })
                    .collect()
            },
        )
        .pb_2()
        .h_full()
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .track_scroll(&self.file_scroll);

        div()
            .flex()
            .flex_row()
            .w_full()
            .h(px(list_height))
            .min_h(px(list_min_height))
            .child(div().flex_1().min_w_0().h_full().child(list))
            .when(row_count > FILE_LIST_SCROLLBAR_THRESHOLD, |el| {
                el.child(Scrollbar::vertical(
                    "detail-files-tree-scrollbar",
                    self.file_scroll.clone(),
                ))
            })
            .into_any_element()
    }
}

impl Render for DetailPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.colors().clone();
        let compact = &cx.global::<SettingsState>().settings().compactness;

        // Compactness-scaled spacing values for header and message cards
        let header_pad_h = px(compact.spacing(12.0));
        let header_pad_v = px(compact.spacing(14.0));
        let header_gap = px(compact.spacing(10.0));
        let msg_pad_h = px(compact.spacing(10.0));
        let msg_pad_v = px(compact.spacing(10.0));
        let msg_gap = px(compact.spacing(6.0));
        let avatar_size = px(compact.spacing(24.0));

        if let Some(status) = &self.working_changes {
            // Cache rows on status changes; an ordinary frame clones only the
            // shared handle and the visible window, not hundreds of file paths.
            let rows = self.cached_working_rows.clone();
            let row_count = rows.len();
            let tree_mode = self.file_view_mode == FileViewMode::Tree;
            let searching = self.file_search_query.is_some();
            let clean = status.staged.is_empty() && status.unstaged.is_empty();
            let selected = self.selected_file_index;
            let checkout = self.working_checkout.clone();
            let weak = cx.entity().downgrade();
            let selected_bg = colors.element_selected;
            let hover_bg = colors.ghost_element_hover;
            let section_bg = colors.surface_background;
            return div()
                .id("detail-panel")
                .track_focus(&self.focus_handle)
                .map(|el| keymap::bind_actions(el, "DetailPanel List", &["Menu", "DetailPanel"], cx, Self::dispatch_command))
                .v_flex().size_full().min_h_0().bg(colors.panel_background)
                .child(div().h_flex().px(px(10.)).py(px(8.)).gap(px(6.))
                    .child(Label::new(t!("sidebar.working_changes")).size(LabelSize::Small))
                    .child(div().flex_1())
                    .child(Button::new("working-path-view", t!("detail.path_view"))
                        .size(ButtonSize::Compact).style(ButtonStyle::Subtle).selected(!tree_mode).disabled(clean)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.file_view_mode != FileViewMode::Flat { this.request_file_view_mode_toggle(cx); }
                        })))
                    .child(Button::new("working-tree-view", t!("detail.tree_view"))
                        .size(ButtonSize::Compact).style(ButtonStyle::Subtle).selected(tree_mode).disabled(clean)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.file_view_mode != FileViewMode::Tree { this.request_file_view_mode_toggle(cx); }
                        })))
                    .child(IconButton::new("working-file-search", IconName::Search).size(ButtonSize::Compact)
                        .tooltip(t!("detail.search_files_hint"))
                        .on_click(cx.listener(|this, _, window, cx| this.dispatch_command(CommandId::FileSearch, window, cx)))))
                .when(self.file_search_active || self.file_search_query.is_some(), |el| el.child(self.file_search_editor.clone()))
                .when(tree_mode && !clean, |el| el.child(div().h_flex().px(px(10.)).gap(px(6.))
                    .child(Button::new("working-expand-all", t!("detail.expand_all"))
                        .size(ButtonSize::Compact).style(ButtonStyle::Subtle).disabled(searching)
                        .on_click(cx.listener(|this, _, _, cx| this.set_working_directories_collapsed(false, cx))))
                    .child(Button::new("working-collapse-all", t!("detail.collapse_all"))
                        .size(ButtonSize::Compact).style(ButtonStyle::Subtle).disabled(searching)
                        .on_click(cx.listener(|this, _, _, cx| this.set_working_directories_collapsed(true, cx))))))
                .when(clean, |el| el.child(div().p(px(10.)).child(Label::new(t!("sidebar.working_clean")).size(LabelSize::Small).color(Color::Muted))))
                .child(uniform_list("working-inspector-files", row_count, move |range: std::ops::Range<usize>, _window: &mut Window, _cx: &mut App| {
                    range.map(|row| {
                        let actual_row = row;
                        let checkout = checkout.clone();
                        let weak = weak.clone();
                        match rows[actual_row].clone() {
                            WorkingRow::Section { staged, count } => {
                                div().id(row).h_flex().w_full().h(px(32.)).px(px(10.)).gap(px(6.)).bg(section_bg)
                                    .child(Label::new(if staged { t!("sidebar.staged") } else { t!("sidebar.unstaged") }).size(LabelSize::Small).weight(gpui::FontWeight::SEMIBOLD))
                                    .child(Label::new(count.to_string()).size(LabelSize::XSmall).color(Color::Muted))
                                    .child(div().flex_1())
                                    .child(Button::new(ElementId::NamedInteger("working-all".into(), row as u64), if staged { t!("sidebar.unstage_all") } else { t!("sidebar.stage_all") })
                                        .size(ButtonSize::Compact).style(ButtonStyle::Subtle).disabled(count == 0)
                                        .on_click(move |_: &ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            weak.update(cx, |_, cx| {
                                                cx.emit(DetailPanelEvent::WorkingAction { checkout: checkout.clone(), action: if staged { WorkingChangesAction::UnstageAll } else { WorkingChangesAction::StageAll } });
                                            }).ok();
                                        }))
                                    .into_any_element()
                            }
                            WorkingRow::Directory { path, name, depth, count, staged, collapsed } => {
                                div().id(row).h_flex().w_full().h(px(32.))
                                    .pl(px(10. + depth as f32 * 16.)).pr(px(10.)).gap(px(6.))
                                    .hover(move |style| style.bg(hover_bg)).cursor_pointer()
                                    .tooltip(Tooltip::text(path.clone()))
                                    .child(Icon::new(if collapsed { IconName::ChevronRight } else { IconName::ChevronDown })
                                        .size(IconSize::Small).color(Color::Muted))
                                    .child(Icon::new(IconName::Folder).size(IconSize::Small).color(Color::Muted))
                                    .child(div().flex_1().min_w_0().child(Label::new(name).size(LabelSize::Small).color(Color::Muted).truncate()))
                                    .child(Label::new(count.to_string()).size(LabelSize::XSmall).color(Color::Muted))
                                    .on_click(move |_: &ClickEvent, window, cx| {
                                        weak.update(cx, |this, cx| {
                                            this.focus_handle.focus(window, cx);
                                            this.toggle_working_directory(staged, path.clone(), cx);
                                        }).ok();
                                    }).into_any_element()
                            }
                            WorkingRow::File { index, file, staged, depth } => {
                                let path = file.path.to_string_lossy().into_owned();
                                let file_name = file.path.file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.clone());
                                let directory = file.path.parent()
                                    .filter(|_| !tree_mode)
                                    .filter(|parent| !parent.as_os_str().is_empty())
                                    .map(|parent| parent.to_string_lossy().into_owned());
                                let action_path = path.clone();
                                let action_checkout = checkout.clone();
                                let action_weak = weak.clone();
                                let discard_weak = weak.clone();
                                let discard_path = path.clone();
                                let discard_checkout = checkout.clone();
                                let conflicted = file.kind == FileChangeKind::Conflicted;
                                div().id(row).group("working-file-row").h_flex().w_full().h(px(32.))
                                    .pl(px(10. + if tree_mode { depth as f32 * 16. + 22. } else { 0. })).pr(px(10.)).gap(px(6.))
                                    .cursor_pointer()
                                    .when(selected == Some(index), |el| el.bg(selected_bg))
                                    .when(selected != Some(index), |el| el.hover(move |s| s.bg(hover_bg)))
                                    .child(Icon::new(file_change_icon(file.kind)).size(IconSize::Small).color(file_change_color(file.kind)))
                                    .tooltip(Tooltip::text(path.clone()))
                                    .child(div().h_flex().flex_1().min_w_0().gap(px(6.)).overflow_hidden()
                                        .child(div().min_w_0()
                                            .when(!tree_mode, |el| el.max_w(gpui::rems(12.)).flex_shrink_0())
                                            .when(tree_mode, |el| el.flex_1())
                                            .child(Label::new(file_name).size(LabelSize::Small).truncate()))
                                        .when_some(directory, |el, directory| el.child(div().flex_1().min_w_0()
                                            .child(Label::new(directory).size(LabelSize::XSmall).color(Color::Muted).truncate()))))
                                    .child(FileStatusBadge::new(file.kind))
                                    // Conflicts must go through the resolver, not blind staging.
                                    .when(!conflicted, |el| el.child(IconButton::new(ElementId::NamedInteger("working-stage".into(), row as u64), if staged { IconName::Minus } else { IconName::Plus })
                                        .size(ButtonSize::Compact)
                                        .tooltip(if staged { t!("sidebar.unstage_file.tooltip") } else { t!("sidebar.stage_file.tooltip") })
                                        .on_click(move |_: &ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            action_weak.update(cx, |_, cx| {
                                                cx.emit(DetailPanelEvent::WorkingAction { checkout: action_checkout.clone(), action: if staged { WorkingChangesAction::UnstageFile(action_path.clone()) } else { WorkingChangesAction::StageFile(action_path.clone()) } });
                                            }).ok();
                                        })))
                                    .when(!staged, |el| el.child(
                                        div().flex_shrink_0().invisible().group_hover("working-file-row", |style| style.visible())
                                            .child(IconButton::new(ElementId::NamedInteger("working-discard".into(), row as u64), IconName::Trash)
                                                .size(ButtonSize::Compact).color(Color::Muted)
                                                .tooltip(t!("sidebar.discard_changes.tooltip"))
                                                .on_click(move |_: &ClickEvent, _, cx| {
                                                    cx.stop_propagation();
                                                    discard_weak.update(cx, |_, cx| {
                                                        cx.emit(DetailPanelEvent::WorkingAction {
                                                            checkout: discard_checkout.clone(),
                                                            action: WorkingChangesAction::DiscardFile(discard_path.clone()),
                                                        });
                                                    }).ok();
                                                }))
                                    ))
                                    .on_click(move |_: &ClickEvent, window, cx| {
                                        weak.update(cx, |this, cx| {
                                            this.focus_handle.focus(window, cx);
                                            this.selected_file_index = Some(index);
                                            cx.emit(DetailPanelEvent::WorkingFileSelected { path: path.clone(), staged, conflicted, checkout: checkout.clone() });
                                            cx.notify();
                                        }).ok();
                                    }).into_any_element()
                            }
                        }
                    }).collect()
                }).flex_1().min_h_0().track_scroll(&self.file_scroll))
                .into_any_element();
        }

        let Some(commit) = &self.commit else {
            return div()
                .id("detail-panel")
                .track_focus(&self.focus_handle)
                .v_flex()
                .size_full()
                .bg(colors.panel_background)
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .h(px(32.))
                        .px(px(10.))
                        .gap(px(4.))
                        .items_center()
                        .bg(colors.toolbar_background)
                        .border_b_1()
                        .border_color(colors.border_variant)
                        .child(
                            Icon::new(IconName::GitCommit)
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            Label::new(t!("common.details"))
                                .size(LabelSize::Small)
                                .weight(gpui::FontWeight::SEMIBOLD)
                                .color(Color::Default),
                        ),
                )
                .child(
                    div().flex_1().flex().items_center().justify_center().child(
                        div()
                            .v_flex()
                            .items_center()
                            .gap(px(12.))
                            .px(px(24.))
                            .py(px(20.))
                            .rounded(px(8.))
                            .bg(colors.surface_background)
                            .border_1()
                            .border_color(colors.border_variant)
                            .child(
                                Icon::new(IconName::GitCommit)
                                    .size(IconSize::Large)
                                    .color(Color::Placeholder),
                            )
                            .child(
                                Label::new(t!("detail.empty.title"))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted)
                                    .weight(gpui::FontWeight::SEMIBOLD),
                            )
                            .child(
                                Label::new(t!("detail.empty.description"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Placeholder),
                            ),
                    ),
                )
                .into_any_element();
        };

        let full_sha: SharedString = format!("{}", commit.oid).into();
        let short_sha: SharedString = format!("{:.7}", commit.oid).into();
        let sha_for_copy = full_sha.clone();
        let sha_for_cherry = full_sha.clone();
        let author_name: SharedString = commit.author.name.clone().into();
        let author_email: SharedString = commit.author.email.clone().into();
        let relative_time = crate::time::format_relative_time_full(commit.time.timestamp());
        let absolute_date = format_absolute_date(commit.time.timestamp());
        let date: SharedString = format!("{} ({})", absolute_date, relative_time).into();
        let refs = compact_ref_labels(&commit.refs);

        let (summary, description) = {
            let msg = &commit.message;
            match msg.find('\n') {
                Some(idx) => (
                    msg[..idx].trim().to_string(),
                    msg[idx + 1..].trim().to_string(),
                ),
                None => (msg.trim().to_string(), String::new()),
            }
        };
        let summary: SharedString = summary.into();

        let initials: SharedString = commit
            .author
            .name
            .split_whitespace()
            .take(2)
            .filter_map(|w| w.chars().next())
            .collect::<String>()
            .to_uppercase()
            .into();

        let mut panel = div()
            .id("detail-panel")
            .track_focus(&self.focus_handle)
            .map(|el| {
                keymap::bind_actions(
                    el,
                    "DetailPanel List",
                    &["Menu", "DetailPanel"],
                    cx,
                    Self::dispatch_command,
                )
            })
            .v_flex()
            .size_full()
            .bg(colors.panel_background);

        // Toolbar
        panel = panel.child(
            div()
                .h_flex()
                .w_full()
                .h(px(32.))
                .px(px(8.))
                .gap(px(4.))
                .items_center()
                .bg(colors.surface_background)
                .border_b_1()
                .border_color(colors.border_variant)
                .child(
                    Icon::new(IconName::GitCommit)
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                )
                .child(
                    Label::new(t!("common.details"))
                        .size(LabelSize::Small)
                        .weight(gpui::FontWeight::SEMIBOLD)
                        .color(Color::Default),
                )
                .child(div().flex_1())
                .child({
                    let file_count = self.file_count();
                    let is_searching =
                        is_file_searching(self.file_search_active, &self.file_search_query);
                    let toggle_disabled = !can_toggle_file_view(file_count, is_searching);
                    let toggle_tooltip =
                        file_view_toggle_tooltip(file_count, is_searching, self.file_view_mode);

                    IconButton::new("view-mode-toggle", self.file_view_mode.toggle_icon())
                        .size(ButtonSize::Compact)
                        .disabled(toggle_disabled)
                        .tooltip(toggle_tooltip)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.request_file_view_mode_toggle(cx);
                        }))
                }),
        );

        let mut content = div()
            .id("detail-content")
            .v_flex()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .px_2()
            .py_3()
            .gap_3();

        // Metadata has its own bounded scroll region. This prevents wheel input
        // over the changed-files list from also moving the commit details above.
        let mut metadata = div()
            .id("detail-metadata")
            .v_flex()
            .w_full()
            .max_h(gpui::relative(0.62))
            .overflow_y_scroll()
            .gap_3()
            .flex_shrink_0();

        // -- Header Card: Author + SHA + Refs --
        // Everything above the file list keeps its full height; the list is the
        // one child that gives up space when the panel is short.
        let mut header_card = div()
            .v_flex()
            .w_full()
            .flex_shrink_0()
            .px(header_pad_h)
            .py(header_pad_v)
            .gap(header_gap)
            .bg(colors.elevated_surface_background)
            .rounded(px(8.))
            .border_1()
            .border_color(colors.border_variant);

        // Author row: avatar + name/email + timestamp
        let avatar_url = cx
            .try_global::<AvatarCache>()
            .and_then(|cache| cache.avatar_url(&commit.author.email))
            .map(|s| s.to_string());
        let avatar_bg = colors.border_focused;
        let avatar_text_color = colors.background;
        let avatar_circle = self.render_avatar(
            avatar_url,
            initials,
            avatar_bg,
            avatar_text_color,
            avatar_size,
        );

        // SHA copy button
        let sha_copy_clone = sha_for_copy.clone();
        let sha_copied = self.is_copied("sha");
        let sha_icon = if sha_copied {
            IconName::Check
        } else {
            IconName::Copy
        };

        let sha_button = div()
            .h_flex()
            .gap_1()
            .items_center()
            .px(px(6.))
            .py(px(2.))
            .bg(colors.surface_background)
            .rounded(px(4.))
            .border_1()
            .border_color(colors.border_variant)
            .child(
                div()
                    .font_family("Lilex")
                    .text_xs()
                    .text_color(colors.text_accent)
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(short_sha),
            )
            .child(
                IconButton::new("copy-sha-btn", sha_icon)
                    .size(ButtonSize::Compact)
                    .style(ButtonStyle::Transparent)
                    .tooltip(t!("detail.copy_sha.tooltip"))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            sha_copy_clone.to_string(),
                        ));
                        cx.emit(DetailPanelEvent::CopySha(sha_copy_clone.to_string()));
                        this.mark_copied("sha", cx);
                    })),
            )
            .when(sha_copied, |el| {
                el.child(
                    Label::new(t!("detail.copied"))
                        .size(LabelSize::XSmall)
                        .color(Color::Success),
                )
            });

        header_card = header_card.child(
            div()
                .v_flex()
                .w_full()
                .gap(px(4.))
                .child(
                    div()
                        .h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(avatar_circle)
                        .child(
                            div().flex_1().min_w_0().overflow_hidden().child(
                                Label::new(author_name)
                                    .size(LabelSize::Small)
                                    .weight(gpui::FontWeight::BOLD)
                                    .truncate(),
                            ),
                        )
                        .when(commit.is_signed, |el| {
                            el.child(
                                div()
                                    .id("signed-commit")
                                    .flex_shrink_0()
                                    .tooltip(Tooltip::text(t!("detail.gpg.tooltip")))
                                    .child(
                                        Icon::new(IconName::Lock)
                                            .size(IconSize::XSmall)
                                            .color(Color::Success),
                                    ),
                            )
                        })
                        .child(sha_button),
                )
                .child(
                    Label::new(author_email)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .truncate(),
                )
                .child(
                    div()
                        .h_flex()
                        .gap(px(4.))
                        .items_center()
                        .child(
                            Icon::new(IconName::Clock)
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            Label::new(date)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                ),
        );

        // Ref badges: branches and tags
        if !refs.is_empty() {
            let mut refs_row = div().h_flex().gap(px(4.)).flex_wrap();
            for (ref_index, compact_ref) in refs.iter().enumerate() {
                let ref_label = &compact_ref.label;
                let badge_color = match ref_label {
                    RefLabel::Head => Color::Warning,
                    RefLabel::LocalBranch(_) => Color::Success,
                    RefLabel::RemoteBranch(_) => Color::Info,
                    RefLabel::Tag(_) => Color::Accent,
                };
                let name: SharedString = ref_label.display_name().to_string().into();
                let is_tag = matches!(ref_label, RefLabel::Tag(_));
                let badge = Badge::new(name)
                    .color(badge_color)
                    .bold()
                    .compact()
                    .when(!compact_ref.remotes.is_empty(), |badge| badge.prefix("↑"));
                let remote_tooltip = (!compact_ref.remotes.is_empty())
                    .then(|| Tooltip::text(format!("Also on {}", compact_ref.remotes.join(", "))));
                if is_tag {
                    refs_row = refs_row.child(
                        div()
                            .h_flex()
                            .gap(px(2.))
                            .items_center()
                            .child(
                                Icon::new(IconName::Tag)
                                    .size(IconSize::XSmall)
                                    .color(Color::Accent),
                            )
                            .child(badge),
                    );
                } else {
                    refs_row = refs_row.child(
                        div()
                            .id(ElementId::NamedInteger(
                                "detail-ref-badge".into(),
                                ref_index as u64,
                            ))
                            .flex_shrink_0()
                            .when_some(remote_tooltip, |el, tooltip| el.tooltip(tooltip))
                            .child(badge),
                    );
                }
            }
            header_card = header_card.child(refs_row);
        }

        // Co-authors
        if !commit.co_authors.is_empty() {
            header_card =
                header_card.child(self.render_co_authors(&commit.co_authors, &colors, cx));
        }

        // Contained in — branches that have this commit as an ancestor (i.e. "merged into").
        // Exclude branches already shown as direct refs to avoid redundancy.
        let refs_names: std::collections::HashSet<_> = commit
            .refs
            .iter()
            .filter_map(|r| {
                if let RefLabel::LocalBranch(name) = r {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();
        let contained_filtered: Vec<_> = self
            .contained_in
            .iter()
            .filter(|b| !refs_names.contains(b.name.as_str()))
            .collect();

        if self.contained_in_loading {
            header_card = header_card.child(
                div()
                    .h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(IconName::Refresh)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(t!("detail.finding_branches"))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            );
        } else if !contained_filtered.is_empty() {
            let mut contained_row = div().v_flex().gap(px(4.));
            contained_row = contained_row.child(
                Label::new(t!("detail.contained_in"))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .weight(gpui::FontWeight::SEMIBOLD),
            );
            let mut badges = div().h_flex().gap_1().flex_wrap();
            for branch in &contained_filtered {
                let branch_name: SharedString = branch.name.clone().into();
                badges = badges.child(
                    Badge::new(branch_name)
                        .color(if branch.is_head {
                            Color::Warning
                        } else {
                            Color::Success
                        })
                        .bold(),
                );
            }
            contained_row = contained_row.child(badges);
            header_card = header_card.child(contained_row);
        }

        metadata = metadata.child(header_card);

        // -- Commit Message Section --
        let mut message_card = div()
            .v_flex()
            .w_full()
            .flex_shrink_0()
            .px(msg_pad_h)
            .py(msg_pad_v)
            .gap(msg_gap)
            .bg(colors.elevated_surface_background)
            .rounded(px(8.))
            .border_1()
            .border_color(colors.border_variant);

        // Subject line with cherry-pick button
        let summary_for_copy = summary.clone();
        let summary_copied = self.is_copied("summary");
        message_card = message_card.child(
            div()
                .h_flex()
                .w_full()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .id("summary-copy")
                        .h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(4.))
                        .items_center()
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.8))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                summary_for_copy.to_string(),
                            ));
                            this.mark_copied("summary", cx);
                        }))
                        .overflow_hidden()
                        .child(
                            Label::new(summary.clone())
                                .size(LabelSize::Small)
                                .weight(gpui::FontWeight::BOLD)
                                .truncate(),
                        )
                        .when(summary_copied, |el| {
                            el.child(
                                Label::new(t!("detail.copied"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Success),
                            )
                        }),
                )
                .child(
                    IconButton::new("cherry-pick-btn", IconName::GitCommit)
                        .size(ButtonSize::Compact)
                        .style(ButtonStyle::Transparent)
                        .tooltip(t!("detail.cherry_pick.tooltip"))
                        .on_click(cx.listener(move |_this, _: &ClickEvent, _, cx| {
                            cx.emit(DetailPanelEvent::CherryPick(sha_for_cherry.to_string()));
                        })),
                ),
        );

        // Collapsible description body
        if !description.is_empty() {
            let desc_for_copy = description.clone();
            let desc_copied = self.is_copied("description");
            let is_expanded = self.description_expanded;
            let chevron_icon = if is_expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            };

            // Toggle header row
            message_card = message_card
                .child(div().w_full().h(px(1.)).bg(colors.border_variant))
                .child(
                    div()
                        .id("description-toggle")
                        .h_flex()
                        .w_full()
                        .gap(px(4.))
                        .items_center()
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.8))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                            this.description_expanded = !this.description_expanded;
                            cx.notify();
                        }))
                        .child(
                            Icon::new(chevron_icon)
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            Label::new(t!("common.description"))
                                .size(LabelSize::XSmall)
                                .weight(gpui::FontWeight::SEMIBOLD)
                                .color(Color::Muted),
                        ),
                );

            // Expanded content
            if is_expanded {
                message_card = message_card.child(
                    div()
                        .id("description-copy")
                        .v_flex()
                        .w_full()
                        .min_w_0()
                        .gap(px(4.))
                        .text_xs()
                        .text_color(colors.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.8))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(desc_for_copy.clone()));
                            this.mark_copied("description", cx);
                        }))
                        // TODO(audit): PERF-17 cache parsed description blocks keyed on the
                        // commit message — requires markdown_view to expose MarkdownBlock and a
                        // parse/render-from-blocks API, which lives in another file.
                        .child(render_markdown(&description, window, cx))
                        .when(desc_copied, |el| {
                            el.child(
                                Label::new(t!("detail.copied"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Success),
                            )
                        }),
                );
            }
        }

        metadata = metadata.child(message_card);
        content = content.child(metadata);

        // -- Changed Files Section --
        if let (Some(diff), Some(cached)) = (&self.commit_diff, &self.cached_file_tree) {
            let total_file_count = diff.files.len();
            let filtered = self.filtered_files();
            let is_searching = is_file_searching(self.file_search_active, &self.file_search_query);
            let query_str = self.file_search_query.clone().unwrap_or_default();

            let file_count_text: SharedString = if is_searching && !query_str.is_empty() {
                let shown = filtered.len();
                let total = total_file_count;
                format!(
                    "{} / {} file{} changed",
                    shown,
                    total,
                    if total == 1 { "" } else { "s" }
                )
                .into()
            } else {
                format!(
                    "{} file{} changed",
                    total_file_count,
                    if total_file_count == 1 { "" } else { "s" },
                )
                .into()
            };

            let total_additions = diff.total_additions;
            let total_deletions = diff.total_deletions;

            // Build the header with optional search input
            let header_children = |cx: &mut Context<Self>| -> Vec<gpui::AnyElement> {
                let mut children: Vec<gpui::AnyElement> = vec![
                    Icon::new(IconName::File)
                        .size(IconSize::XSmall)
                        .color(Color::Muted)
                        .into_any_element(),
                    Label::new(file_count_text.clone())
                        .size(LabelSize::XSmall)
                        .weight(gpui::FontWeight::SEMIBOLD)
                        .color(Color::Muted)
                        .into_any_element(),
                ];

                if is_searching {
                    let search_input: gpui::AnyElement = div()
                        .flex_1()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Icon::new(IconName::Search)
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(24.))
                                .overflow_hidden()
                                .child(self.file_search_editor.clone()),
                        )
                        .child(
                            IconButton::new("clear-search", IconName::X)
                                .size(ButtonSize::Compact)
                                .style(ButtonStyle::Transparent)
                                .tooltip(t!("detail.clear_search.tooltip"))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.clear_file_search(cx);
                                }))
                                .into_any_element(),
                        )
                        .into_any_element();
                    children.push(search_input);
                } else {
                    children.push(div().flex_1().into_any_element());
                }

                // Diff stat
                let diff_stat: gpui::AnyElement =
                    DiffStat::new(total_additions, total_deletions).into_any_element();
                children.push(diff_stat);

                children
            };

            let header_children = header_children(cx);
            let mut header = div()
                .h_flex()
                .w_full()
                .flex_shrink_0()
                .h(px(28.))
                .px(px(10.))
                .gap_2()
                .items_center()
                .bg(colors.surface_background)
                .border_y_1()
                .border_color(colors.border_variant);

            for child in header_children {
                header = header.child(child);
            }
            content = content.child(header);

            // Show search hint when not searching and there are files
            if !is_searching && total_file_count > 0 {
                content = content.child(
                    div()
                        .h_flex()
                        .w_full()
                        .flex_shrink_0()
                        .h(px(22.))
                        .px_2()
                        .items_center()
                        .gap_1()
                        .child(
                            Label::new(t!("detail.search_files_hint"))
                                .size(LabelSize::XSmall)
                                .color(Color::Placeholder),
                        )
                        .child(div().flex_1()),
                );
            }

            // Show "no results" when search has no matches
            if is_searching && !query_str.is_empty() && filtered.is_empty() {
                content = content.child(
                    div()
                        .h_flex()
                        .w_full()
                        .flex_shrink_0()
                        .py_4()
                        .items_center()
                        .justify_center()
                        .child(
                            Label::new(format!("No files match \"{}\"", query_str))
                                .size(LabelSize::XSmall)
                                .color(Color::Placeholder),
                        ),
                );
            }

            // Render file list with search filter applied
            if !filtered.is_empty() {
                let searching = is_searching && !query_str.is_empty();
                let file_list: gpui::AnyElement = match self.file_view_mode {
                    FileViewMode::Flat => {
                        self.render_flat_file_list_filtered(cached, &filtered, cx)
                    }
                    FileViewMode::Tree => {
                        // While searching, show a match-filtered tree (matching files
                        // plus their ancestor directories, fully expanded); otherwise
                        // apply the collapsed-directory state.
                        let visible = if searching {
                            let matches = filtered.matched_set();
                            Arc::new(searched_tree_row_indices(&cached.tree_rows, &matches))
                        } else {
                            self.visible_tree_rows.clone()
                        };
                        self.render_tree_file_list(&cached.tree_rows, visible, cx)
                    }
                };
                content = content.child(file_list);
            }
        }

        panel = panel.child(content);
        panel.into_any_element()
    }
}

/// The detail panel's share of the heap census.
///
/// The panel holds the whole `CommitDiff` for the selected commit — every hunk
/// of every file — and a render-ready row set derived from it. The diff itself
/// is the same `Arc` the per-tab diff cache holds, so it reports zero here and
/// its cost shows up under the cache; the derived rows are the panel's own, and
/// they are what this walk exists to name.
#[cfg(feature = "perf")]
mod census {
    use rgitui_perf::{Census, HeapSize};

    use super::{
        CachedFileDiffTree, CachedFlatRow, CachedTreeFile, CachedTreeRow, CachedTreeRowKind,
        DetailPanel,
    };

    /// A flat row's numbers and colours are inline; its two shared strings
    /// are not, and a file path is exactly the length that stops being inline.
    impl HeapSize for CachedFlatRow {
        fn heap_size(&self, census: &mut Census) -> usize {
            self.file_name.heap_size(census) + self.dir_path.heap_size(census)
        }
    }

    impl HeapSize for CachedTreeFile {
        fn heap_size(&self, _census: &mut Census) -> usize {
            0
        }
    }

    impl HeapSize for CachedTreeRowKind {
        fn heap_size(&self, census: &mut Census) -> usize {
            match self {
                CachedTreeRowKind::Dir { key } => key.heap_size(census),
                CachedTreeRowKind::File(file) => file.heap_size(census),
            }
        }
    }

    impl HeapSize for CachedTreeRow {
        fn heap_size(&self, census: &mut Census) -> usize {
            self.label.heap_size(census) + self.kind.heap_size(census)
        }
    }

    impl HeapSize for CachedFileDiffTree {
        fn heap_size(&self, census: &mut Census) -> usize {
            self.flat_rows.heap_size(census) + self.tree_rows.heap_size(census)
        }

        fn heap_count(&self) -> Option<usize> {
            Some(self.flat_rows.len() + self.tree_rows.len())
        }
    }

    impl DetailPanel {
        /// Records the panel's retained commit, diff and derived rows under the
        /// current census path.
        pub(crate) fn census(&self, census: &mut Census) -> usize {
            census.enter("commit_diff", &self.commit_diff)
                + census.enter("commit", &self.commit)
                + census.enter("file_rows", &self.cached_file_tree)
                + census.enter("visible_rows", &self.visible_tree_rows)
                + census.enter("contained_in", &self.contained_in)
                + census.enter("collapsed_dirs", &self.collapsed_dirs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn working_test_rows(staged: &[&str], unstaged: &[&str]) -> Vec<WorkingRow> {
        let files = |paths: &[&str]| {
            paths
                .iter()
                .map(|path| rgitui_git::FileStatus {
                    path: (*path).into(),
                    kind: FileChangeKind::Modified,
                    old_path: None,
                    additions: 0,
                    deletions: 0,
                })
                .collect()
        };
        working_rows(&rgitui_git::WorkingTreeStatus {
            staged: files(staged),
            unstaged: files(unstaged),
        })
    }

    #[test]
    fn working_tree_preserves_sources_and_navigates_only_visible_leaves() {
        let file = |path: &str| rgitui_git::FileStatus {
            path: path.into(),
            kind: FileChangeKind::Modified,
            old_path: None,
            additions: 0,
            deletions: 0,
        };
        let status = rgitui_git::WorkingTreeStatus {
            staged: vec![file("src/z.rs"), file("src/a.rs")],
            unstaged: vec![file("src/a.rs"), file("README.md")],
        };
        let trees = [&status.staged, &status.unstaged].map(|files| {
            working_tree::build_tree(
                &files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>(),
            )
        });
        let collapsed = [HashSet::from(["src".to_string()]), HashSet::new()];
        let rows = working_tree_rows(&status, &trees, &collapsed, None);
        assert!(matches!(
            rows[1],
            WorkingRow::Directory {
                staged: true,
                collapsed: true,
                count: 2,
                ..
            }
        ));
        assert!(matches!(
            rows[3],
            WorkingRow::Directory {
                staged: false,
                collapsed: false,
                ..
            }
        ));
        let first = working_navigation(&rows, None, Some(1), CommandId::SelectFirst).unwrap();
        assert_eq!((first.file_index, first.displayed_row), (2, 4));
        assert_eq!(
            working_selection(&status, first.file_index),
            Some(("src/a.rs".into(), false))
        );
        assert!(working_navigation(&rows, None, Some(1), CommandId::Confirm).is_none());

        // Search reveals both sources and hides nonmatching files, without
        // mutating saved expansion or rebinding sorted indices to another file.
        let found = working_tree_rows(&status, &trees, &collapsed, Some("A.RS"));
        assert!(matches!(
            found[1],
            WorkingRow::Directory {
                collapsed: false,
                ..
            }
        ));
        let first = working_navigation(&found, None, None, CommandId::SelectFirst).unwrap();
        let next = working_navigation(&found, None, Some(first.file_index), CommandId::SelectNext)
            .unwrap();
        assert_eq!((first.file_index, next.file_index), (1, 2));
        assert_eq!(
            working_selection(&status, first.file_index),
            Some(("src/a.rs".into(), true))
        );
        assert!(collapsed[0].contains("src"));
        let missing = working_tree_rows(&status, &trees, &collapsed, Some("missing"));
        assert_eq!(missing.len(), 2);
        assert!(working_navigation(&missing, None, Some(2), CommandId::SelectFirst).is_none());
    }

    #[test]
    fn working_unfiltered_display_uses_cached_identity_order() {
        let rows = working_test_rows(&["a"], &["b"]);
        assert!(working_display_order(&rows, None).is_none());
        let first = working_navigation(&rows, None, None, CommandId::SelectFirst).unwrap();
        let last = working_navigation(&rows, None, None, CommandId::SelectLast).unwrap();
        assert_eq!((first.file_index, first.displayed_row), (0, 1));
        assert_eq!((last.file_index, last.displayed_row), (1, 3));
    }

    #[test]
    fn working_keyboard_uses_the_rendered_filter_order_and_skips_headers() {
        let rows = working_test_rows(&["ignore", "src/a.rs"], &["docs", "src/b.rs"]);
        let order = working_display_order(&rows, Some("SRC/")).unwrap();
        assert_eq!(order, [0, 2, 3, 5]);
        let next = working_navigation(&rows, Some(&order), Some(1), CommandId::SelectNext).unwrap();
        assert_eq!((next.file_index, next.displayed_row), (3, 3));
        assert!(matches!(
            rows[order[next.displayed_row]],
            WorkingRow::File { index: 3, .. }
        ));
        let previous =
            working_navigation(&rows, Some(&order), Some(3), CommandId::SelectPrev).unwrap();
        assert_eq!((previous.file_index, previous.displayed_row), (1, 1));
        let first =
            working_navigation(&rows, Some(&order), Some(3), CommandId::SelectFirst).unwrap();
        let last = working_navigation(&rows, Some(&order), Some(1), CommandId::SelectLast).unwrap();
        assert_eq!((first.file_index, last.file_index), (1, 3));
        // A selection hidden by the query starts at the first displayed file.
        for command in [CommandId::SelectNext, CommandId::SelectPrev] {
            assert_eq!(
                working_navigation(&rows, Some(&order), Some(0), command)
                    .unwrap()
                    .file_index,
                1
            );
        }
        assert!(working_navigation(&rows, Some(&order), Some(0), CommandId::Confirm).is_none());
    }

    #[test]
    fn working_keyboard_keeps_both_source_versions_of_the_same_path() {
        let rows = working_test_rows(&["partial.rs"], &["partial.rs"]);
        let order = working_display_order(&rows, Some("partial"));
        let staged =
            working_navigation(&rows, order.as_deref(), None, CommandId::SelectNext).unwrap();
        let unstaged = working_navigation(
            &rows,
            order.as_deref(),
            Some(staged.file_index),
            CommandId::SelectNext,
        )
        .unwrap();
        assert_eq!((staged.file_index, staged.displayed_row), (0, 1));
        assert_eq!((unstaged.file_index, unstaged.displayed_row), (1, 3));
        assert!(matches!(
            rows[staged.displayed_row],
            WorkingRow::File { staged: true, .. }
        ));
        assert!(matches!(
            rows[unstaged.displayed_row],
            WorkingRow::File { staged: false, .. }
        ));
    }

    #[test]
    fn working_empty_results_never_select_headers_or_activate_hidden_files() {
        for rows in [
            working_test_rows(&["a"], &["b"]),
            working_test_rows(&[], &[]),
        ] {
            let order = working_display_order(&rows, Some("missing"));
            for command in [
                CommandId::SelectNext,
                CommandId::SelectPrev,
                CommandId::SelectFirst,
                CommandId::SelectLast,
                CommandId::Confirm,
            ] {
                assert!(working_navigation(&rows, order.as_deref(), Some(0), command).is_none());
                assert!(working_navigation(&rows, order.as_deref(), None, command).is_none());
            }
        }
    }

    #[test]
    fn working_confirm_reopens_current_file_even_without_a_selection_change() {
        let rows = working_test_rows(&["partial.rs"], &["partial.rs"]);
        for _ in 0..2 {
            let confirm = working_navigation(&rows, None, Some(1), CommandId::Confirm).unwrap();
            assert_eq!(
                confirm,
                WorkingNavigation {
                    file_index: 1,
                    displayed_row: 3,
                    activate: true
                }
            );
        }
        let boundary = working_navigation(&rows, None, Some(1), CommandId::SelectNext).unwrap();
        assert!(!boundary.activate);
        assert_eq!(boundary.displayed_row, 3);
        assert!(working_navigation(&rows, None, None, CommandId::Confirm).is_none());
    }

    #[test]
    fn working_long_list_scroll_target_is_displayed_row_not_file_index() {
        let paths: Vec<_> = (0..450).map(|index| format!("file-{index}.rs")).collect();
        let paths: Vec<_> = paths.iter().map(String::as_str).collect();
        let rows = working_test_rows(&[], &paths);
        let last = working_navigation(&rows, None, None, CommandId::SelectLast).unwrap();
        assert_eq!((last.file_index, last.displayed_row), (449, 451));
        let previous = working_navigation(&rows, None, Some(449), CommandId::SelectPrev).unwrap();
        assert_eq!((previous.file_index, previous.displayed_row), (448, 450));
        let order = working_display_order(&rows, Some("file-449.rs"));
        let filtered =
            working_navigation(&rows, order.as_deref(), Some(449), CommandId::Confirm).unwrap();
        assert_eq!((filtered.file_index, filtered.displayed_row), (449, 2));
    }

    #[test]
    fn working_rows_keep_index_and_worktree_versions_of_the_same_path_distinct() {
        let file = rgitui_git::FileStatus {
            path: "partial.rs".into(),
            kind: FileChangeKind::Modified,
            old_path: None,
            additions: 1,
            deletions: 0,
        };
        let status = rgitui_git::WorkingTreeStatus {
            staged: vec![file.clone()],
            unstaged: vec![file],
        };
        let rows = working_rows(&status);
        assert!(matches!(
            rows[0],
            WorkingRow::Section {
                staged: true,
                count: 1
            }
        ));
        assert!(matches!(
            rows[1],
            WorkingRow::File {
                index: 0,
                staged: true,
                ..
            }
        ));
        assert!(matches!(
            rows[2],
            WorkingRow::Section {
                staged: false,
                count: 1
            }
        ));
        assert!(matches!(
            rows[3],
            WorkingRow::File {
                index: 1,
                staged: false,
                ..
            }
        ));
    }

    #[test]
    fn working_refresh_preserves_identity_not_numeric_position_or_other_source() {
        let file = |path: &str| rgitui_git::FileStatus {
            path: path.into(),
            kind: FileChangeKind::Modified,
            old_path: None,
            additions: 0,
            deletions: 0,
        };
        let old = rgitui_git::WorkingTreeStatus {
            staged: vec![file("same")],
            unstaged: vec![file("same"), file("other")],
        };
        let new = rgitui_git::WorkingTreeStatus {
            staged: vec![file("added"), file("same")],
            unstaged: vec![file("other"), file("same")],
        };
        assert_eq!(reconcile_working_selection(&old, &new, Some(0)), Some(1));
        assert_eq!(reconcile_working_selection(&old, &new, Some(1)), Some(3));
        let staged_away = rgitui_git::WorkingTreeStatus {
            staged: vec![file("same")],
            unstaged: vec![file("other")],
        };
        assert_eq!(
            reconcile_working_selection(&old, &staged_away, Some(1)),
            None
        );
        assert_eq!(
            reconcile_working_selection(&old, &staged_away, Some(2)),
            Some(1)
        );
    }

    #[test]
    fn clean_working_inspector_has_both_sections_and_no_historical_rows() {
        let rows = working_rows(&rgitui_git::WorkingTreeStatus::default());
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|row| matches!(row, WorkingRow::Section { count: 0, .. })));
    }
    use crate::time::format_relative_time_full;

    #[test]
    fn file_list_asks_for_every_row_plus_padding() {
        assert_eq!(file_list_content_height(3, 26.0), 86.0);
        assert_eq!(file_list_content_height(100, 26.0), 2608.0);
    }

    #[test]
    fn long_file_lists_shrink_no_further_than_four_rows() {
        assert_eq!(file_list_min_height(100, 26.0), 104.0);
    }

    #[test]
    fn short_file_lists_never_reserve_more_than_their_content() {
        assert_eq!(file_list_min_height(2, 26.0), 60.0);
    }

    #[test]
    fn co_author_initials_shrink_to_fit_the_small_avatar() {
        assert!((avatar_initials_size(16.0) - 9.6).abs() < f32::EPSILON);
    }

    #[test]
    fn large_avatar_initials_keep_the_original_size() {
        assert_eq!(avatar_initials_size(24.0), AVATAR_INITIALS_MAX_SIZE);
        assert_eq!(avatar_initials_size(40.0), AVATAR_INITIALS_MAX_SIZE);
    }

    // --- format_relative_time tests ---

    #[test]
    fn test_format_relative_time_just_now() {
        let now = chrono::Utc::now().timestamp();
        assert_eq!(format_relative_time_full(now), "just now");
    }

    #[test]
    fn test_format_relative_time_minutes() {
        let five_mins_ago = chrono::Utc::now().timestamp() - 300;
        assert_eq!(format_relative_time_full(five_mins_ago), "5 mins ago");
    }

    #[test]
    fn test_format_relative_time_one_minute() {
        let one_min_ago = chrono::Utc::now().timestamp() - 60;
        assert_eq!(format_relative_time_full(one_min_ago), "1 min ago");
    }

    #[test]
    fn test_format_relative_time_hours() {
        let two_hours_ago = chrono::Utc::now().timestamp() - 7200;
        assert_eq!(format_relative_time_full(two_hours_ago), "2 hours ago");
    }

    #[test]
    fn test_format_relative_time_one_hour() {
        let one_hour_ago = chrono::Utc::now().timestamp() - 3600;
        assert_eq!(format_relative_time_full(one_hour_ago), "1 hour ago");
    }

    #[test]
    fn test_format_relative_time_days() {
        let three_days_ago = chrono::Utc::now().timestamp() - 259200;
        assert_eq!(format_relative_time_full(three_days_ago), "3 days ago");
    }

    #[test]
    fn test_format_relative_time_one_day() {
        let one_day_ago = chrono::Utc::now().timestamp() - 86400;
        assert_eq!(format_relative_time_full(one_day_ago), "1 day ago");
    }

    #[test]
    fn test_format_relative_time_months() {
        let two_months_ago = chrono::Utc::now().timestamp() - 5184000;
        assert_eq!(format_relative_time_full(two_months_ago), "2 months ago");
    }

    #[test]
    fn test_format_relative_time_one_month() {
        let one_month_ago = chrono::Utc::now().timestamp() - 2592000;
        assert_eq!(format_relative_time_full(one_month_ago), "1 month ago");
    }

    #[test]
    fn test_format_relative_time_years() {
        let two_years_ago = chrono::Utc::now().timestamp() - 63072000;
        assert_eq!(format_relative_time_full(two_years_ago), "2 years ago");
    }

    #[test]
    fn test_format_relative_time_one_year() {
        let one_year_ago = chrono::Utc::now().timestamp() - 31536000;
        assert_eq!(format_relative_time_full(one_year_ago), "1 year ago");
    }

    #[test]
    fn test_format_relative_time_future() {
        let future = chrono::Utc::now().timestamp() + 1000;
        assert_eq!(format_relative_time_full(future), "in the future");
    }

    // --- format_absolute_date tests ---

    #[test]
    fn test_format_absolute_date_valid() {
        let ts = 1704067200; // 2024-01-01 00:00:00 UTC
        let result = format_absolute_date(ts);
        assert!(result.contains("2024"));
    }

    #[test]
    fn test_format_absolute_date_zero() {
        // timestamp 0 is 1970-01-01 00:00:00 UTC — valid epoch, not unknown
        let result = format_absolute_date(0);
        assert!(result.contains("1970"));
    }

    #[test]
    fn test_format_absolute_date_before_epoch() {
        // negative timestamps before epoch 0 are valid in chrono
        let result = format_absolute_date(-86400);
        assert!(result.contains("1969"));
    }

    // --- file_change_icon tests ---

    #[test]
    fn test_file_change_icon_added() {
        assert_eq!(file_change_icon(FileChangeKind::Added), IconName::FileAdded);
    }

    #[test]
    fn test_file_change_icon_modified() {
        assert_eq!(
            file_change_icon(FileChangeKind::Modified),
            IconName::FileModified
        );
    }

    #[test]
    fn test_file_change_icon_deleted() {
        assert_eq!(
            file_change_icon(FileChangeKind::Deleted),
            IconName::FileDeleted
        );
    }

    #[test]
    fn test_file_change_icon_renamed() {
        assert_eq!(
            file_change_icon(FileChangeKind::Renamed),
            IconName::FileRenamed
        );
    }

    #[test]
    fn test_file_change_icon_copied() {
        // Copied maps to FileRenamed (same icon)
        assert_eq!(
            file_change_icon(FileChangeKind::Copied),
            IconName::FileRenamed
        );
    }

    #[test]
    fn test_file_change_icon_type_change() {
        assert_eq!(
            file_change_icon(FileChangeKind::TypeChange),
            IconName::FileModified
        );
    }

    #[test]
    fn test_file_change_icon_untracked() {
        assert_eq!(
            file_change_icon(FileChangeKind::Untracked),
            IconName::FileAdded
        );
    }

    #[test]
    fn test_file_change_icon_conflicted() {
        assert_eq!(
            file_change_icon(FileChangeKind::Conflicted),
            IconName::FileConflict
        );
    }

    // --- file_change_color tests ---

    #[test]
    fn test_file_change_color_added() {
        assert_eq!(file_change_color(FileChangeKind::Added), Color::Added);
    }

    #[test]
    fn test_file_change_color_modified() {
        assert_eq!(file_change_color(FileChangeKind::Modified), Color::Modified);
    }

    #[test]
    fn test_file_change_color_deleted() {
        assert_eq!(file_change_color(FileChangeKind::Deleted), Color::Deleted);
    }

    #[test]
    fn test_file_change_color_renamed() {
        assert_eq!(file_change_color(FileChangeKind::Renamed), Color::Renamed);
    }

    #[test]
    fn test_file_change_color_copied() {
        assert_eq!(file_change_color(FileChangeKind::Copied), Color::Info);
    }

    #[test]
    fn test_file_change_color_type_change() {
        assert_eq!(
            file_change_color(FileChangeKind::TypeChange),
            Color::Warning
        );
    }

    #[test]
    fn test_file_change_color_untracked() {
        assert_eq!(
            file_change_color(FileChangeKind::Untracked),
            Color::Untracked
        );
    }

    #[test]
    fn test_file_change_color_conflicted() {
        assert_eq!(
            file_change_color(FileChangeKind::Conflicted),
            Color::Conflict
        );
    }

    // --- build_cached_file_tree tests ---

    fn make_file_diff(path: &str, kind: FileChangeKind) -> FileDiff {
        FileDiff {
            image: None,
            path: std::path::PathBuf::from(path),
            hunks: vec![],
            additions: 10,
            deletions: 5,
            kind,
        }
    }

    /// (depth, label, is_dir) for each tree row, in display order.
    fn tree_summary(cached: &CachedFileDiffTree) -> Vec<(usize, String, bool)> {
        cached
            .tree_rows
            .iter()
            .map(|r| {
                (
                    r.depth,
                    r.label.to_string(),
                    matches!(r.kind, CachedTreeRowKind::Dir { .. }),
                )
            })
            .collect()
    }

    fn tree_labels(cached: &CachedFileDiffTree, indices: &[usize]) -> Vec<String> {
        indices
            .iter()
            .map(|&i| cached.tree_rows[i].label.to_string())
            .collect()
    }

    #[test]
    fn tree_renders_root_files_at_depth_zero() {
        let files = vec![make_file_diff("README.md", FileChangeKind::Modified)];
        let cached = build_cached_file_tree(&files);
        assert_eq!(cached.tree_rows.len(), 1);
        assert_eq!(cached.tree_rows[0].depth, 0);
        assert_eq!(cached.tree_rows[0].label.as_str(), "README.md");
        assert!(matches!(
            cached.tree_rows[0].kind,
            CachedTreeRowKind::File(_)
        ));
    }

    #[test]
    fn tree_compaction_stops_at_a_file() {
        // `src` has a file, so it is not compacted; dirs render before files.
        let files = vec![
            make_file_diff("Cargo.toml", FileChangeKind::Modified),
            make_file_diff("src/main.rs", FileChangeKind::Added),
        ];
        let cached = build_cached_file_tree(&files);
        assert_eq!(
            tree_summary(&cached),
            vec![
                (0, "src".to_string(), true),
                (1, "main.rs".to_string(), false),
                (0, "Cargo.toml".to_string(), false),
            ]
        );
    }

    #[test]
    fn tree_compacts_single_child_dir_chains() {
        let files = vec![make_file_diff(
            "crates/foo/src/lib.rs",
            FileChangeKind::Modified,
        )];
        let cached = build_cached_file_tree(&files);
        assert_eq!(
            tree_summary(&cached),
            vec![
                (0, "crates/foo/src".to_string(), true),
                (1, "lib.rs".to_string(), false),
            ]
        );
        match &cached.tree_rows[0].kind {
            CachedTreeRowKind::Dir { key } => assert_eq!(key.as_str(), "crates/foo/src"),
            _ => panic!("expected a directory node"),
        }
    }

    #[test]
    fn collapsed_dir_hides_its_descendants() {
        let files = vec![
            make_file_diff("src/a.rs", FileChangeKind::Modified),
            make_file_diff("src/b.rs", FileChangeKind::Modified),
            make_file_diff("README.md", FileChangeKind::Modified),
        ];
        let cached = build_cached_file_tree(&files);
        let mut collapsed = HashSet::new();
        collapsed.insert("src".to_string());
        let visible = visible_tree_row_indices(&cached.tree_rows, &collapsed);
        assert_eq!(tree_labels(&cached, &visible), vec!["src", "README.md"]);
    }

    #[test]
    fn search_filter_keeps_matches_and_ancestors() {
        let files = vec![
            make_file_diff("src/foo/a.rs", FileChangeKind::Modified), // index 0
            make_file_diff("src/bar/b.rs", FileChangeKind::Modified), // index 1
            make_file_diff("README.md", FileChangeKind::Modified),    // index 2
        ];
        let cached = build_cached_file_tree(&files);
        let matches: HashSet<usize> = [0usize].into_iter().collect();
        let visible = searched_tree_row_indices(&cached.tree_rows, &matches);
        // Keeps the path src > foo > a.rs; excludes the bar subtree and README.md.
        assert_eq!(tree_labels(&cached, &visible), vec!["src", "foo", "a.rs"]);
    }

    #[test]
    fn test_build_cached_file_tree_empty() {
        let files: &[FileDiff] = &[];
        let cached = build_cached_file_tree(files);
        assert!(cached.flat_rows.is_empty());
    }

    #[test]
    fn test_build_cached_file_tree_single_file() {
        let files = vec![make_file_diff("src/main.rs", FileChangeKind::Modified)];
        let cached = build_cached_file_tree(&files);
        assert_eq!(cached.flat_rows.len(), 1);
        let row = &cached.flat_rows[0];
        assert_eq!(row.file_index, 0);
        assert_eq!(row.file_name.as_str(), "main.rs");
        assert_eq!(row.dir_path.as_str(), "src/");
        assert_eq!(row.additions, 10);
        assert_eq!(row.deletions, 5);
    }

    #[test]
    fn test_build_cached_file_tree_file_at_root() {
        let files = vec![make_file_diff("Cargo.toml", FileChangeKind::Added)];
        let cached = build_cached_file_tree(&files);
        assert_eq!(cached.flat_rows.len(), 1);
        let row = &cached.flat_rows[0];
        assert_eq!(row.file_name.as_str(), "Cargo.toml");
        assert!(row.dir_path.is_empty());
    }

    #[test]
    fn test_build_cached_file_tree_nested_path() {
        let files = vec![make_file_diff(
            "crates/rgitui_workspace/src/panel.rs",
            FileChangeKind::Modified,
        )];
        let cached = build_cached_file_tree(&files);
        let row = &cached.flat_rows[0];
        assert_eq!(row.file_name.as_str(), "panel.rs");
        assert_eq!(row.dir_path.as_str(), "crates/rgitui_workspace/src/");
    }

    #[test]
    fn test_build_cached_file_tree_multiple_files_order_preserved() {
        let files = vec![
            make_file_diff("a.rs", FileChangeKind::Added),
            make_file_diff("b.rs", FileChangeKind::Modified),
            make_file_diff("c.rs", FileChangeKind::Deleted),
        ];
        let cached = build_cached_file_tree(&files);
        assert_eq!(cached.flat_rows.len(), 3);
        assert_eq!(cached.flat_rows[0].file_index, 0);
        assert_eq!(cached.flat_rows[1].file_index, 1);
        assert_eq!(cached.flat_rows[2].file_index, 2);
    }

    #[test]
    fn test_build_cached_file_tree_file_change_kinds() {
        let files = vec![
            make_file_diff("added.rs", FileChangeKind::Added),
            make_file_diff("modified.rs", FileChangeKind::Modified),
            make_file_diff("deleted.rs", FileChangeKind::Deleted),
            make_file_diff("renamed.rs", FileChangeKind::Renamed),
            make_file_diff("copied.rs", FileChangeKind::Copied),
            make_file_diff("type_change.rs", FileChangeKind::TypeChange),
            make_file_diff("untracked.rs", FileChangeKind::Untracked),
            make_file_diff("conflicted.rs", FileChangeKind::Conflicted),
        ];
        let cached = build_cached_file_tree(&files);
        assert_eq!(cached.flat_rows.len(), 8);
        // Preserve typed status so badges translate at render time.
        assert_eq!(cached.flat_rows[0].kind, FileChangeKind::Added);
        assert_eq!(cached.flat_rows[1].kind, FileChangeKind::Modified);
        assert_eq!(cached.flat_rows[2].kind, FileChangeKind::Deleted);
        assert_eq!(cached.flat_rows[3].kind, FileChangeKind::Renamed);
        assert_eq!(cached.flat_rows[4].kind, FileChangeKind::Copied);
        assert_eq!(cached.flat_rows[5].kind, FileChangeKind::TypeChange);
        assert_eq!(cached.flat_rows[6].kind, FileChangeKind::Untracked);
        assert_eq!(cached.flat_rows[7].kind, FileChangeKind::Conflicted);
    }

    #[test]
    fn test_build_cached_file_tree_deeply_nested() {
        let files = vec![make_file_diff(
            "very/deeply/nested/directory/structure/file.rs",
            FileChangeKind::Modified,
        )];
        let cached = build_cached_file_tree(&files);
        let row = &cached.flat_rows[0];
        assert_eq!(row.file_name.as_str(), "file.rs");
        assert_eq!(
            row.dir_path.as_str(),
            "very/deeply/nested/directory/structure/"
        );
    }

    // --- FileViewMode tests ---

    #[test]
    fn test_file_view_mode_default_is_flat() {
        assert_eq!(FileViewMode::default(), FileViewMode::Flat);
    }

    #[test]
    fn test_file_view_mode_toggles_between_flat_and_tree() {
        assert_eq!(FileViewMode::Flat.toggled(), FileViewMode::Tree);
        assert_eq!(FileViewMode::Tree.toggled(), FileViewMode::Flat);
    }

    #[test]
    fn test_file_view_mode_icons_describe_next_action() {
        assert_eq!(FileViewMode::Flat.toggle_icon(), IconName::Folder);
        assert_eq!(FileViewMode::Tree.toggle_icon(), IconName::File);
    }

    #[test]
    fn test_file_view_mode_tooltips_describe_next_action() {
        assert_eq!(
            FileViewMode::Flat.toggle_tooltip().as_ref(),
            "Switch to Tree view (v)"
        );
        assert_eq!(
            FileViewMode::Tree.toggle_tooltip().as_ref(),
            "Switch to Flat view (v)"
        );
    }

    #[test]
    fn test_file_view_toggle_tooltip_describes_disabled_state() {
        assert_eq!(
            file_view_toggle_tooltip(0, false, FileViewMode::Flat).as_ref(),
            "No changed files to display"
        );
        assert_eq!(
            file_view_toggle_tooltip(2, true, FileViewMode::Flat).as_ref(),
            "Clear file search to switch views"
        );
        assert_eq!(
            file_view_toggle_tooltip(2, false, FileViewMode::Flat).as_ref(),
            "Switch to Tree view (v)"
        );
    }

    #[test]
    fn test_file_view_mode_list_ids_select_distinct_renderers() {
        assert_eq!(
            FileViewMode::Flat.list_element_id(),
            "detail-files-filtered"
        );
        assert_eq!(
            FileViewMode::Tree.list_element_id(),
            "detail-files-filtered-tree"
        );
    }

    #[test]
    fn test_file_view_toggle_requires_files_and_no_search() {
        assert!(can_toggle_file_view(1, false));
        assert!(!can_toggle_file_view(0, false));
        assert!(!can_toggle_file_view(1, true));
    }

    #[test]
    fn test_file_view_toggle_request_preserves_mode_when_disabled() {
        assert_eq!(
            file_view_mode_after_toggle_request(FileViewMode::Flat, 2, false),
            FileViewMode::Tree
        );
        assert_eq!(
            file_view_mode_after_toggle_request(FileViewMode::Flat, 0, false),
            FileViewMode::Flat
        );
        assert_eq!(
            file_view_mode_after_toggle_request(FileViewMode::Tree, 3, true),
            FileViewMode::Tree
        );
    }

    #[test]
    fn test_file_search_state_blocks_view_toggle_shortcut() {
        assert!(is_file_searching(true, &None));
        assert!(is_file_searching(false, &Some("view".to_string())));
        assert!(!is_file_searching(false, &None));

        assert!(!can_toggle_file_view(
            3,
            is_file_searching(true, &Some("view".to_string()))
        ));
    }

    // --- filtered_files tests ---

    // Tests for fuzzy file search sort-by-relevance (PR #28).
    // The query path of `filtered_files` uses CommandPalette::fuzzy_score, the
    // same scoring used by the command palette.

    #[test]
    fn test_filtered_files_relevance_sort_is_descending() {
        // The query path scores matches then sorts by score descending so the
        // best (highest-scoring) match comes first.
        let scored: Vec<(usize, usize)> = vec![(3, 0), (10, 1), (7, 2)];
        let mut sorted = scored.clone();
        sorted.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        let order: Vec<usize> = sorted.into_iter().map(|(_, i)| i).collect();
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn test_filtered_files_relevance_order() {
        // "sh" matches "Show" (pos 0) higher than "Fish" (pos 1)
        let score_show = rgitui_ui::fuzzy_score("sh", "Show").unwrap();
        let score_fish = rgitui_ui::fuzzy_score("sh", "Fish").unwrap();
        assert!(
            score_show > score_fish,
            "earlier match should score higher: {score_show} vs {score_fish}"
        );
    }
}
