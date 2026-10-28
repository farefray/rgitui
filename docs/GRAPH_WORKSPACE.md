# Graph-first workspace

## Intent

Make repository history read as branches and ancestry rather than a table of
hashes, and give file review the same primary space as the graph. This is inspired
by GitKraken's hierarchy, not a pixel-for-pixel recreation.

The repository tab strip remains the top-level navigation. Graph and file/tool
views belong to the active repository; opening a file never creates another
repository tab.

## Graph

- Branch/tag references precede the graph, aligned with their commit.
- Reference accents and connectors identify their ancestry lane. Lane colors do
  not mean author identity or exclusive branch ownership of commits.
- Commit subjects have a consistent start instead of following inline badges.
- Hashes are optional secondary information, available in commit details and
  copy actions even when their graph column is hidden.
- Selection, checked-out HEAD and uncommitted changes have distinct indicators.
- Existing commit topology, virtualization and Git commands remain in place.
  Virtual pending-change placement also checks visible parent relationships, so
  WIP stays a sibling of a real feature child when lanes converge at a merge.

## Worktrees and safe navigation

- A checkout gets its own graph row only when the row says something a badge
  cannot: it has uncommitted changes, its HEAD is a confirmed unborn branch, or
  it is the linked worktree being viewed. A clean worktree is shown instead by a
  folder icon on its branch badge; **Display Settings → Show clean worktrees**
  gives every checkout a row again. Unknown status is not presented as "clean".
  Missing HEAD metadata is not called a "new branch": unborn HEAD is confirmed
  by Git, separately from unavailable checkouts.
- Worktree rows are labelled with a dashed chip naming the checkout, matching the
  dashed pending-changes node.
- A ref badge folds an attached HEAD and same-named remote branches into their
  local branch, so `+N` counts only the other refs. Its tooltip and click-open
  list name each ref with what else is true of it (HEAD, its remotes, the
  worktree that has it checked out); a worktree entry in that list opens it.
- Click or double-click a worktree row to view that existing checkout. This does
  not run Git checkout, move HEAD, or discard any checkout's changes. Subsequent
  Git actions target the viewed checkout.
- Removing a worktree uses `core.longpaths` on Windows. If Git unregisters a
  worktree but cannot delete all its files, rgitui finishes deleting the folder
  rather than leaving an unlisted one behind, and refreshes the list either way.
  A folder that is not a registered worktree is never deleted.
- A failed operation is reported once, in the failure banner, which can expand to
  show Git's full output.
- Ordinary commit double-click now only inspects the commit. Checking out a commit
  remains an explicit context-menu action, not a navigation gesture.
- The sidebar reveals its Worktrees section when linked checkouts first appear,
  shows branch names with full-path/safe-entry tooltips, marks checkouts with
  uncommitted changes, and marks the checkout actually being viewed rather than
  always marking the originally opened one.
- A blue informational worktree banner identifies the active context. **Back to
  repository** returns to the original checkout without removing the worktree or
  changing its files. Per-checkout commit drafts remain intact.

## Main area

The graph-focused layout uses the full center height for one view at a time:

1. Select a commit to inspect its metadata and files on the right. This must not
   automatically replace the graph with a diff.
2. Select a file to open the reusable file view in the center. Selecting another
   file updates that view rather than adding another file tab.
3. Use Graph to return without recreating its entity or losing its scroll and
   selection. The file view remains available in its tab.
4. Close the file view to return to Graph.

History, blame and the existing tool views must remain reachable. A file's source
(commit, staged or unstaged) belongs to its diff context, not whichever graph row
happens to be selected later. Closing a view or returning to Graph must not be
undone by a late background result.

The visible **Split view** control restores the classic graph-above-file layout;
**Graph focus** returns to the new layout. The layout choice is saved. Classic
split sizes are retained rather than overwritten by full-height views.

## Diff readability

- Syntax-rendered text now explicitly uses a verified monospace face at the
  user's font size, instead of inheriting proportional UI text from `StyledText`.
  Code line height, gutter width, wrapping and horizontal extent share those
  measured font metrics. Interface labels remain proportional.
- Added/deleted code rows use quieter tints; gutters and word highlights retain
  stronger change cues. Comments are upright, hunk headers calmer, and file
  headers emphasize the filename with secondary directory context.
- Syntax, selection and word-highlight ink is corrected to at least 4.5:1 against
  its composited background. Already readable colors are unchanged; adjustments
  preserve hue rather than replacing all syntax with white. A frame-local color
  cache avoids repeating the same correction for each token.
