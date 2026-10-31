//! Pure working-change tree construction and visibility filtering.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::path::{Component, PathBuf};

use rgitui_git::FileChangeKind;

/// Sections with more files than this open with their top-level folders
/// collapsed, so the folder summaries fit on screen before any scrolling.
pub(super) const AUTO_COLLAPSE_FILE_COUNT: usize = 24;

/// What a folder's descendants changed, summarized the way a collapsed folder
/// shows it: edits, additions, removals and conflicts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ChangeTally {
    /// Modified, renamed, copied and type-changed files.
    pub modified: usize,
    /// Added and untracked files.
    pub added: usize,
    pub deleted: usize,
    pub conflicted: usize,
}

impl ChangeTally {
    fn add(&mut self, kind: FileChangeKind) {
        match kind {
            FileChangeKind::Modified
            | FileChangeKind::Renamed
            | FileChangeKind::Copied
            | FileChangeKind::TypeChange => self.modified += 1,
            FileChangeKind::Added | FileChangeKind::Untracked => self.added += 1,
            FileChangeKind::Deleted => self.deleted += 1,
            FileChangeKind::Conflicted => self.conflicted += 1,
        }
    }
}

/// One tally per row of `rows` (from [`build_tree`] over the same files),
/// counting every file below a directory whether or not it is collapsed.
/// File rows get an empty tally. `kinds` is indexed like the tree's input paths.
pub(super) fn directory_tallies(rows: &[TreeRow], kinds: &[FileChangeKind]) -> Vec<ChangeTally> {
    let mut tallies = vec![ChangeTally::default(); rows.len()];
    // Rows are depth-first, so the open directories are a stack.
    let mut open: Vec<(usize, usize)> = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        while open.last().is_some_and(|&(_, depth)| depth >= row.depth()) {
            open.pop();
        }
        match row {
            TreeRow::Directory { depth, .. } => open.push((row_index, *depth)),
            TreeRow::File { index, .. } => {
                if let Some(&kind) = kinds.get(*index) {
                    for &(directory, _) in &open {
                        tallies[directory].add(kind);
                    }
                }
            }
        }
    }
    tallies
}

