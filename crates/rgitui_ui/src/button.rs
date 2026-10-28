use gpui::prelude::*;
use gpui::{
    div, px, AnyElement, AnyView, App, ClickEvent, CursorStyle, ElementId, Hsla, MouseButton,
    SharedString, StyleRefinement, Window,
};
use rgitui_theme::{
    contrast_ratio, contrast_text_on, ActiveTheme, Color, StatusColors, StyledExt, ThemeColors,
};

use crate::{Icon, IconName, IconSize, Label, LabelSize, Tooltip};

/// A tooltip factory closure matching GPUI's native `.tooltip()` signature.
type TooltipBuilder = Box<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>;

/// Button visual styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    /// Primary action: solid accent with a contrasting foreground.
    Primary,
    /// Solid background color.
    Filled,
    /// Default: transparent, shows bg on hover.
    #[default]
    Subtle,
    /// Border + semi-transparent background.
    Outlined,
    /// Semantic tinted button.
    Tinted(TintColor),
    /// Fully transparent, only text color changes.
    Transparent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TintColor {
    Accent,
    Error,
    Warning,
    Success,
}

/// Button sizes controlling height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Large,
    #[default]
    Default,
    Compact,
    None,
}

impl ButtonSize {
    pub fn height(&self) -> gpui::Rems {
        match self {
            ButtonSize::Large => gpui::rems(2.0),     // 32px
            ButtonSize::Default => gpui::rems(1.75),  // 28px
            ButtonSize::Compact => gpui::rems(1.375), // 22px
            ButtonSize::None => gpui::rems(1.0),      // 16px
        }
    }

    fn icon_size(&self) -> IconSize {
        match self {
            ButtonSize::Large => IconSize::Medium,
            ButtonSize::Default => IconSize::Small,
            ButtonSize::Compact => IconSize::XSmall,
            ButtonSize::None => IconSize::XSmall,
        }
    }

    fn label_size(&self) -> LabelSize {
        match self {
            ButtonSize::Large => LabelSize::Default,
            ButtonSize::Default => LabelSize::Small,
            ButtonSize::Compact => LabelSize::XSmall,
            ButtonSize::None => LabelSize::XSmall,
        }
    }
}

/// Colors are resolved without a GPUI context so state precedence can be tested.
struct ResolvedColors {
    background: Option<Hsla>,
    hover: Option<Hsla>,
    active: Option<Hsla>,
    border: Hsla,
    focus_border: Hsla,
}

fn primary_background(colors: &ThemeColors) -> Hsla {
    // A primary action is always solid, even with a translucent custom accent.
    Hsla {
        a: 1.0,
        ..colors.text_accent
    }
}

