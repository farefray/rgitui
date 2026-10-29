mod colors;
mod contrast;
mod forest;
pub mod json_theme;
mod theme;

pub use colors::*;
pub use contrast::{composite, contrast_ratio, contrast_text_on};
pub use forest::{forest_colors, forest_status};
pub use theme::*;

/// Embedded proportional font for interface chrome; code and hashes stay monospace.
pub const DEFAULT_UI_FONT_FAMILY: &str = "IBM Plex Sans";

/// Set the active theme by name. Convenience wrapper around ThemeState.
pub fn set_theme(name: &str, cx: &mut gpui::App) {
    use gpui::BorrowAppContext;
    cx.update_global::<ThemeState, _>(|state: &mut ThemeState, _cx| {
        state.set_theme_by_name(name);
    });
}

/// Return the names of all available built-in themes.
pub fn available_themes(cx: &gpui::App) -> Vec<String> {
    cx.global::<ThemeState>()
        .available_themes()
        .iter()
        .map(|t| t.name.clone())
        .collect()
}
