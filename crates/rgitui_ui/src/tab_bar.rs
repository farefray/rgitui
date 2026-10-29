use gpui::prelude::*;
use gpui::{
    div, px, rems, AnyElement, App, ClickEvent, Context, CursorStyle, ElementId, EntityId,
    FontWeight, Hsla, MouseButton, ScrollHandle, SharedString, Window,
};
use rgitui_theme::{ActiveTheme, Color, StyledExt, ThemeColors};
use smallvec::SmallVec;

use crate::{IconButton, IconName, Label, LabelSize, Tooltip};

struct TabColors {
    background: Hsla,
    hover: Hsla,
    pressed: Hsla,
    border: Hsla,
}

fn tab_colors(colors: &ThemeColors, active: bool) -> TabColors {
    if active {
        TabColors {
            background: colors.tab_active_background,
            // Keep the active surface and selected boundary through interaction.
            hover: colors
                .tab_active_background
                .blend(colors.ghost_element_hover.opacity(0.35)),
            pressed: colors
                .tab_active_background
                .blend(colors.ghost_element_active.opacity(0.50)),
            border: colors.border_selected,
        }
    } else {
        TabColors {
            background: colors.tab_inactive_background,
            hover: colors
                .tab_inactive_background
                .blend(colors.ghost_element_hover),
            pressed: colors
                .tab_inactive_background
                .blend(colors.ghost_element_active),
            border: colors.border,
        }
    }
}

type ReorderHandler = Box<dyn Fn(EntityId, &mut Window, &mut App)>;

struct TabReorder {
    scope: EntityId,
    item: EntityId,
    handler: ReorderHandler,
}

#[derive(Clone)]
struct DraggedTab {
    scope: EntityId,
    item: EntityId,
    label: SharedString,
    dirty: bool,
}

impl DraggedTab {
    fn can_drop_on(&self, scope: EntityId, item: EntityId) -> bool {
        self.scope == scope && self.item != item
    }
}

impl Render for DraggedTab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Tab::new("tab-drag-preview", self.label.clone())
            .active(true)
            .dirty(self.dirty)
            .closeable(false)
    }
}

/// A single tab in a tab bar.
#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    active: bool,
    dirty: bool,
    closeable: bool,
    tooltip: Option<SharedString>,
    on_click: Option<crate::ClickHandler>,
    on_close: Option<crate::ClickHandler>,
    reorder: Option<TabReorder>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            active: false,
            dirty: false,
            closeable: true,
            tooltip: None,
            on_click: None,
            on_close: None,
            reorder: None,
        }
    }

    /// Full repository/worktree identity, shown without truncation on hover.
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    pub fn closeable(mut self, closeable: bool) -> Self {
        self.closeable = closeable;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Opt into dragging and receiving drops from other tabs in the same scope.
    /// The target's handler receives the dragged item's identity; the owner moves
    /// that item to this tab's index. Self-drops and other scopes are ignored.
    pub fn on_reorder(
        mut self,
        scope: EntityId,
        item: EntityId,
        handler: impl Fn(EntityId, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.reorder = Some(TabReorder {
            scope,
            item,
            handler: Box::new(handler),
        });
        self
    }

    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Tab {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.colors();
        let resolved = tab_colors(colors, self.active);

        let label_color = if self.active {
            Color::Default
        } else {
            Color::Muted
        };

        let label_weight = if self.active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };

        let mut tab = div()
            .id(self.id.clone())
            .group("tab")
            .h_flex()
            .h(rems(2.125))
            .flex_shrink_0()
            .min_w(px(100.))
            .max_w(px(240.))
            .px_2()
            .gap_1()
            .bg(resolved.background)
            // Reserve the underline for every tab; activation never shifts text.
            .border_b_2()
            .border_r_1()
            .border_color(resolved.border)
            .cursor(CursorStyle::PointingHand)
            .hover(move |s| s.bg(resolved.hover))
            .active(move |s| s.bg(resolved.pressed));

        if let Some(tooltip) = self.tooltip {
            tab = tab.tooltip(Tooltip::text(tooltip));
        }

        if let Some(on_click) = self.on_click {
            tab = tab.on_click(on_click);
        }

        let reorderable = self.reorder.is_some();
        if let Some(TabReorder {
            scope,
            item,
            handler,
        }) = self.reorder
        {
            let accent = colors.text_accent;
            let drop_background = resolved.background.blend(accent.opacity(0.15));
            tab = tab
                .on_drag(
                    DraggedTab {
                        scope,
                        item,
                        label: self.label.clone(),
                        dirty: self.dirty,
                    },
                    |dragged, _, _, cx| cx.new(|_| dragged.clone()),
                )
                .can_drop(move |value, _, _| {
                    value
                        .downcast_ref::<DraggedTab>()
                        .is_some_and(|dragged| dragged.can_drop_on(scope, item))
                })
                .drag_over::<DraggedTab>(move |style, _, _, _| {
                    style.bg(drop_background).border_color(accent)
                })
                .on_drop(move |dragged: &DraggedTab, window, cx| {
                    if dragged.can_drop_on(scope, item) {
                        handler(dragged.item, window, cx);
                    }
                });
        }

        // Dirty indicator
        if self.dirty {
            tab = tab.child(
                div()
                    .flex_shrink_0()
                    .w(px(6.))
                    .h(px(6.))
                    .rounded_full()
                    .bg(colors.text_accent),
            );
        }

        tab = tab.child(
            div().flex_1().min_w_0().child(
                Label::new(self.label)
                    .size(LabelSize::Small)
                    .weight(label_weight)
                    .color(label_color)
                    .truncate(),
            ),
        );

        // Close button (visible on hover or when active)
        if self.closeable {
            let is_active = self.active;
            let mut close_btn = IconButton::new(
                ElementId::Name(format!("{}-close", self.id).into()),
                IconName::X,
            )
            .size(crate::ButtonSize::Compact)
            .color(Color::Muted);

            if let Some(on_close) = self.on_close {
                close_btn = close_btn.on_click(move |event, window, cx| {
                    // Prevent the click from bubbling to the tab row, which would
                    // otherwise activate the tab on the same gesture that closes it.
                    cx.stop_propagation();
                    on_close(event, window, cx);
                });
            }

            // Only show close button on hover or when tab is active
            tab = tab.child(
                div()
                    .flex_shrink_0()
                    .when(reorderable, |el| {
                        // The close button still receives its click, but pressing
                        // it must not arm a drag on the containing tab.
                        el.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    })
                    .when(!is_active, |el| {
                        el.invisible().group_hover("tab", |s| s.visible())
                    })
                    .child(close_btn),
            );
        }

        tab
    }
}