impl ButtonStyle {
    fn colors(
        self,
        colors: &ThemeColors,
        status: &StatusColors,
        selected: bool,
        disabled: bool,
    ) -> ResolvedColors {
        let mut resolved = ResolvedColors {
            background: None,
            hover: Some(colors.ghost_element_hover),
            active: Some(colors.ghost_element_active),
            border: colors.border_transparent,
            focus_border: colors.border_focused,
        };

        if disabled {
            if self.is_solid() {
                resolved.background = Some(colors.element_disabled);
                resolved.border = colors.border_disabled;
            }
            resolved.hover = None;
            resolved.active = None;
            return resolved;
        }

        match self {
            ButtonStyle::Primary => {
                let background = primary_background(colors);
                let foreground = contrast_text_on(background);
                // Use one direction for both states, keeping readable contrast.
                // Falling back away from the foreground also handles mid-tone
                // accents; moving toward it handles pure black/white accents.
                let shade = if contrast_ratio(
                    background.blend(foreground.opacity(0.20)),
                    foreground,
                ) >= 4.5
                {
                    foreground
                } else {
                    contrast_text_on(foreground)
                };
                resolved.background = Some(background);
                resolved.hover = Some(background.blend(shade.opacity(0.10)));
                resolved.active = Some(background.blend(shade.opacity(0.20)));
                // The theme's focus accent can equal the fill. An inset,
                // contrasting border stays visible even in that case.
                resolved.focus_border = foreground;
                if selected {
                    resolved.border = foreground.opacity(0.55);
                }
            }
            ButtonStyle::Filled | ButtonStyle::Outlined => {
                resolved.background = Some(colors.element_background);
                resolved.hover = Some(colors.element_hover);
                resolved.active = Some(colors.element_active);
                if self == ButtonStyle::Outlined {
                    resolved.border = colors.border;
                }
            }
            ButtonStyle::Tinted(tint) => {
                let tint_background = match tint {
                    TintColor::Accent => colors.text_accent.opacity(0.15),
                    TintColor::Error => status.error_background,
                    TintColor::Warning => status.warning_background,
                    TintColor::Success => status.success_background,
                };
                // Resolve a stable surface first. Changing only alpha provides
                // no hover/press feedback for opaque custom status backgrounds.
                let surface = Hsla {
                    a: 1.0,
                    ..colors.surface_background
                }
                .blend(colors.element_background);
                let background = surface.blend(tint_background);
                let shade = contrast_text_on(background);
                resolved.background = Some(background);
                resolved.hover = Some(background.blend(shade.opacity(0.06)));
                resolved.active = Some(background.blend(shade.opacity(0.12)));
            }
            ButtonStyle::Subtle | ButtonStyle::Transparent => {}
        }

        if selected && self != ButtonStyle::Primary {
            let background = if self.is_solid() {
                colors.element_selected
            } else {
                colors.ghost_element_selected
            };
            resolved.background = Some(background);
            resolved.hover = Some(background.blend(colors.element_hover.opacity(0.12)));
            resolved.active = Some(background.blend(colors.element_active.opacity(0.20)));
            // Hover and press never replace this persistent selected boundary.
            resolved.border = colors.border_selected;
        }

        resolved
    }

    fn foreground(
        self,
        colors: &ThemeColors,
        selected: bool,
        disabled: bool,
        requested: Color,
    ) -> Color {
        if disabled {
            // Keep semantic resolution: icons and labels have separate tokens.
            Color::Disabled
        } else if self == ButtonStyle::Primary {
            // Explicit colors must not defeat contrast on a primary action.
            Color::Custom(contrast_text_on(primary_background(colors)))
        } else if selected {
            Color::Accent
        } else {
            requested
        }
    }

    fn is_solid(self) -> bool {
        matches!(
            self,
            ButtonStyle::Primary
                | ButtonStyle::Filled
                | ButtonStyle::Outlined
                | ButtonStyle::Tinted(_)
        )
    }
}

/// Shared base for [`Button`] and [`IconButton`].
///
/// Owns the single style, state, cursor, focus, and click-handling path so both
/// public components resolve appearance and behavior identically. Callers build a
/// `ButtonLike`, push their content as children, and render it.
#[derive(IntoElement)]
struct ButtonLike {
    id: ElementId,
    style: ButtonStyle,
    size: ButtonSize,
    disabled: bool,
    selected: bool,
    full_width: bool,
    fixed_size: bool,
    tab_index: isize,
    tooltip: Option<TooltipBuilder>,
    on_click: Option<crate::ClickHandler>,
    children: Vec<AnyElement>,
}

impl ButtonLike {
    fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: ButtonStyle::default(),
            size: ButtonSize::default(),
            disabled: false,
            selected: false,
            full_width: false,
            fixed_size: false,
            tab_index: 0,
            tooltip: None,
            on_click: None,
            children: Vec::new(),
        }
    }

    fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    /// Render as a square button sized to [`ButtonSize::height`] on both axes.
    fn fixed_size(mut self, fixed_size: bool) -> Self {
        self.fixed_size = fixed_size;
        self
    }

    fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = tab_index;
        self
    }

    fn tooltip(mut self, tooltip: TooltipBuilder) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    fn on_click(mut self, on_click: Option<crate::ClickHandler>) -> Self {
        self.on_click = on_click;
        self
    }
}

impl ParentElement for ButtonLike {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ButtonLike {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.colors();
        let resolved = self
            .style
            .colors(colors, cx.status(), self.selected, self.disabled);

        let height = self.size.height();
        let padding = match self.size {
            ButtonSize::Large => px(12.),
            ButtonSize::Default => px(8.),
            ButtonSize::Compact => px(6.),
            ButtonSize::None => px(4.),
        };

