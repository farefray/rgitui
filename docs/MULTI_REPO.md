# Multi-repository navigation (v1)

## Agreed scope

This improves an existing feature: repository tabs already existed, but were easy
to miss below the toolbar. Discoverability and reliable switching are the main
product goals, not a second multi-repository subsystem.

Support a daily session of up to 15 independent repositories (a target workload,
not an enforced tab limit). Top-level tabs
represent repositories; linked worktrees are contexts inside their repository's
tab, not additional top-level tabs. Returning from a worktree means returning to
the main checkout, never deleting the worktree.

- Scrollable tabs with active-tab reveal and always-accessible repository search.
- Search open repositories first, then recent repositories; show identifying paths.
- Keep sequential tab shortcuts; add repository search and previous-repository toggle.
- Preserve repository UI state and separate checkout-specific drafts/state.
- Restore repository order, active repository, and selected worktrees.
- Operations, confirmations, retries and async results retain their original target.
- No permanent repository sidebar, named workspace manager or automatic unloading in v1.

## Execution

1. **Safety:** stable project identities, origin-bound confirmations/retries,
   repository-scoped operation tracking, inactive-tab updates and late-result guards.
2. **Navigation:** centralized activation/close handling, repository switcher,
   tab scrolling, duplicate-name disambiguation and keyboard/mouse parity.
3. **Worktrees and persistence:** nested checkout contexts, separated drafts/view
   state, backwards-compatible session data and robust restore.
4. **Integration and verification:** regression tests, formatting, compiler/lint
   checks and a documented manual 15-repository scenario.

## Implemented

- Repository tabs now sit above the toolbar they scope. A labeled **Repositories**
  button opens the searchable list; the add button remains available.
- Search open repositories before recents, by name/path/worktree, with keyboard
  selection, scroll-follow, focus restoration and full-path identification.
- `Ctrl+Shift+O` searches repositories; Ctrl+backtick toggles the previous repository
  (Command equivalents on macOS). Sequential Ctrl+Tab behavior is unchanged.
- Scrollable, bounded-width tabs reveal the active item on activation, not on
  every render. Duplicate names include their parent path.
- Runtime routing uses project entity identity, not mutable tab positions. Paths
  resolved by repository metadata/common Git directory group aliases and worktrees
  under their owning repository, including linked-first opens.
- Operation IDs are scoped by repository. Confirmations carry immutable origin
  payloads, retries resolve their original live target, and queued commit requests
  retain their checkout rather than consulting the current selection.
- Closing/home/workspace replacement is refused during tracked Git operations.
  Per-tab subscriptions are released with the tab; stale callbacks cannot target
  a reopened replacement. Background-operation labels name their origin.
- Each checkout keeps its own in-session commit editors, co-authors, amend state
  and AI undo. Checkout changes cancel only that panel's AI request and invalidate
  late preparation/results. Other repositories' requests keep running.
- Session snapshots retain repository order, active repository and selected
  worktrees, with defaults for old settings files. Unavailable paths are retained.
- Restoration refreshes the active repository first, then inactive repositories
  sequentially. Switching an already-open tab does not refresh the whole session;
  speculative first-page diff prewarming is limited to the visible repository.
- Branch/commit/tag checkout and merge use explicitly captured checkout paths.

### Boundaries

- GUI smoke testing and release-profile performance measurements remain manual;
  no 15-repository latency, CPU or memory claim has been established.
- Drafts and detailed panel state are in-session, not persisted to settings.
  Checkout transitions clear stale file/diff/search content; this is not a full
  per-worktree graph/diff navigation-history implementation.
- Interactive rebase/squash, bisect and previous-branch switching still have
  main-checkout-only backend assumptions. Linked-worktree entry points now warn
  instead of silently mutating the main checkout. Use Git in that worktree's
  terminal until those backends become checkout-aware.
- Named workspace management, pinning/reordering UI, aggregate cache budgets and
  automatic tab unloading remain outside this increment.

## Verification status

Final checks on Windows with the pinned Rust 1.97.1 toolchain:

- `cargo check --workspace`: passed.
- `cargo test --workspace`: **1,882 passed, 0 failed, 12 ignored**, including
  queued confirmation/commit tests, checkout-draft tests, alias/worktree identity,
  missing-session ordering, all active/closed combinations for 15 tabs, and seven
  linked-worktree checkout/merge mutation tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The Windows release preview was built successfully and the user reported that it
works well in an interactive trial. Non-Windows runtime checks and release-profile
performance measurements remain outstanding. The acceptance checklist below is
not a claim that every scenario has been exercised interactively.

## Acceptance checks

- Start operations numbered 1 in repositories A and B; finishing A cannot clear B.
- Open a confirmation in A, switch context, then confirm: never act on B.
- Retry A's failed operation while B is selected: target A or reject if A is closed.
- Close a tab before the active tab: preserve the active repository.
- Close/reopen a repository while reads are running: late results cannot update a
  replacement tab merely because it occupies the same index or has the same path.
- Update an inactive repository: its own panels reconcile, not the active repo's.
- Open 15 repositories: all remain accessible by scrolling and search; keyboard
  activation reveals the active tab without continually resetting manual scrolling.
- Search duplicate repository names: paths make the choices unambiguous.
- Enter/leave a linked worktree inside one repo tab; verify operation targets and
  separate drafts, then switch repositories and return.
- Restart: restore order, active repository and each selected checkout; missing
  paths must not accidentally select a different saved repository.
- Repeat open/close cycles and measure CPU, memory and task/watcher ownership.

## Validation approach

Use pure helper tests for selection, filtering, identity and persistence rules.
Run `cargo fmt --all -- --check`, targeted crate tests, workspace tests and
`cargo clippy --workspace --all-targets -- -D warnings` where the environment
permits. Use the existing performance harness in `docs/PERFORMANCE.md` for
release-profile measurements; GUI behavior and performance require an interactive
run and must be reported separately from source/unit-test validation.
