use gpui::prelude::*;
use gpui::{
    div, px, AnyElement, App, CursorStyle, Decorations, Div, MouseButton, ResizeEdge, Stateful,
    Window,
};
use rgitui_theme::ActiveTheme;

const RESIZE_BORDER: f32 = 6.0;

/// Supply compositor-driven resize gestures when Linux cannot provide a window
/// frame. Native/server-decorated windows pass through unchanged. The reserved
/// border keeps resize hitboxes out of the workspace's buttons and scrollbars.
///
/// Call at the window root (not around an individual panel). The inset tells
/// Wayland which part of the surface is decoration rather than window content.
pub fn client_window_frame(window: &mut Window, cx: &App, content: impl IntoElement) -> AnyElement {
    if !cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return content.into_any_element();
    }

    let decorations = window.window_decorations();
    let resizable = window.is_resizable();
    let edges = ResizeEdges::for_window(
        decorations,
        window.is_maximized(),
        window.is_fullscreen(),
        resizable,
    );
    let inset = px(
        if resizable && matches!(decorations, Decorations::Client { .. }) {
            RESIZE_BORDER
        } else {
            0.0
        },
    );
    // Keep the CSD inset stable across maximize/fullscreen/restore: Wayland
    // applies it while processing configure events, before this render runs,
    // and already suppresses it on tiled edges. Call every render because X11
    // recalculates per-edge, scale-adjusted frame extents inside this setter.
    window.set_client_inset(inset);
    if !edges.any() {
        return content.into_any_element();
    }

    div()
        .id("client-window-frame")
        .relative()
        .size_full()
        .bg(cx.colors().border)
        .when(edges.top, |el| el.pt(inset))
        .when(edges.bottom, |el| el.pb(inset))
        .when(edges.left, |el| el.pl(inset))
        .when(edges.right, |el| el.pr(inset))
        .child(div().size_full().child(content))
        .children(
            RESIZE_EDGES
                .into_iter()
                .filter(|edge| edges.includes(*edge))
                .map(|edge| resize_handle(edge, edges)),
        )
        .into_any_element()
}

