#!/usr/bin/env bash
# Run the Linux release build in WSLg, from inside the project's nix-shell.
set -euo pipefail

repo_root="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="$repo_root/target/release/rgitui"

if [[ ! -x "$binary" ]]; then
    printf 'Error: Linux release binary missing or not executable: %s\n' "$binary" >&2
    printf 'From %s, enter nix-shell and run: cargo build --release\n' "$repo_root" >&2
    exit 1
fi

# Keep WAYLAND_DISPLAY: prefer Wayland, with WSLg X11 available at :0.
# Override only the launched process; retain nix-shell's library paths.
exec env \
    -u LIBGL_ALWAYS_INDIRECT \
    -u LIBGL_ALWAYS_SOFTWARE \
    -u VK_DRIVER_FILES \
    -u VK_ICD_FILENAMES \
    DISPLAY=:0 \
    GALLIUM_DRIVER=d3d12 \
    LD_LIBRARY_PATH="/run/opengl-driver/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    "$binary" "$@"