/// A horizontal tab bar containing multiple tabs.
#[derive(IntoElement)]
pub struct TabBar {
    tabs: SmallVec<[AnyElement; 4]>,
    end_slot: Option<AnyElement>,
    scroll_handle: Option<ScrollHandle>,
}

impl Default for TabBar {
    fn default() -> Self {
        Self::new()
    }
}

impl TabBar {
    pub fn new() -> Self {
        Self {
            tabs: SmallVec::new(),
            end_slot: None,
            scroll_handle: None,
        }
    }

    pub fn tab(mut self, tab: impl IntoElement) -> Self {
        self.tabs.push(tab.into_any_element());
        self
    }

    /// The scroll container's direct children are exactly the tabs, in order.
    /// Owners can call `scroll_to_item(active_index)` after activating a tab.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.scroll_handle = Some(handle.clone());
        self
    }

    pub fn end_slot(mut self, slot: impl IntoElement) -> Self {
        self.end_slot = Some(slot.into_any_element());
        self
    }
}

impl RenderOnce for TabBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.colors();

        let mut bar = div()
            .h_flex()
            .w_full()
            .h(rems(2.25))
            .bg(colors.tab_bar_background)
            .border_b_1()
            .border_color(colors.border)
            .overflow_x_hidden();

        // Scrollable tab area
        let mut tab_area = div()
            .id("tab-bar-scroll")
            .h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_x_scroll();
        if let Some(handle) = &self.scroll_handle {
            tab_area = tab_area.track_scroll(handle);
        }
        for tab in self.tabs {
            tab_area = tab_area.child(tab);
        }
        bar = bar.child(tab_area);

        // End slot (e.g., + button)
        if let Some(end) = self.end_slot {
            bar = bar.child(div().h_flex().flex_shrink_0().px_1().child(end));
        }

        bar
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgitui_theme::{builtin_themes, hex_to_hsla};

    #[test]
    fn tab_surfaces_and_boundaries_follow_theme_roles() {
        for theme in builtin_themes() {
            let active = tab_colors(&theme.colors, true);
            let inactive = tab_colors(&theme.colors, false);
            assert_eq!(active.background, theme.colors.tab_active_background);
            assert_eq!(active.border, theme.colors.border_selected);
            assert_eq!(inactive.background, theme.colors.tab_inactive_background);
            assert_eq!(inactive.border, theme.colors.border);
        }
    }

    #[test]
    fn opaque_hover_tokens_do_not_erase_the_active_tab_surface() {
        let theme = builtin_themes().remove(0);
        let mut colors = theme.colors.clone();
        colors.tab_active_background = hex_to_hsla("#203020");
        colors.tab_inactive_background = hex_to_hsla("#101810");
        colors.ghost_element_hover = hex_to_hsla("#506050");
        colors.ghost_element_active = hex_to_hsla("#607060");
        let active = tab_colors(&colors, true);
        let inactive = tab_colors(&colors, false);
        assert_ne!(active.hover, colors.ghost_element_hover);
        assert_ne!(active.pressed, colors.ghost_element_active);
        assert_ne!(active.hover, inactive.hover);
        assert_ne!(active.pressed, inactive.pressed);
        assert_ne!(active.background, active.hover);
        assert_ne!(active.hover, active.pressed);
        assert_eq!(active.border, colors.border_selected);
    }
}
