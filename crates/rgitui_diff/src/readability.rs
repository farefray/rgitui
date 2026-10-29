//! Diff-local typography and surface-aware ink; no theme schema or palette overrides.
use gpui::{font, px, Font, HighlightStyle, Hsla, TextStyle, Window};
use rgitui_settings::Compactness;
use rgitui_theme::{contrast_ratio, ThemeColors};
use std::{cell::RefCell, collections::HashMap};

pub(super) const MIN_CONTRAST: f32 = 4.5;
pub(super) const WORD_ALPHA: f32 = 0.18;
/// Added/removed line fills: strong enough that a removed line cannot pass for
/// context at a glance, light enough that syntax ink rarely needs correcting.
const LINE_TINT: f32 = 0.11;
/// The number gutter carries the change color more strongly than the code.
const GUTTER_TINT: f32 = 0.20;

/// All diff surfaces are flattened before painting, so nested gutter/word
/// fills have an unambiguous backing.
pub(super) use rgitui_theme::composite;

pub(super) fn tint(color: Hsla, opacity: f32, background: Hsla) -> Hsla {
    composite(
        Hsla {
            a: color.a * opacity,
            ..color
        },
        background,
    )
}

/// Keep already-readable syntax untouched. Otherwise find the smallest HSL
/// lightness change in either direction, retaining hue/saturation (including
/// comments and links). Never substitute all syntax with the theme's text ink.
pub(super) fn readable_ink(ink: Hsla, background: Hsla) -> Hsla {
    if contrast_ratio(composite(ink, background), background) >= MIN_CONTRAST {
        return ink;
    }
    let opaque = Hsla { a: 1.0, ..ink };
    // Leave a small margin for color conversion/rounding in the paint pipeline.
    let target = MIN_CONTRAST + 0.01;
    let mut best: Option<Hsla> = None;
    for end in [0.0, 1.0] {
        if contrast_ratio(Hsla { l: end, ..opaque }, background) < target {
            continue;
        }
        let (mut fail, mut pass) = (ink.l, end);
        for _ in 0..18 {
            let mid = (fail + pass) * 0.5;
            if contrast_ratio(Hsla { l: mid, ..opaque }, background) >= target {
                pass = mid;
            } else {
                fail = mid;
            }
        }
        let candidate = Hsla { l: pass, ..opaque };
        if best.is_none_or(|best| (candidate.l - ink.l).abs() < (best.l - ink.l).abs()) {
            best = Some(candidate);
        }
    }
    best.unwrap_or(opaque)
}

/// Frame-local memoization: a handful of syntax colors repeat over hundreds of
/// visible runs. Exact foreground AND surface keys include alpha and avoid
/// stale results when switching between same-appearance/custom themes.
#[derive(Default)]
pub(super) struct InkCache(RefCell<HashMap<[u32; 8], Hsla>>);

impl InkCache {
    pub fn get(&self, ink: Hsla, background: Hsla) -> Hsla {
        let key = [
            ink.h,
            ink.s,
            ink.l,
            ink.a,
            background.h,
            background.s,
            background.l,
            background.a,
        ]
        .map(f32::to_bits);
        *self
            .0
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| readable_ink(ink, background))
    }

    pub fn highlight(
        &self,
        mut style: HighlightStyle,
        default: Hsla,
        background: Hsla,
    ) -> HighlightStyle {
        let surface = style
            .background_color
            .map_or(background, |word| composite(word, background));
        if style.background_color.is_some() {
            style.background_color = Some(surface);
        }
        style.color = Some(self.get(style.color.unwrap_or(default), surface));
        style
    }
}

#[derive(Clone, Copy)]
pub(super) struct DiffSurfaces {
    pub editor: Hsla,
    pub added: Hsla,
    pub deleted: Hsla,
    pub added_gutter: Hsla,
    pub deleted_gutter: Hsla,
    pub gutter: Hsla,
    pub selected: Hsla,
    pub active: Hsla,
}

