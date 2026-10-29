use gpui::{Hsla, Rgba};

/// WCAG relative luminance for an opaque sRGB color.
/// Translucent colors must be composited onto their actual surface first.
fn luminance(color: Hsla) -> f32 {
    let rgb = Rgba::from(color);
    let linear = |channel: f32| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b)
}

/// Source-over in sRGB, matching the renderer: `front` painted on `back`.
/// Use it to flatten a translucent fill before measuring or occluding with it.
pub fn composite(front: Hsla, back: Hsla) -> Hsla {
    if front.a == 1.0 {
        return front;
    }
    if front.a == 0.0 {
        return back;
    }
    let f = Rgba::from(front);
    let b = Rgba::from(back);
    let a = f.a + b.a * (1.0 - f.a);
    if a == 0.0 {
        return Hsla::transparent_black();
    }
    Rgba {
        r: (f.r * f.a + b.r * b.a * (1.0 - f.a)) / a,
        g: (f.g * f.a + b.g * b.a * (1.0 - f.a)) / a,
        b: (f.b * f.a + b.b * b.a * (1.0 - f.a)) / a,
        a,
    }
    .into()
}

/// WCAG contrast ratio (1–21) between opaque/composited sRGB colors.
/// Alpha is deliberately not interpreted without a known backing surface.
pub fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Choose black or white ink for a solid accent fill, including custom themes.
/// The brighter-looking color is not always the higher-contrast choice.
pub fn contrast_text_on(background: Hsla) -> Hsla {
    let black = Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.0,
        a: 1.0,
    };
    let white = Hsla { l: 1.0, ..black };
    if contrast_ratio(black, background) >= contrast_ratio(white, background) {
        black
    } else {
        white
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex_to_hsla;

    #[test]
    fn contrast_matches_wcag_reference_values() {
        let black = hex_to_hsla("#000000");
        let white = hex_to_hsla("#ffffff");
        let gray = hex_to_hsla("#777777");
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.001);
        assert!((contrast_ratio(gray, white) - 4.478).abs() < 0.01);
        assert!((contrast_ratio(gray, gray) - 1.0).abs() < 0.001);
        assert_eq!(contrast_ratio(gray, white), contrast_ratio(white, gray));
    }

    #[test]
    fn primary_ink_is_readable_on_dark_light_and_custom_accents() {
        for accent in [
            "#b2cf76", "#0044aa", "#ffffff", "#000000", "#777777", "#ff00ff",
        ] {
            let background = hex_to_hsla(accent);
            assert!(
                contrast_ratio(contrast_text_on(background), background) >= 4.5,
                "{accent}"
            );
        }
        for theme in crate::builtin_themes() {
            let background = theme.colors.text_accent;
            assert!(
                contrast_ratio(contrast_text_on(background), background) >= 4.5,
                "{}",
                theme.name
            );
        }
    }
}
