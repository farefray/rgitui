//! Forest: a layered, green-charcoal workbench with olive actions and blue selection.
//! Colors are inspired by the fork owner's editor, not an exact theme import.
use crate::{hex_to_hsla as hex, StatusColors, ThemeColors};

pub fn forest_colors() -> ThemeColors {
    ThemeColors {
        // Recessed shell, supporting panels, and a separate reading surface.
        background: hex("#18211e"),
        surface_background: hex("#2b3932"),
        elevated_surface_background: hex("#35463b"),
        editor_background: hex("#243029"),

        border: hex("#536657"),
        border_variant: hex("#35463c"),
        border_focused: hex("#b2cf76"),
        border_selected: hex("#90bed0"),
        border_disabled: hex("#35463c"),
        border_transparent: hex("#00000000"),

        element_background: hex("#34463b"),
        element_hover: hex("#394c3f"),
        element_active: hex("#3d5142"),
        element_selected: hex("#364e5c"),
        element_disabled: hex("#28362e"),
        ghost_element_background: hex("#00000000"),
        ghost_element_hover: hex("#2e4035"),
        ghost_element_active: hex("#3d5142"),
        ghost_element_selected: hex("#364e5c"),

        text: hex("#e1e9dd"),
        text_muted: hex("#b5c2b5"),
        text_placeholder: hex("#a3b09f"),
        text_disabled: hex("#778874"),
        text_accent: hex("#b2cf76"),

        icon: hex("#c8d5c3"),
        icon_muted: hex("#a3b49f"),
        icon_disabled: hex("#778874"),
        icon_accent: hex("#b2cf76"),

        title_bar_background: hex("#18211e"),
        toolbar_background: hex("#304037"),
        tab_bar_background: hex("#141d18"),
        tab_active_background: hex("#344535"),
        tab_inactive_background: hex("#202b25"),
        status_bar_background: hex("#18211e"),
        panel_background: hex("#202b25"),
        scrollbar_thumb_background: hex("#536657"),
        scrollbar_thumb_hover_background: hex("#829777"),

        hint_background: hex("#303e35"),

        // Status is not interchangeable with navigation: retain separate hues.
        vc_added: hex("#b2cf76"),
        vc_modified: hex("#e4b66e"),
        vc_deleted: hex("#ec9790"),
        vc_conflict: hex("#d8a4ca"),
        vc_renamed: hex("#90bed0"),
        vc_untracked: hex("#89c6b3"),
    }
}

pub fn forest_status() -> StatusColors {
    StatusColors {
        error: hex("#ec9790"),
        error_background: hex("#492d2d"),
        warning: hex("#e4b66e"),
        warning_background: hex("#453b26"),
        success: hex("#b2cf76"),
        success_background: hex("#34432b"),
        info: hex("#90bed0"),
        info_background: hex("#2c404b"),
        hint: hex("#b5c2b5"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contrast_ratio, hsla_to_hex};

    #[test]
    fn forest_body_and_metadata_have_readable_contrast_on_workbench_surfaces() {
        let c = forest_colors();
        for surface in [
            c.background,
            c.panel_background,
            c.editor_background,
            c.surface_background,
            c.elevated_surface_background,
            c.toolbar_background,
            c.tab_active_background,
            c.tab_inactive_background,
            c.element_background,
            c.element_hover,
            c.element_active,
            c.element_selected,
            c.ghost_element_hover,
        ] {
            for ink in [c.text, c.text_muted] {
                let ratio = contrast_ratio(ink, surface);
                assert!(
                    ratio >= 4.5,
                    "{} on {}: {ratio:.2}:1",
                    hsla_to_hex(ink),
                    hsla_to_hex(surface)
                );
            }
        }
        assert!(contrast_ratio(c.text_placeholder, c.editor_background) >= 4.5);
    }

    #[test]
    fn forest_focus_and_selection_boundaries_are_visible() {
        let c = forest_colors();
        for surface in [
            c.panel_background,
            c.editor_background,
            c.toolbar_background,
            c.tab_active_background,
            c.element_hover,
            c.element_selected,
        ] {
            assert!(contrast_ratio(c.border_focused, surface) >= 3.0);
            assert!(contrast_ratio(c.border_selected, surface) >= 3.0);
        }
    }

    #[test]
    fn forest_status_foregrounds_are_readable_on_their_surfaces() {
        let c = forest_colors();
        let s = forest_status();
        for (ink, surface) in [
            (s.error, s.error_background),
            (s.warning, s.warning_background),
            (s.success, s.success_background),
            (s.info, s.info_background),
        ] {
            assert!(contrast_ratio(ink, surface) >= 4.5);
        }
        for ink in [
            c.vc_added,
            c.vc_modified,
            c.vc_deleted,
            c.vc_conflict,
            c.vc_renamed,
            c.vc_untracked,
        ] {
            assert!(contrast_ratio(ink, c.editor_background) >= 4.5);
            assert!(contrast_ratio(ink, c.panel_background) >= 4.5);
        }
    }

    #[test]
    fn forest_regions_and_git_statuses_do_not_collapse_to_one_color() {
        let c = forest_colors();
        assert_ne!(c.panel_background, c.editor_background);
        assert_ne!(c.toolbar_background, c.tab_bar_background);
        assert_ne!(c.tab_active_background, c.tab_inactive_background);
        assert_ne!(c.element_selected, c.element_hover);
        assert_ne!(c.text_accent, c.border_selected);
        let statuses = [
            c.vc_added,
            c.vc_modified,
            c.vc_deleted,
            c.vc_conflict,
            c.vc_renamed,
            c.vc_untracked,
        ];
        for (i, status) in statuses.iter().enumerate() {
            assert!(!statuses[..i].contains(status));
        }
    }
}