impl DiffSurfaces {
    pub fn new(colors: &ThemeColors) -> Self {
        // Custom editor colors may be translucent. The workspace backing is
        // normally opaque; make the final fallback deterministic as well.
        let backing = composite(
            colors.background,
            Hsla {
                a: 1.0,
                ..colors.background
            },
        );
        let editor = composite(colors.editor_background, backing);
        Self {
            editor,
            added: tint(colors.vc_added, LINE_TINT, editor),
            deleted: tint(colors.vc_deleted, LINE_TINT, editor),
            added_gutter: tint(colors.vc_added, GUTTER_TINT, editor),
            deleted_gutter: tint(colors.vc_deleted, GUTTER_TINT, editor),
            gutter: tint(colors.text, 0.035, editor),
            selected: composite(colors.element_selected, editor),
            active: composite(colors.element_active, editor),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CodeTypography {
    pub font: Font,
    pub size: f32,
    pub line_height: f32,
    pub advance: f32,
}

fn line_height(size: f32, glyph_height: f32, compactness: Compactness) -> f32 {
    // Compact mode must never shrink below the glyph box. Density changes
    // leading, not font size; all sizes follow the user's existing preference.
    let leading = compactness.spacing(6.0);
    ((size * 1.2).max(glyph_height) + leading).ceil()
}

fn equal_advances(advances: &[f32]) -> bool {
    advances
        .first()
        .is_some_and(|first| *first > 0.0 && advances.iter().all(|a| (*a - first).abs() < 0.01))
}

impl CodeTypography {
    pub fn resolve(window: &Window, preferred: &str, size: u32, compactness: Compactness) -> Self {
        let size = size.clamp(8, 24) as f32;
        let system = window.text_system();
        let preferred_font = font(
            if preferred.is_empty() {
                "Lilex"
            } else {
                preferred
            }
            .to_string(),
        );
        let preferred_id = system.resolve_font(&preferred_font);
        let advances: Vec<_> = ['i', 'W', '0', ' ', '@']
            .into_iter()
            .filter_map(|ch| {
                system
                    .advance(preferred_id, px(size), ch)
                    .ok()
                    .map(|s| f32::from(s.width))
            })
            .collect();
        // ui_font is not a code-font setting. Honor it when it is genuinely
        // monospaced, otherwise use the bundled mono without changing UI text.
        let font = if !preferred.is_empty() && advances.len() == 5 && equal_advances(&advances) {
            preferred_font
        } else {
            font("Lilex")
        };
        let id = system.resolve_font(&font);
        let glyph_height =
            f32::from(system.ascent(id, px(size))) + f32::from(system.descent(id, px(size))).abs();
        let advance = system
            .advance(id, px(size), '0')
            .map(|s| f32::from(s.width))
            .unwrap_or(size * 0.6);
        Self {
            font,
            size,
            line_height: line_height(size, glyph_height, compactness),
            advance,
        }
    }

    pub fn run_style(&self, mut style: TextStyle, color: Hsla) -> TextStyle {
        style.font_family = self.font.family.clone();
        style.font_features = self.font.features.clone();
        style.font_fallbacks = self.font.fallbacks.clone();
        style.font_weight = self.font.weight;
        style.font_style = self.font.style;
        style.font_size = px(self.size).into();
        style.line_height = px(self.line_height).into();
        style.color = color;
        style.background_color = None;
        style.underline = None;
        style.strikethrough = None;
        style
    }

    pub fn gutter_width(&self, max_number: usize) -> f32 {
        let digits = max_number.max(1).ilog10() + 1;
        (self.advance * digits.max(4) as f32 + 10.0).ceil()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Rgba;
    use rgitui_theme::{builtin_themes, hex_to_hsla};

    #[test]
    fn composite_matches_source_over_not_hsl_interpolation() {
        let result = Rgba::from(tint(hex_to_hsla("#ffffff"), 0.25, hex_to_hsla("#000000")));
        assert!((result.r - 0.25).abs() < 0.0001);
        assert_eq!(result.a, 1.0);
    }

    #[test]
    fn all_theme_surfaces_support_readable_distinct_syntax() {
        for theme in builtin_themes() {
            let s = DiffSurfaces::new(&theme.colors);
            for background in [
                s.editor,
                s.added,
                s.deleted,
                s.selected,
                s.active,
                s.gutter,
                s.added_gutter,
                s.deleted_gutter,
            ] {
                for surface in [
                    background,
                    tint(theme.colors.vc_added, WORD_ALPHA, background),
                    tint(theme.colors.vc_deleted, WORD_ALPHA, background),
                ] {
                    for ink in ["#65737e", "#96b5b4", "#8fa1b3", "#a3be8c", "#bf616a"] {
                        let original = hex_to_hsla(ink);
                        let corrected = readable_ink(original, surface);
                        assert!(
                            contrast_ratio(composite(corrected, surface), surface) >= MIN_CONTRAST,
                            "{} {ink}",
                            theme.name
                        );
                        assert_eq!(corrected.h, original.h);
                        assert_eq!(corrected.s, original.s);
                        if contrast_ratio(original, surface) >= MIN_CONTRAST {
                            assert_eq!(corrected, original);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn custom_translucent_ink_and_mid_tone_surfaces_are_readable() {
        for bg in ["#777777", "#818181", "#000000", "#ffffff", "#3f4423"] {
            let bg = hex_to_hsla(bg);
            let ink = Hsla {
                a: 0.3,
                ..hex_to_hsla("#537689")
            };
            assert!(contrast_ratio(composite(readable_ink(ink, bg), bg), bg) >= MIN_CONTRAST);
        }
    }

    #[test]
    fn surfaces_preserve_editor_and_respect_custom_token_alpha() {
        for theme in builtin_themes() {
            let s = DiffSurfaces::new(&theme.colors);
            assert_eq!(s.editor, theme.colors.editor_background);
            assert_eq!(s.added, tint(theme.colors.vc_added, LINE_TINT, s.editor));
            assert_eq!(
                s.added_gutter,
                tint(theme.colors.vc_added, GUTTER_TINT, s.editor)
            );
        }
        let mut colors = builtin_themes()[0].colors.clone();
        colors.editor_background.a = 0.4;
        colors.vc_added.a = 0.5;
        colors.element_selected = Hsla {
            a: 0.3,
            ..hex_to_hsla("#ff00ff")
        };
        let s = DiffSurfaces::new(&colors);
        assert_eq!(s.editor.a, 1.0);
        assert_eq!(s.selected.a, 1.0);
        assert_eq!(
            s.added,
            composite(
                Hsla {
                    a: 0.5 * LINE_TINT,
                    ..colors.vc_added
                },
                s.editor
            )
        );
        for bg in [s.gutter, s.added_gutter, s.deleted_gutter, s.selected] {
            let ink = readable_ink(colors.text_muted, bg);
            assert!(contrast_ratio(composite(ink, bg), bg) >= MIN_CONTRAST);
        }
    }

    #[test]
    fn ink_cache_keys_actual_surface_and_alpha() {
        let cache = InkCache::default();
        let ink = hex_to_hsla("#65737e");
        for bg in ["#111111", "#ffffff", "#778855"] {
            let bg = hex_to_hsla(bg);
            for ink in [ink, Hsla { a: 0.2, ..ink }] {
                assert_eq!(cache.get(ink, bg), readable_ink(ink, bg));
            }
        }
        assert_eq!(cache.0.borrow().len(), 6);
    }

    #[test]
    fn metrics_scale_without_compact_mode_clipping() {
        for size in [8.0, 14.0, 24.0] {
            let compact = line_height(size, size * 1.3, Compactness::Compact);
            let normal = line_height(size, size * 1.3, Compactness::Default);
            let comfortable = line_height(size, size * 1.3, Compactness::Comfortable);
            assert!(compact >= size * 1.3 + 4.0);
            assert!(compact <= normal && normal <= comfortable);
        }
        assert!(equal_advances(&[8.0; 5]));
        assert!(!equal_advances(&[3.0, 9.0, 7.0, 4.0, 11.0]));
    }

    #[test]
    fn runs_do_not_inherit_proportional_font_or_synthetic_italics() {
        let typography = CodeTypography {
            font: font("Lilex"),
            size: 18.0,
            line_height: 29.0,
            advance: 10.0,
        };
        let inherited = TextStyle {
            font_family: "Arial".into(),
            font_style: gpui::FontStyle::Italic,
            font_weight: gpui::FontWeight::BOLD,
            ..Default::default()
        };
        let style = typography.run_style(inherited, hex_to_hsla("#ffffff"));
        assert_eq!(style.font(), font("Lilex"));
        assert_eq!(style.font_size, px(18.0).into());
        assert_eq!(style.line_height, px(29.0).into());
        assert_eq!(typography.gutter_width(100_000), 70.0);
    }
}