const RESIZE_EDGES: [ResizeEdge; 8] = [
    ResizeEdge::Top,
    ResizeEdge::Right,
    ResizeEdge::Bottom,
    ResizeEdge::Left,
    ResizeEdge::TopLeft,
    ResizeEdge::TopRight,
    ResizeEdge::BottomLeft,
    ResizeEdge::BottomRight,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ResizeEdges {
    top: bool,
    right: bool,
    bottom: bool,
    left: bool,
}

impl ResizeEdges {
    fn for_window(
        decorations: Decorations,
        maximized: bool,
        fullscreen: bool,
        resizable: bool,
    ) -> Self {
        match decorations {
            Decorations::Client { tiling } if resizable && !maximized && !fullscreen => Self {
                top: !tiling.top,
                right: !tiling.right,
                bottom: !tiling.bottom,
                left: !tiling.left,
            },
            _ => Self::default(),
        }
    }

    fn any(self) -> bool {
        self.top || self.right || self.bottom || self.left
    }

    fn includes(self, edge: ResizeEdge) -> bool {
        match edge {
            ResizeEdge::Top => self.top,
            ResizeEdge::Right => self.right,
            ResizeEdge::Bottom => self.bottom,
            ResizeEdge::Left => self.left,
            ResizeEdge::TopLeft => self.top && self.left,
            ResizeEdge::TopRight => self.top && self.right,
            ResizeEdge::BottomLeft => self.bottom && self.left,
            ResizeEdge::BottomRight => self.bottom && self.right,
        }
    }
}

fn resize_handle(edge: ResizeEdge, edges: ResizeEdges) -> Stateful<Div> {
    let border = px(RESIZE_BORDER);
    let top = px(if edges.top { RESIZE_BORDER } else { 0.0 });
    let right = px(if edges.right { RESIZE_BORDER } else { 0.0 });
    let bottom = px(if edges.bottom { RESIZE_BORDER } else { 0.0 });
    let left = px(if edges.left { RESIZE_BORDER } else { 0.0 });
    let (id, cursor) = match edge {
        ResizeEdge::Top => ("window-resize-top", CursorStyle::ResizeUpDown),
        ResizeEdge::Bottom => ("window-resize-bottom", CursorStyle::ResizeUpDown),
        ResizeEdge::Left => ("window-resize-left", CursorStyle::ResizeLeftRight),
        ResizeEdge::Right => ("window-resize-right", CursorStyle::ResizeLeftRight),
        ResizeEdge::TopLeft => ("window-resize-top-left", CursorStyle::ResizeUpLeftDownRight),
        ResizeEdge::BottomRight => (
            "window-resize-bottom-right",
            CursorStyle::ResizeUpLeftDownRight,
        ),
        ResizeEdge::TopRight => (
            "window-resize-top-right",
            CursorStyle::ResizeUpRightDownLeft,
        ),
        ResizeEdge::BottomLeft => (
            "window-resize-bottom-left",
            CursorStyle::ResizeUpRightDownLeft,
        ),
    };
    let handle = div().id(id).absolute().occlude().cursor(cursor);
    let handle = match edge {
        ResizeEdge::Top => handle.top_0().left(left).right(right).h(border),
        ResizeEdge::Bottom => handle.bottom_0().left(left).right(right).h(border),
        ResizeEdge::Left => handle.left_0().top(top).bottom(bottom).w(border),
        ResizeEdge::Right => handle.right_0().top(top).bottom(bottom).w(border),
        ResizeEdge::TopLeft => handle.top_0().left_0().size(border),
        ResizeEdge::TopRight => handle.top_0().right_0().size(border),
        ResizeEdge::BottomLeft => handle.bottom_0().left_0().size(border),
        ResizeEdge::BottomRight => handle.bottom_0().right_0().size(border),
    };
    handle.on_mouse_down(MouseButton::Left, move |_, window, cx| {
        // A real pointer-down serial is required by Wayland. Do not defer this
        // to an on_click callback or implement resizing by setting bounds.
        cx.stop_propagation();
        window.start_window_resize(edge);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Tiling;

    #[test]
    fn floating_client_window_has_all_eight_resize_handles() {
        let edges = ResizeEdges::for_window(
            Decorations::Client {
                tiling: Tiling::default(),
            },
            false,
            false,
            true,
        );
        assert!(edges.any());
        assert!(RESIZE_EDGES.into_iter().all(|edge| edges.includes(edge)));
    }

    #[test]
    fn native_fixed_maximized_and_fullscreen_windows_have_no_handles() {
        let client = Decorations::Client {
            tiling: Tiling::default(),
        };
        for (decorations, maximized, fullscreen, resizable) in [
            (Decorations::Server, false, false, true),
            (client, true, false, true),
            (client, false, true, true),
            (client, false, false, false),
            (
                Decorations::Client {
                    tiling: Tiling::tiled(),
                },
                false,
                false,
                true,
            ),
        ] {
            let edges = ResizeEdges::for_window(decorations, maximized, fullscreen, resizable);
            assert_eq!(edges, ResizeEdges::default());
            assert!(!edges.any());
            assert!(RESIZE_EDGES.into_iter().all(|edge| !edges.includes(edge)));
        }
    }

    #[test]
    fn tiled_edges_and_their_corners_are_not_resizable() {
        let edges = ResizeEdges::for_window(
            Decorations::Client {
                tiling: Tiling {
                    top: true,
                    left: true,
                    ..Tiling::default()
                },
            },
            false,
            false,
            true,
        );
        let handles: Vec<_> = RESIZE_EDGES
            .into_iter()
            .filter(|edge| edges.includes(*edge))
            .collect();
        assert_eq!(
            handles,
            vec![
                ResizeEdge::Right,
                ResizeEdge::Bottom,
                ResizeEdge::BottomRight
            ]
        );
    }
}