        // Reserve the border in every state: focus/selection never move content.
        let mut container = div()
            .id(self.id)
            .h_flex()
            .h(height)
            .rounded_md()
            .border_1()
            .border_color(resolved.border);
        if let Some(background) = resolved.background {
            container = container.bg(background);
        }

        // Disabled controls are conventionally removed from the keyboard tab order.
        if !self.disabled {
            container = container.tab_index(self.tab_index);
        }

        if self.fixed_size {
            container = container.w(height).justify_center();
        } else {
            container = container.px(padding).gap_1();
        }

        if self.full_width {
            container = container.w_full().justify_center();
        }

        if self.disabled {
            // Children already use disabled tokens; do not dim them a second time.
            container = container.cursor_not_allowed();
        } else {
            let focus_color = resolved.focus_border;
            container = container
                .cursor(CursorStyle::PointingHand)
                .focus_visible(move |s: StyleRefinement| s.border_color(focus_color));

            container = match (resolved.hover, resolved.active) {
                (Some(hover), Some(active)) => container
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(active)),
                (Some(hover), None) => container.hover(move |s| s.bg(hover)),
                (None, Some(active)) => container.active(move |s| s.bg(active)),
                (None, None) => container,
            };

            if let Some(on_click) = self.on_click {
                container = container
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(on_click);
            }
        }

        container = container.children(self.children);

        if let Some(tooltip) = self.tooltip {
            container = container.tooltip(move |window, cx| tooltip(window, cx));
        }

        container
    }
}

/// A clickable button with optional icon and label.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    label_color: Option<Color>,
    icon: Option<IconName>,
    icon_position: IconPosition,
    style: ButtonStyle,
    size: ButtonSize,
    disabled: bool,
    selected: bool,
    full_width: bool,
    tab_index: isize,
    tooltip: Option<TooltipBuilder>,
    on_click: Option<crate::ClickHandler>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconPosition {
    #[default]
    Start,
    End,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            label_color: None,
            icon: None,
            icon_position: IconPosition::Start,
            style: ButtonStyle::default(),
            size: ButtonSize::default(),
            disabled: false,
            selected: false,
            full_width: false,
            tab_index: 0,
            tooltip: None,
            on_click: None,
        }
    }

    /// Set a plain-text tooltip.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(Box::new(Tooltip::text(tooltip.into())));
        self
    }

    /// Set a tooltip from a factory closure matching GPUI's native `.tooltip()`
    /// signature, allowing richer tooltips (for example with a shortcut hint).
    pub fn tooltip_fn(
        mut self,
        tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.tooltip = Some(Box::new(tooltip));
        self
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn icon_position(mut self, pos: IconPosition) -> Self {
        self.icon_position = pos;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.label_color = Some(color);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    /// Set the keyboard tab-navigation index. Defaults to `0`, which makes the
    /// button focusable and reachable via Tab.
    pub fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = tab_index;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let label_color = self.style.foreground(
            cx.colors(),
            self.selected,
            self.disabled,
            self.label_color.unwrap_or(Color::Default),
        );

        let icon_el = self.icon.map(|name| {
            Icon::new(name)
                .size(self.size.icon_size())
                .color(label_color)
        });
        let mut label_el = Label::new(self.label)
            .size(self.size.label_size())
            .color(label_color);
        if self.style == ButtonStyle::Primary {
            label_el = label_el.weight(gpui::FontWeight::SEMIBOLD);
        }

        let mut base = ButtonLike::new(self.id)
            .style(self.style)
            .size(self.size)
            .disabled(self.disabled)
            .selected(self.selected)
            .full_width(self.full_width)
            .tab_index(self.tab_index)
            .on_click(self.on_click);

        if let Some(tooltip) = self.tooltip {
            base = base.tooltip(tooltip);
        }

        match self.icon_position {
            IconPosition::Start => base.children(icon_el).child(label_el),
            IconPosition::End => base.child(label_el).children(icon_el),
        }
    }
}

