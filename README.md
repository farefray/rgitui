<p align="center">
  <img src="assets/icons/app-icon-256.png" width="128" alt="rgitui logo" />
</p>

<h1 align="center">rgitui</h1>

<p align="center">
  A GPU-accelerated desktop Git client built with <a href="https://github.com/zed-industries/zed/tree/main/crates/gpui">GPUI</a> and Rust.
</p>

<p align="center">
  <a href="https://github.com/noahbclarkson/rgitui/actions/workflows/ci.yml"><img src="https://github.com/noahbclarkson/rgitui/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="https://github.com/noahbclarkson/rgitui/releases/latest"><img src="https://img.shields.io/github/v/release/noahbclarkson/rgitui?label=release&color=blue" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="License: MIT"></a>
  <a href="https://github.com/noahbclarkson/rgitui/releases"><img src="https://img.shields.io/github/downloads/noahbclarkson/rgitui/total?color=orange" alt="Downloads"></a>
</p>

<br />

<p align="center">
  <img src="assets/screenshots/main.png" width="100%" alt="rgitui screenshot" />
</p>

---

> [!WARNING]
> **rgitui is still in early, active development.** Expect bugs, rough edges, and breaking changes between releases. If you run into something, please [open an issue](https://github.com/noahbclarkson/rgitui/issues) — it genuinely helps.

---

## Highlights

- **GPU-accelerated** rendering via [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's UI framework) — smooth 60 fps, even on large repos
- **Multi-repo tabs** — scroll through open repositories, search open/recent repos, and switch back to the previous repository
- **Interactive rebase** — pick, squash, reword, fixup, and drop commits visually
- **AI commit messages** — optional Google Gemini integration to draft commit messages from your staged diff
- **Cross-platform** — Windows, Linux (AppImage), and macOS (Intel + Apple Silicon)

## Features

### Git operations

| Category | Operations |
|----------|-----------|
| **Working tree** | Stage / unstage / discard at file, hunk, and line level |
| **Commits** | Commit, amend, cherry-pick, revert |
| **Branches** | Create, checkout, rename, delete, switch |
| **Tags** | Create (annotated & lightweight), delete, checkout |
| **Stash** | Save, pop, apply, drop, create branch from stash |
| **Remote** | Fetch, pull, push, force push; multi-remote support |
| **Merge** | Merge with conflict detection; per-conflict three-way resolver, whole-side choices, and editor handoff |
| **Rebase** | Interactive rebase with pick / squash / reword / fixup / drop |
| **Bisect** | Start, good, bad, skip, reset |
| **Worktrees** | Create, list, switch |
| **Submodules** | Init, update, manage |
| **Other** | Clean untracked files, undo stack, crash recovery |

### Views

- **Commit graph** — branch-first references, connected ancestry lanes, optional hashes, and commit search
- **Diff viewer** — unified and side-by-side diffs plus an interactive Current / Result / Incoming conflict resolver with syntax highlighting (syntect)
- **Blame view** — per-line author info with avatars
- **File history** — commit history filtered to a single file
- **Reflog** — reference log browser
- **Global search** — full-repo search via `git grep`
- **Contextual inspector** — selected commit details or staged/unstaged working changes, with checkout-bound staging controls

### GitHub integration

- Device-flow authentication
- Create pull requests from the UI
- Browse issues and pull requests (cached with 60s TTL)

### UI

- **Graph-focused layout** — full-height Graph and reusable file views; return to Graph without losing your place
- **Classic split layout** remains available through the Split view icon at the end of the toolbar (Graph focus returns from the classic panel header)
- Drag-resizable sidebar, detail panel, split-layout diff viewer, and commit panel
- Command palette (`Ctrl+P`) with context-aware commands
- Toolbar with one-click fetch, pull, push, branch, stash, create PR, and more
- Status bar showing branch, ahead/behind, staged/unstaged counts, and operation status
- Toast notifications and confirmation dialogs for destructive actions
- Animated splash screen

### Theming

Built-in themes:

- **Forest** (fork default for new settings): green-charcoal surfaces, olive actions,
  muted-blue selection, and higher-contrast text
- **Catppuccin Mocha**, **Catppuccin Latte**, **Cream & Blue**
- **One Dark**, **GitHub Dark**, **Dracula**, **High Contrast Dark**
- Custom JSON themes via the config directory

Existing theme choices are preserved. Select **Forest** in Settings → Appearance
to try it. Interface text defaults to embedded **IBM Plex Sans**; code and hashes
remain monospace, and an explicit `ui_font` setting still takes precedence.
See [UX and theming](docs/UX_THEMING.md) for the visual hierarchy and review checklist,
and [Graph-first workspace](docs/GRAPH_WORKSPACE.md) for graph/file navigation and the
contextual working-changes inspector.

### Performance

- Pre-computed trig tables for graph rendering
- LRU caching for styled diff rows, blame, file history, and avatars
- Diff prefetching (±25 commits, 200-entry cache)
- Parallelized diff stats, stash, and worktree enumeration
- All git operations run on a background executor to keep the UI thread free

## Installation

### Pre-built binaries

Download the latest release for your platform from the [Releases](https://github.com/noahbclarkson/rgitui/releases) page:

| Platform | Artifact |
|----------|----------|
| **Windows** | `.zip` (portable) or `.exe` installer (adds to PATH, integrates with Add/Remove Programs) |
| **Linux** | `.AppImage` (recommended) or `.tar.gz` |
| **macOS Apple Silicon** | `.dmg` (aarch64) |

Each release includes a `SHA256SUMS.txt` file for verification.

> **Note:** Binaries are not code-signed. On Windows, click "More info" then "Run anyway" in SmartScreen. On macOS, right-click the app and choose "Open", or run `xattr -cr /Applications/rgitui.app` from the terminal. **macOS builds are experimental** — the `.dmg` may trigger Gatekeeper warnings. Building from source (`cargo build --release`) is reliable and recommended for macOS users.

### Build from source

**Prerequisites:**

- Rust stable toolchain

**Linux system dependencies:**

```bash
sudo apt-get install -y \
  build-essential cmake clang lld pkg-config \
  libasound2-dev libfontconfig-dev libfreetype-dev libgit2-dev \
  libglib2.0-dev libssl-dev libsqlite3-dev libva-dev \
  libvulkan1 libvulkan-dev libwayland-dev libx11-xcb-dev \
  libxcb1-dev libxkbcommon-x11-dev libxkbcommon-dev \
  libxcomposite-dev libxdamage-dev libxext-dev libxfixes-dev \
  libxrandr-dev libxi-dev libxcursor-dev libdrm-dev \
  libgbm-dev libzstd-dev vulkan-tools
```

**Build and run:**

```bash
git clone https://github.com/noahbclarkson/rgitui.git
cd rgitui
cargo build --release
./target/release/rgitui            # or: cargo run --release
```

Open a specific repository:

```bash
rgitui /path/to/repo
```

### Windows: launch the latest local build

Double-click `rgitui.cmd`, or run `rgitui.cmd "C:\path\to\repo"`.
The launcher picks the most recently modified `target/debug/rgitui.exe` or
`target/release/rgitui.exe` on every launch. It does not download or build anything;
run `cargo build --package rgitui` (optionally `--release`) to update it.
Without a repository argument, normal saved-workspace restoration applies.

Create **rgitui (latest)** and **rgitui (build & run)** shortcuts on your Desktop
and in the Start Menu:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/install-windows-shortcuts.ps1
```

**rgitui (build & run)** runs an incremental debug build of the current local
source, then launches that exact build. Close the debug app first so Windows can
replace its executable. Build progress appears in a console; failures show an
error and never launch an older build. Cargo must be on your PATH. This does not
pull changes from GitHub. You can also run it directly:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-run-windows.ps1
# Build without launching:
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-run-windows.ps1 -BuildOnly
```

Re-run the installer if you move the checkout. Shortcuts use the app icon;
**latest** opens silently, while **build & run** closes its console after a
successful launch. These helpers use this checkout's default `target` directory,
not a custom `CARGO_TARGET_DIR` or cross-compilation directory.

To remove the old named preview bundles/copies (without touching current builds,
demo repos, screenshots, or settings), preview the cleanup first:

```powershell
./scripts/clean-windows-builds.ps1
./scripts/clean-windows-builds.ps1 -Apply
# Also clear regenerable incremental compilation caches (next build is slower):
./scripts/clean-windows-builds.ps1 -Incremental -Apply
```

### NixOS on WSL (WSLg)

Use a WSLg-enabled session with working host GPU drivers and the NixOS graphics
libraries already available at `/run/opengl-driver/lib`. From the checkout:

```bash
nix-shell
cargo build --release --locked -p rgitui
bash scripts/run-wsl.sh /path/to/repo
```

Run the launcher **inside the project's `nix-shell`** for its runtime libraries;
it does not enter Nix automatically or install dependencies. From another working
directory in that shell, use `bash /path/to/rgitui/scripts/run-wsl.sh [repo]`.
The binary is always the checkout's `target/release/rgitui`; arguments and the
caller's working directory are preserved.

The launcher retains `WAYLAND_DISPLAY` (Wayland preferred), sets `DISPLAY=:0`
and `GALLIUM_DRIVER=d3d12`, clears `LIBGL_ALWAYS_INDIRECT`,
`LIBGL_ALWAYS_SOFTWARE`, `VK_DRIVER_FILES`, and `VK_ICD_FILENAMES`, and prepends
`/run/opengl-driver/lib` to the existing `LD_LIBRARY_PATH`. These changes apply
only to the launched process, not global configuration. It does not force Vulkan
or verify hardware acceleration; a successful launch alone does not rule out
software rendering. If graphics initialization fails, check the WSLg/host driver
and NixOS graphics setup rather than treating software fallback as success.

When the compositor does not supply window decorations, the main window has a
thin resize border: drag an edge or corner to resize, drag the blank title-bar
space to move, and double-click that space to maximize/restore. Resize handles
are hidden on maximized/fullscreen windows and on tiled edges. Branch and
window-control buttons remain independent of the drag area. Native Windows and
server-decorated Linux windows keep their existing window management.

Window snapping and Windows snap layouts are compositor-controlled; WSLg may
behave differently from the native Windows app. This launches the **Linux** app,
using Linux-native configuration (`$XDG_CONFIG_HOME/rgitui/settings.json`, or
`~/.config/rgitui/settings.json` by default), not the native Windows app's config.

## Keyboard shortcuts

### Global

| Key | Action |
|-----|--------|
| `Ctrl+P` | Command palette |
| `Ctrl+,` | Settings |
| `Ctrl+O` | Open repository |
| `Ctrl+Shift+O` | Search open and recent repositories |
| `` Ctrl+` `` | Switch to the previously used repository |
| `Ctrl+F` | Search commit graph |
| `Ctrl+Shift+F` | Global search (`git grep`) |
| `Ctrl+Shift+G` | Switch Graph / file view (classic layout: hide / show graph) |
| `Ctrl+Shift+R` | Fetch |
| `Ctrl+G` | Generate AI commit message |
| `Ctrl+Enter` | Commit staged changes |
| `Ctrl+S` | Stage all |
| `Ctrl+U` | Unstage all |
| `Ctrl+B` | Create branch |
| `Ctrl+Z` | Stash save |
| `Ctrl+Shift+Z` | Stash pop |
| `Ctrl+Tab` | Next tab |
| `Ctrl+W` | Close tab |
| `F5` | Refresh |
| `?` | Shortcuts help |

### Multiple repositories and worktrees

The **Repositories** button and `Ctrl+Shift+O` search open repositories first,
then recently used repositories. Search by name, path, or selected worktree.
Tabs scroll horizontally; keyboard activation reveals the selected tab. Hover a
tab to see its full checkout path. On macOS, the repository search and
previous-repository shortcuts use Command instead of Ctrl.

Worktrees stay inside their repository's tab. Select one in the sidebar, then use
**Exit worktree** to return to the main checkout without deleting anything. Commit
drafts are kept separately for each checkout while its repository tab stays open.
The next session restores repository order, the active repository, and selected
worktrees. Closing a tab while a tracked Git operation is running is refused;
wait for the operation to finish first.

Interactive rebase/squash, bisect, and previous-branch switching are currently
main-checkout-only. In a linked worktree these actions show a warning instead of
silently operating on the main checkout; use Git in that worktree's terminal.

### Navigation

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up in active list |
| `g` / `Shift+G` | Jump to first / last item |
| `[` / `]` | Previous / next hunk (diff) or commit (detail panel) |
| `Tab` / `Shift+Tab` | Cycle panel focus |
| `Alt+1`..`Alt+4` | Focus sidebar / graph / detail / diff |
| `Ctrl+[` / `Ctrl+]` | Resize detail panel |
| `Ctrl+Up` / `Ctrl+Down` | Resize diff viewer in classic split layout |
| `/` | Search in focused panel |

### Diff viewer

| Key | Action |
|-----|--------|
| `d` | Toggle unified / side-by-side |
| `p` | Toggle line-selection mode |
| `s` | Stage selected hunks/lines |
| `u` | Unstage selected hunks/lines |
| `Ctrl+C` | Copy selected lines |
| `Ctrl+A` | Select all |

### Sidebar

| Key | Action |
|-----|--------|
| `Enter` / `Space` | Activate selected item |
| `s` | Stage / unstage selected file |
| `x` / `Delete` | Delete branch, tag, stash, or discard file |

### Other views

| Key | Action |
|-----|--------|
| `b` | Open blame view |
| `h` | Open file history view |
| `d` | Switch to diff view |
| `Shift+D` | Toggle diff display mode |
| `y` | Copy commit SHA |
| `Shift+C` | Copy commit message |

## Architecture

Cargo workspace with 9 crates:

| Crate | Purpose |
|-------|---------|
| `rgitui` | Binary entry point, splash screen, window management |
| `rgitui_workspace` | Root workspace view — sidebar, toolbar, panels, dialogs, command palette |
| `rgitui_git` | Git operations via `git2` (local) and shell `git` (network), repository state |
| `rgitui_graph` | Commit graph visualization — lane layout, Bezier edges, canvas rendering |
| `rgitui_diff` | Virtualized diff viewer — unified, side-by-side, three-way conflict modes |
| `rgitui_ui` | Reusable component library — buttons, labels, badges, modals, toasts, text inputs |
| `rgitui_theme` | Theme system with semantic color tokens and JSON theme loader |
| `rgitui_ai` | AI commit message generation (Google Gemini) |
| `rgitui_settings` | Settings persistence (JSON config), keychain integration, workspace snapshots |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

[MIT](LICENSE)