- Same-appearance theme replacement invalidates prepared highlighting. Font or
  density changes preserve the visible top row, with strict scroll restoration.
- Long paragraphs respect **Settings → Wrap Long Lines in Diff**; split and
  three-way layouts retain their existing wrapping behavior.

## One contextual inspector

- **Commit selected:** metadata, message and changed files; no unrelated composer.
- **Working changes selected:** staged and unstaged files, staging controls and
  the commit composer for that checkout. Unstaged files also expose a discard
  button on hover, using the existing origin-bound confirmation.
- Hiding the composer preserves its draft.
- A late commit load must never replace a newer Working changes selection.
- The graph-focused sidebar offers a Working changes entry instead of duplicating
  the inspector's file lists. Classic mode retains its existing sidebar sections.

A file with both staged and unstaged changes has two distinct selectable sources.
Repository/worktree switching must not redirect file operations or display the
previous checkout's contents as current.

## Acceptance checklist

- [ ] Local/remote refs, tags and multiple refs at one commit remain discoverable.
- [ ] Long labels, several lanes and narrow windows leave messages usable.
- [ ] Photos, fallback nodes, HEAD and selection remain distinguishable.
- [x] Graph -> file -> Graph preserves the selected commit and visible viewport in the smoke fixture.
- [ ] Repeated file selection reuses one view; closing it returns to Graph.
- [x] Commit -> Working changes replaces old commit details.
- [x] Staged and unstaged versions of the same file open the correct diff.
- [ ] Stage/unstage operations refresh both inspector and graph counts.
- [ ] Clean/unborn repositories and conflicts have sensible working-state views.
- [x] Linked-worktree changes and whole-checkout stage/unstage remain checkout-bound in the smoke fixture.
- [x] Switching repositories retains each repository's center view.
- [ ] Slow results after selection/view/checkout changes do not restore stale UI.
- [ ] Keyboard focus never remains in an unrendered graph, diff or composer.
- [ ] History, blame, search, reflog, bisect and submodules remain reachable.
- [ ] Switching classic/focused layouts and restarting preserves the choice.

## Validation

Initial graph-layout preview (before the follow-up worktree/diff polish),
Windows, Rust 1.97.1:

- `cargo check --workspace`: passed.
- `cargo test --workspace`: **1,942 passed, 0 failed, 12 ignored**.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Optimized, non-instrumented Windows release build: passed.
- Isolated debug-harness visual tours covered branch/merge/WIP rendering,
  committed and working diffs, Graph return, contextual composer, linked checkout
  inspection, repository return, and saved classic-layout rendering.
- A disposable linked-checkout Stage All / Unstage All replay changed only its
  own index; the main checkout's partially staged README stayed unchanged.

Regression tests cover layout compatibility, source-distinct working selections,
filtered keyboard navigation, visible focus cycles, queued checkout-qualified
file/conflict events, late inspection responses, retained auxiliary loads and
virtual WIP ancestry. The visual tests used separate settings and generated
repositories, not the user's working copies. They are not performance benchmarks.

### Worktree/diff polish validation

- Windows workspace check, strict all-target Clippy, formatting and release build
  passed; **1,962 tests passed, 0 failed, 12 ignored**.
- Final-source isolated visual checks covered Rust additions/removals with
  comments, wrapped Markdown, graph return, clean-worktree visibility, active
  sidebar context and the informational worktree banner. All three fixture
  checkout HEADs remained unchanged.
- Added regressions cover inspect-only commit double-clicks, clean/unscanned
  checkout discovery, confirmed-unborn versus unavailable HEAD, actual viewed
  worktree markers, syntax contrast over composited surfaces, same-appearance
  theme replacement, and strict top-row preservation after font/density changes.
- Runtime visual coverage is still Windows/Forest, not every supported theme,
  font scale, platform or large-repository workload.

## Current boundaries

- Working changes uses grouped flat lists, with filename-first labels, directory
  context, full-path tooltips and search. Historical commit tree view remains;
  the classic sidebar retains its richer file-tree/kind-filter controls.
- Center view/selection state is retained in-session. The layout preference is
  persisted; checkout transitions intentionally clear stale file/tool context.
- An additional smoke check found an existing **untracked-file hunk-staging**
  edge case: Git can reject the patch with `index does not contain ...`. Use the
  whole-file `+` control or Stage All for a newly untracked file. This Git-layer
  issue is not fixed by the layout change; whole-checkout staging was verified.
- Full large-history/15-repository acceptance, every theme/font scale,
  non-Windows runtime checks, and comprehensive destructive-action interaction
  testing remain outstanding. No latency, memory or idle-CPU claim is made.