/// An icon-only button.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    color: Color,
    style: ButtonStyle,
    size: ButtonSize,
    disabled: bool,
    selected: bool,
    tab_index: isize,
    tooltip: Option<TooltipBuilder>,
    on_click: Option<crate::ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            color: Color::Default,
            style: ButtonStyle::Subtle,
            size: ButtonSize::Default,
            disabled: false,
            selected: false,
            tab_index: 0,
            tooltip: None,
            on_click: None,
        }
    }

    /// Set a plain-text tooltip.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(Box::new(Tooltip::text(tooltip.into())));
        self
    }

    /// Set a tooltip from a factory closure matching GPUI's native `.tooltip()`
    /// signature, allowing richer tooltips (for example with a shortcut hint).
    pub fn tooltip_fn(
        mut self,
        tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.tooltip = Some(Box::new(tooltip));
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Set the keyboard tab-navigation index. Defaults to `0`, which makes the
    /// button focusable and reachable via Tab.
    pub fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = tab_index;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let icon_color =
            self.style
                .foreground(cx.colors(), self.selected, self.disabled, self.color);

        let icon_el = Icon::new(self.icon)
            .size(self.size.icon_size())
            .color(icon_color);

        let mut base = ButtonLike::new(self.id)
            .style(self.style)
            .size(self.size)
            .disabled(self.disabled)
            .selected(self.selected)
            .fixed_size(true)
            .tab_index(self.tab_index)
            .on_click(self.on_click);

        if let Some(tooltip) = self.tooltip {
            base = base.tooltip(tooltip);
        }

        base.child(icon_el)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgitui_theme::{builtin_themes, hex_to_hsla};

    const STYLES: [ButtonStyle; 9] = [
        ButtonStyle::Primary,
        ButtonStyle::Filled,
        ButtonStyle::Outlined,
        ButtonStyle::Subtle,
        ButtonStyle::Transparent,
        ButtonStyle::Tinted(TintColor::Accent),
        ButtonStyle::Tinted(TintColor::Error),
        ButtonStyle::Tinted(TintColor::Warning),
        ButtonStyle::Tinted(TintColor::Success),
    ];

    #[test]
    fn primary_foreground_contrasts_in_every_enabled_state() {
        for theme in builtin_themes() {
            for selected in [false, true] {
                let resolved =
                    ButtonStyle::Primary.colors(&theme.colors, &theme.status, selected, false);
                let Color::Custom(foreground) =
                    ButtonStyle::Primary.foreground(&theme.colors, selected, false, Color::Error)
                else {
                    panic!("primary must resolve a contrasting foreground")
                };
                assert_eq!(resolved.focus_border, foreground);
                for background in [resolved.background, resolved.hover, resolved.active] {
                    let background = background.expect("primary always has a surface");
                    assert_eq!(background.a, 1.0);
                    assert!(
                        contrast_ratio(background, foreground) >= 4.5,
                        "{}",
                        theme.name
                    );
                }
                assert_ne!(resolved.background, resolved.hover);
                assert_ne!(resolved.hover, resolved.active);
            }
        }
    }

    #[test]
    fn primary_normalizes_custom_alpha_and_handles_black_and_white_accents() {
        let theme = builtin_themes().remove(0);
        let mut colors = theme.colors.clone();
        for accent in ["#00000040", "#ffffff80", "#82937140", "#77777780"] {
            colors.text_accent = hex_to_hsla(accent);
            let resolved = ButtonStyle::Primary.colors(&colors, &theme.status, false, false);
            assert_eq!(
                resolved.background,
                Some(Hsla {
                    a: 1.0,
                    ..colors.text_accent
                })
            );
            let Color::Custom(foreground) =
                ButtonStyle::Primary.foreground(&colors, false, false, Color::Accent)
            else {
                panic!("primary must resolve a contrasting foreground")
            };
            for background in [resolved.background, resolved.hover, resolved.active] {
                assert!(
                    contrast_ratio(background.unwrap(), foreground) >= 4.5,
                    "{accent}"
                );
            }
            assert_ne!(resolved.background, resolved.hover, "{accent}");
            assert_ne!(resolved.hover, resolved.active, "{accent}");
        }
    }

    #[test]
    fn disabled_wins_over_selection_and_requested_foreground() {
        for theme in builtin_themes() {
            for style in STYLES {
                for selected in [false, true] {
                    let resolved = style.colors(&theme.colors, &theme.status, selected, true);
                    assert_eq!(
                        style.foreground(&theme.colors, selected, true, Color::Error),
                        Color::Disabled
                    );
                    assert_eq!(resolved.hover, None);
                    assert_eq!(resolved.active, None);
                    if style.is_solid() {
                        assert_eq!(resolved.background, Some(theme.colors.element_disabled));
                        assert_eq!(resolved.border, theme.colors.border_disabled);
                    } else {
                        assert_eq!(resolved.background, None);
                        assert_eq!(resolved.border, theme.colors.border_transparent);
                    }
                }
            }
        }
    }

    #[test]
    fn selected_controls_keep_a_surface_and_boundary_separate_from_focus() {
        let theme = builtin_themes().remove(0);
        let mut colors = theme.colors.clone();
        // Do not rely on built-ins giving selected and focused borders different hues.
        colors.border_selected = hex_to_hsla("#ff0000");
        colors.border_focused = hex_to_hsla("#00ff00");
        for style in STYLES
            .into_iter()
            .filter(|style| *style != ButtonStyle::Primary)
        {
            let resolved = style.colors(&colors, &theme.status, true, false);
            assert_eq!(resolved.border, colors.border_selected);
            assert_eq!(resolved.focus_border, colors.border_focused);
            assert_eq!(
                resolved.background,
                Some(if style.is_solid() {
                    colors.element_selected
                } else {
                    colors.ghost_element_selected
                })
            );
            assert!(resolved.hover.is_some());
            assert!(resolved.active.is_some());
            assert_eq!(
                style.foreground(&colors, true, false, Color::Error),
                Color::Accent
            );
            assert_eq!(
                style.foreground(&colors, false, false, Color::Error),
                Color::Error
            );
        }
    }

    #[test]
    fn accent_tint_uses_accent_not_information_status() {
        let theme = builtin_themes().remove(0);
        let mut status = theme.status.clone();
        status.info_background = hex_to_hsla("#ff0000");
        let resolved =
            ButtonStyle::Tinted(TintColor::Accent).colors(&theme.colors, &status, false, false);
        assert_eq!(
            resolved.background,
            Some(
                theme
                    .colors
                    .element_background
                    .blend(theme.colors.text_accent.opacity(0.15))
            )
        );
        status.info_background = hex_to_hsla("#00ff00");
        let changed =
            ButtonStyle::Tinted(TintColor::Accent).colors(&theme.colors, &status, false, false);
        assert_eq!(resolved.background, changed.background);
        assert_eq!(resolved.hover, changed.hover);
        assert_eq!(resolved.active, changed.active);
    }

    #[test]
    fn translucent_custom_tints_are_composited_before_resolving_states() {
        let theme = builtin_themes().remove(0);
        let mut colors = theme.colors.clone();
        colors.surface_background.a = 0.5;
        colors.element_background.a = 0.4;
        let mut status = theme.status.clone();
        status.error_background.a = 0.2;
        let resolved = ButtonStyle::Tinted(TintColor::Error).colors(&colors, &status, false, false);
        for background in [resolved.background, resolved.hover, resolved.active] {
            assert_eq!(background.unwrap().a, 1.0);
        }
        assert_ne!(resolved.background, resolved.hover);
        assert_ne!(resolved.hover, resolved.active);
    }

    #[test]
    fn opaque_tints_still_have_distinct_hover_and_pressed_surfaces() {
        let theme = builtin_themes().remove(0);
        let mut status = theme.status.clone();
        for background in ["#000000", "#ffffff", "#40502e"] {
            status.error_background = hex_to_hsla(background);
            status.warning_background = hex_to_hsla(background);
            status.success_background = hex_to_hsla(background);
            for tint in [TintColor::Error, TintColor::Warning, TintColor::Success] {
                let resolved =
                    ButtonStyle::Tinted(tint).colors(&theme.colors, &status, false, false);
                assert_eq!(resolved.background, Some(hex_to_hsla(background)));
                assert_ne!(resolved.background, resolved.hover);
                assert_ne!(resolved.background, resolved.active);
                assert_ne!(resolved.hover, resolved.active);
            }
        }
    }
}
