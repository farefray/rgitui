# Fork UX and theming: first pass

## Diagnosis

The repository-tab discovery failure was not just a missing shortcut. In the
supplied rgitui screenshot, navigation, Git actions, headings, metadata and
content had nearly the same visual weight. The source audit confirmed:

- The interface inherited Lilex, a monospace font, even for navigation/actions.
- Essential actions and headings frequently used muted, extra-small labels.
- Adjacent panels and their headers reused visually similar surfaces.
- Several controls appeared as bare text; keyboard focus often looked like hover.
- Hover could replace the selected surface, weakening persistent selection.
- Disabled ghost buttons used both disabled colors and 50% opacity.

GitKraken is the reference for region hierarchy and visible affordances, not a
request to duplicate all of its layout or add another navigation subsystem.
The owner's VS Code screenshot supplies the color direction: green charcoal,
olive/lime, muted blue and warm readable text. **Forest** is an interpretation,
not an exact import or identification of that editor theme.

## Visual system

| Role | Forest color | Intent |
|---|---|---|
| Recessed shell | `#18211e` | Frame the workspace without competing with content |
| Repository tab strip | `#141d18` | A distinct, persistent navigation level |
| Supporting panels | `#202b25` | Sidebar and detail context |
| Reading/editor surface | `#243029` | Central content and editable fields |
| Section/header surface | `#2b3932` | Delimit groups without heavy decoration |
| Action toolbar | `#304037` | Actions belong to the repository above |
| Elevated surface | `#35463b` | Menus/dialogs remain distinguishable |
| Main / secondary text | `#e1e9dd` / `#b5c2b5` | Readable content, quieter metadata |
| Accent | `#b2cf76` | Deliberate action and keyboard focus |
| Selected surface / boundary | `#364e5c` / `#90bed0` | Persistent selection, distinct from hover |

Green, amber, red, violet, blue and teal retain different Git/status meanings.
Selection also uses weight/boundaries; it must not rely on color alone. Borders
between ordinary surfaces are decorative, whereas keyboard-focus boundaries
must remain visibly distinct. Native Windows title-bar accent is OS-controlled.

### Typography and controls

- IBM Plex Sans for interface text, preserving explicit user font choices and
  locale-aware fallback. Code and commit hashes retain monospace styling.
- Readable action and navigation labels; small metadata remains small. Do not
  solve hierarchy by making every row bigger or every label bold.
- A dedicated primary-button style, not a blanket change to every filled button.
  Commit has its own full-width footer row instead of competing with Amend and
  availability hints.
- Stable focus boundaries, persistent active-tab styling, and recognizable
  disabled states without double-dimming.
- Editable fields retain an editor surface; focus changes their boundary.
  Read-only content remains readable rather than looking disabled.

## Scope and compatibility

This pass improves the existing shell and components. It does not move staging
into the right pane, change branch click/checkout semantics, alter Git operation
targeting, or replace the graph/diff architecture. Resizers, compactness controls,
virtualized rows, tab overflow and existing shortcuts remain in place.

Forest becomes the default only when no theme was saved. Existing built-in and
custom themes remain available, and the JSON theme schema has no new required
fields. Shared component improvements apply across themes. Existing users select
**Settings → Appearance → Forest** to see the new palette. An explicit `ui_font`
continues to win over the new proportional default.

## Validation

Automated checks target palette/state invariants, not a claim of complete
accessibility or pixel-perfect rendering:

- Forest main and secondary text: at least **4.5:1** on tested opaque workbench,
  selected, hover and active surfaces; placeholder text on editor surface.
- Forest focus/selection boundaries: at least **3:1** on tested control surfaces.
- Status foreground/background pairs: at least **4.5:1**.
- Solid primary-button ink chosen by sRGB relative luminance, not HSL lightness;
  checked against dark, light and custom accents.
- Theme export/import and saved-theme/default compatibility.

Validated on Windows with Rust 1.97.1:

- `cargo test --workspace`: **1,905 passed, 0 failed, 12 ignored**.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Normal optimized Windows release build: passed.
- Basic rendered smoke test using the optional harness, a disposable repository,
  and isolated settings: repository tabs, working changes, selected commit,
  detail panel, code diff and two-row commit footer rendered successfully.
  Local screenshot: `target/forest-preview-final.png`.

The visual check used a debug instrumented build; its timings are not performance
measurements. Full keyboard interaction, large-font scaling, non-Windows rendering
and every built-in/custom theme still need interactive review. A narrow sidebar
may abbreviate headings/secondary statistics to keep actions and counts visible.

### Interactive review checklist

- Forest, One Dark, a light theme, High Contrast Dark, and an existing custom theme.
- Default size, larger font settings, compact density, and narrow windows.
- Active/inactive/hovered tabs, long duplicate repo names, nested worktree labels.
- Normal, hovered, pressed, keyboard-focused, selected and disabled controls.
- No focused-border layout jumps; disabled actions cannot dispatch.
- Selected graph rows keep their identity while hovered; code stays monospace.
- Commit fields remain readable, and the valid commit action is prominent without
  confusing AI generation with the primary action.
- Scrollbars, resize handles, context menus, tooltips, dialogs and empty states.

## Follow-up, after using the preview

The subsequent [graph workspace pass](GRAPH_WORKSPACE.md) adds contextual
changes/details and explicit worktree navigation, and removes implicit checkout
from commit double-clicks. Its readability follow-up fixes proportional font
inheritance in syntax-rendered code, softens diff backgrounds, and corrects syntax
ink against composited row/selection/word backgrounds. The underlying syntax
palettes are still syntect's light/dark palettes, not a VS Code theme import.
Full keyboard-navigation audits, cross-platform screenshots, and comprehensive
large-font/custom-theme acceptance remain follow-up work.

## Second pass: GitKraken-style chrome and working changes

Compared side by side with GitKraken on a real repository, rgitui spent three
rows on chrome, mixed bordered and borderless toolbar buttons, and made the
working-changes list read as a wall of `Modified` pills with a `+` on every row.

- **One chrome row fewer.** With native window decorations there is no in-app
  title bar: the repository tabs come first, ending in the command palette and
  help hints. Linux client-side decorations keep a slim title bar for window
  controls and moving the window.
- **Toolbar.** A `Repository › Branch` breadcrumb starts the row (repository
  opens the switcher; branch reveals it in the sidebar, or HEAD when detached;
  worktree inspection and repository state live here too). Actions are uniform,
  borderless icon-over-label buttons centered in the remaining space: Undo,
  Fetch, Pull, Push, Branch, Stash, Pop, Create PR and Terminal. Pull/Push carry
  their behind/ahead counts on the icon. Undo names what it would reverse and is
  disabled when the workspace's undo history is empty or expired.
- **Center tabs only when useful.** In the focused layout the Graph/file strip
  appears once a file or tool view is open; the Split view switch is an icon at
  the end of the toolbar.
- **Working changes.** Rows end in a one-letter status column (`M`, `A`, `D`,
  `?`…, with the full name on hover) after the colored status icon; stage,
  unstage and discard appear on hover or on the selected row. Folder rows
  summarize their descendants by kind (edits, additions, removals, conflicts),
  and a section with more than 24 files opens with its top-level folders
  collapsed. Folder expansion is remembered per checkout across visits to
  commits. Stage All is the section's tinted primary action; one Expand/Collapse
  All toggle sits beside Path/Tree. Commit file lists use the same status letter.
- **Commit composer.** The description grows into the panel's spare height, and
  the button says what is missing or what will happen: *Stage Changes to
  Commit*, *Enter a Summary to Commit*, *Commit 3 Files*.

Pure helpers (`directory_tallies`, `default_collapsed`, `UndoStack::peek_label`,
toolbar count rules) are unit-tested; layout was checked in isolated debug-harness
screenshots on Windows. Hover states, Linux client-side decorations and light
themes still need an interactive look.