/// The folders a section opens with collapsed: its top-level folders once it
/// has more than [`AUTO_COLLAPSE_FILE_COUNT`] files, otherwise none.
pub(super) fn default_collapsed(rows: &[TreeRow]) -> HashSet<String> {
    let files = rows
        .iter()
        .filter(|row| matches!(row, TreeRow::File { .. }))
        .count();
    if files <= AUTO_COLLAPSE_FILE_COUNT {
        return HashSet::new();
    }
    rows.iter()
        .filter_map(|row| match row {
            TreeRow::Directory { path, depth: 0, .. } => Some(path.clone()),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TreeRow {
    Directory {
        /// Full repository-relative key, using `/` separators even on Windows.
        path: String,
        /// Display name, including any compacted single-child directory chain.
        name: String,
        depth: usize,
        /// Number of descendant files, independent of visibility.
        count: usize,
    },
    File {
        /// Index into the original input paths, not the sorted tree.
        index: usize,
        depth: usize,
    },
}

impl TreeRow {
    fn depth(&self) -> usize {
        match self {
            Self::Directory { depth, .. } | Self::File { depth, .. } => *depth,
        }
    }
}

#[derive(Default)]
struct TreeBuilder {
    dirs: BTreeMap<OsString, TreeBuilder>,
    files: Vec<usize>,
    count: usize,
}

/// Build depth-first rows from repository-relative file paths. At each level,
/// directories precede files and both groups are alphabetical. A directory with
/// no direct files and exactly one child directory is compacted with that child.
/// The virtual repository root is never emitted. Paths without a filename are
/// ignored; all other file indices refer to their original input positions.
pub(super) fn build_tree(paths: &[PathBuf]) -> Vec<TreeRow> {
    let mut root = TreeBuilder::default();
    for (index, path) in paths.iter().enumerate() {
        if path.file_name().is_none() {
            continue;
        }
        let mut node = &mut root;
        node.count += 1;
        if let Some(parent) = path.parent() {
            for component in parent.components() {
                if component == Component::CurDir {
                    continue;
                }
                node = node
                    .dirs
                    .entry(component.as_os_str().to_owned())
                    .or_default();
                node.count += 1;
            }
        }
        node.files.push(index);
    }

    let mut rows = Vec::new();
    flatten_tree(&root, "", 0, paths, &mut rows);
    rows
}

fn flatten_tree(
    builder: &TreeBuilder,
    prefix: &str,
    depth: usize,
    paths: &[PathBuf],
    rows: &mut Vec<TreeRow>,
) {
    for (name, child) in &builder.dirs {
        let mut name = name.to_string_lossy().into_owned();
        let mut path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let mut node = child;
        while node.files.is_empty() && node.dirs.len() == 1 {
            let (child_name, child) = node.dirs.iter().next().unwrap();
            let child_name = child_name.to_string_lossy();
            name.push('/');
            name.push_str(&child_name);
            path.push('/');
            path.push_str(&child_name);
            node = child;
        }
        rows.push(TreeRow::Directory {
            path: path.clone(),
            name,
            depth,
            count: node.count,
        });
        flatten_tree(node, &path, depth + 1, paths, rows);
    }

    let mut files = builder.files.clone();
    files.sort_by(|&a, &b| paths[a].file_name().cmp(&paths[b].file_name()));
    rows.extend(
        files
            .into_iter()
            .map(|index| TreeRow::File { index, depth }),
    );
}

/// Preserve collapsed folders when a status refresh compacts a different chain.
/// A saved `src` key may now be represented by `src/sub`; transfer it to that
/// row rather than silently expanding the remaining files. Consume each old key
/// once so descendants do not acquire extra collapses when their parent survives.
/// Keys with no surviving directory are dropped.
pub(super) fn reconcile_collapsed(
    rows: &[TreeRow],
    collapsed: &HashSet<String>,
) -> HashSet<String> {
    let mut pending = collapsed.clone();
    let mut reconciled = HashSet::new();
    for row in rows {
        let TreeRow::Directory { path, .. } = row else {
            continue;
        };
        let mut prefix = path.as_str();
        let mut keep = false;
        loop {
            keep |= pending.remove(prefix);
            let Some((parent, _)) = prefix.rsplit_once('/') else {
                break;
            };
            prefix = parent;
        }
        if keep {
            reconciled.insert(path.clone());
        }
    }
    reconciled
}

/// Return indices into `rows` in display order. Collapsed directories remain
/// visible but hide their descendants. With `Some(matching_files)`, collapse is
/// ignored and only matching original file indices and their ancestors remain.
/// An empty search match set therefore yields no rows.
pub(super) fn visible_rows(
    rows: &[TreeRow],
    collapsed: &HashSet<String>,
    matching_files: Option<&HashSet<usize>>,
) -> Vec<usize> {
    if let Some(matches) = matching_files {
        let mut keep = vec![false; rows.len()];
        let mut ancestors: Vec<usize> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            while let Some(&ancestor) = ancestors.last() {
                if rows[ancestor].depth() < row.depth() {
                    break;
                }
                ancestors.pop();
            }
            match row {
                TreeRow::Directory { .. } => ancestors.push(index),
                TreeRow::File { index: file, .. } => {
                    if matches.contains(file) {
                        keep[index] = true;
                        for &ancestor in &ancestors {
                            keep[ancestor] = true;
                        }
                    }
                }
            }
        }
        return (0..rows.len()).filter(|&index| keep[index]).collect();
    }

    let mut visible = Vec::new();
    let mut hide_below = None;
    for (index, row) in rows.iter().enumerate() {
        if let Some(depth) = hide_below {
            if row.depth() > depth {
                continue;
            }
            hide_below = None;
        }
        visible.push(index);
        if let TreeRow::Directory { path, depth, .. } = row {
            if collapsed.contains(path) {
                hide_below = Some(*depth);
            }
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(paths: &[&str]) -> Vec<TreeRow> {
        build_tree(&paths.iter().map(PathBuf::from).collect::<Vec<_>>())
    }

    fn directory(path: &str, name: &str, depth: usize, count: usize) -> TreeRow {
        TreeRow::Directory {
            path: path.into(),
            name: name.into(),
            depth,
            count,
        }
    }

    fn file(index: usize, depth: usize) -> TreeRow {
        TreeRow::File { index, depth }
    }

    fn nested_tree() -> Vec<TreeRow> {
        tree(&[
            "src/z.rs",
            "README.md",
            "src/nested/b.rs",
            "docs/guide.md",
            "src/a.rs",
            "src/nested/a.rs",
            "Cargo.toml",
        ])
    }

    #[test]
    fn nested_tree_sorts_directories_first_and_preserves_original_indices() {
        assert_eq!(
            nested_tree(),
            vec![
                directory("docs", "docs", 0, 1),
                file(3, 1),
                directory("src", "src", 0, 4),
                directory("src/nested", "nested", 1, 2),
                file(5, 2),
                file(2, 2),
                file(4, 1),
                file(0, 1),
                file(6, 0),
                file(1, 0),
            ]
        );
    }

    #[test]
    fn compacts_chains_but_stops_at_branches_and_direct_files() {
        assert_eq!(
            tree(&[
                "crates/foo/src/lib.rs",
                "crates/foo/src/deep/module/file.rs",
                "crates/bar/src/lib.rs",
            ]),
            vec![
                directory("crates", "crates", 0, 3),
                directory("crates/bar/src", "bar/src", 1, 1),
                file(2, 2),
                directory("crates/foo/src", "foo/src", 1, 2),
                directory("crates/foo/src/deep/module", "deep/module", 2, 1),
                file(1, 3),
                file(0, 2),
            ]
        );
        assert_eq!(
            tree(&["one/two/three/file.rs"]),
            vec![
                directory("one/two/three", "one/two/three", 0, 1),
                file(0, 1)
            ]
        );
    }

    #[test]
    fn root_files_are_sorted_stably_without_a_root_directory() {
        assert_eq!(
            tree(&["z.txt", "a.txt", "a.txt"]),
            vec![file(1, 0), file(2, 0), file(0, 0)]
        );
        assert_eq!(tree(&["README.md"]), vec![file(0, 0)]);
    }

    #[test]
    fn empty_input_and_empty_search_show_nothing() {
        assert!(build_tree(&[]).is_empty());
        assert!(visible_rows(&[], &HashSet::new(), None).is_empty());
        assert!(visible_rows(&nested_tree(), &HashSet::new(), Some(&HashSet::new())).is_empty());
        assert_eq!(
            visible_rows(&nested_tree(), &HashSet::new(), None),
            (0..10).collect::<Vec<_>>()
        );
    }

    #[test]
    fn nested_collapses_hide_descendants_not_siblings() {
        let rows = nested_tree();
        let mut collapsed = HashSet::from(["src/nested".into()]);
        assert_eq!(
            visible_rows(&rows, &collapsed, None),
            vec![0, 1, 2, 3, 6, 7, 8, 9]
        );
        collapsed.insert("src".into());
        assert_eq!(visible_rows(&rows, &collapsed, None), vec![0, 1, 2, 8, 9]);
        collapsed.remove("src");
        assert_eq!(
            visible_rows(&rows, &collapsed, None),
            vec![0, 1, 2, 3, 6, 7, 8, 9]
        );
    }

    #[test]
    fn compact_directory_uses_its_full_key_for_collapse_and_search() {
        let rows = tree(&["one/two/file.rs", "root.txt"]);
        let collapsed = HashSet::from(["one/two".into()]);
        assert_eq!(visible_rows(&rows, &collapsed, None), vec![0, 2]);
        assert_eq!(
            visible_rows(&rows, &collapsed, Some(&HashSet::from([0]))),
            vec![0, 1]
        );
    }

    #[test]
    fn search_reveals_matches_and_only_their_ancestors() {
        let rows = nested_tree();
        let collapsed = HashSet::from(["src".into(), "src/nested".into(), "docs".into()]);
        assert_eq!(
            visible_rows(&rows, &collapsed, Some(&HashSet::from([2, 5, 6]))),
            vec![2, 3, 4, 5, 8]
        );
        assert_eq!(
            visible_rows(&rows, &collapsed, Some(&HashSet::from([1]))),
            vec![9]
        );
        assert!(visible_rows(&rows, &collapsed, Some(&HashSet::from([99]))).is_empty());
    }

    #[test]
    fn refresh_preserves_collapses_when_directory_chains_change() {
        let collapsed = HashSet::from(["src".into()]);
        let compacted = tree(&["src/sub/b.rs"]);
        let reconciled = reconcile_collapsed(&compacted, &collapsed);
        assert_eq!(reconciled, HashSet::from(["src/sub".into()]));
        assert_eq!(visible_rows(&compacted, &reconciled, None), vec![0]);

        // Adding a direct file splits the chain again. The new sibling is
        // visible, while the previously collapsed subtree stays collapsed.
        let split = tree(&["src/a.rs", "src/sub/b.rs"]);
        assert_eq!(reconcile_collapsed(&split, &reconciled), reconciled);
        // Keeping the original parent must not collapse all its children too.
        assert_eq!(reconcile_collapsed(&split, &collapsed), collapsed);
        let nested = HashSet::from(["src".into(), "src/sub".into()]);
        assert_eq!(reconcile_collapsed(&split, &nested), nested);
        assert_eq!(reconcile_collapsed(&compacted, &nested), reconciled);
        assert!(reconcile_collapsed(&tree(&["other/a.rs"]), &nested).is_empty());
    }

    #[test]
    fn tallies_count_every_descendant_by_kind_even_when_collapsed() {
        use FileChangeKind::*;
        // Indices: 0 src/z.rs, 1 README.md, 2 src/nested/b.rs, 3 docs/guide.md,
        // 4 src/a.rs, 5 src/nested/a.rs, 6 Cargo.toml.
        let rows = nested_tree();
        let kinds = [
            Modified, Untracked, Deleted, Added, Renamed, Conflicted, Modified,
        ];
        let tallies = directory_tallies(&rows, &kinds);
        assert_eq!(tallies.len(), rows.len());
        let tally = |modified, added, deleted, conflicted| ChangeTally {
            modified,
            added,
            deleted,
            conflicted,
        };
        // docs
        assert_eq!(tallies[0], tally(0, 1, 0, 0));
        // src: z.rs, nested/b.rs, a.rs, nested/a.rs
        assert_eq!(tallies[2], tally(2, 0, 1, 1));
        // src/nested
        assert_eq!(tallies[3], tally(0, 0, 1, 1));
        // File rows carry nothing; root files belong to no directory.
        assert_eq!(tallies[1], ChangeTally::default());
        assert_eq!(tallies[9], ChangeTally::default());
    }

    #[test]
    fn tallies_follow_compacted_directory_chains() {
        use FileChangeKind::*;
        let rows = tree(&[
            "crates/foo/src/lib.rs",
            "crates/foo/src/deep/module/file.rs",
            "crates/bar/src/lib.rs",
        ]);
        let tallies = directory_tallies(&rows, &[Modified, Added, Deleted]);
        assert_eq!(
            tallies[0].modified + tallies[0].added + tallies[0].deleted,
            3
        );
        // crates/bar/src holds only the deleted file.
        assert_eq!(tallies[1].deleted, 1);
        assert_eq!(tallies[1].modified + tallies[1].added, 0);
        // crates/foo/src holds the other two.
        assert_eq!((tallies[3].modified, tallies[3].added), (1, 1));
    }

    #[test]
    fn large_sections_open_with_top_level_folders_collapsed() {
        let small = nested_tree();
        assert!(default_collapsed(&small).is_empty());

        let paths: Vec<String> = (0..=AUTO_COLLAPSE_FILE_COUNT)
            .map(|i| match i % 3 {
                0 => format!("src/nested/f{i}.rs"),
                1 => format!("docs/f{i}.md"),
                _ => format!("root{i}.txt"),
            })
            .collect();
        let rows = build_tree(&paths.iter().map(PathBuf::from).collect::<Vec<_>>());
        let collapsed = default_collapsed(&rows);
        assert_eq!(
            collapsed,
            HashSet::from(["docs".to_string(), "src/nested".to_string()])
        );
        // Root files stay visible beside the collapsed folders.
        let visible = visible_rows(&rows, &collapsed, None);
        assert!(visible
            .iter()
            .any(|&row| matches!(rows[row], TreeRow::File { depth: 0, .. })));
        assert!(!visible
            .iter()
            .any(|&row| matches!(rows[row], TreeRow::File { depth: 1, .. })));
    }

    #[test]
    fn native_path_components_produce_slash_separated_keys() {
        let path = PathBuf::from(".")
            .join("crates")
            .join("example")
            .join("src")
            .join("lib.rs");
        assert_eq!(
            build_tree(&[path]),
            vec![
                directory("crates/example/src", "crates/example/src", 0, 1),
                file(0, 1),
            ]
        );
    }
}
