//! Filename-based syntax lookup for diff snapshots, with no filesystem I/O.
//!
//! Syntect 5.3's defaults already include Go. `find_syntax_for_file` can open
//! files to inspect shebangs; diff paths may refer to deleted files or another
//! revision and must never be read synchronously on the UI thread.

use syntect::parsing::{SyntaxReference, SyntaxSet};

/// Look up native grammars before the viewer's existing fallback aliases.
/// Accept both Git-style and Windows separators on every host platform.
pub(crate) fn syntax_for_path<'a>(set: &'a SyntaxSet, path: &str) -> Option<&'a SyntaxReference> {
    let filename = path.rsplit(['/', '\\']).next()?;
    set.find_syntax_by_extension(filename)
        .or_else(|| {
            let (stem, extension) = filename.rsplit_once('.')?;
            if stem.is_empty() {
                return None;
            }
            set.find_syntax_by_extension(extension)
        })
        .or_else(|| set.find_syntax_by_token(filename))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;
    use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, Style, ThemeSet};
    use syntect::parsing::{ParseState, ScopeStack};

    fn syntaxes() -> &'static SyntaxSet {
        static SET: OnceLock<SyntaxSet> = OnceLock::new();
        SET.get_or_init(SyntaxSet::load_defaults_newlines)
    }

    #[test]
    fn defaults_include_go_for_snapshot_paths_without_reading_files() {
        let set = syntaxes();
        assert_eq!(set.find_syntax_by_extension("go").unwrap().name, "Go");
        for path in [
            "main.go",
            "deleted/not-on-disk/main.go",
            "F:\\not-on-disk\\main.GO",
            "sources/代码.go",
            "Go",
        ] {
            assert_eq!(syntax_for_path(set, path).unwrap().name, "Go", "{path}");
        }
        assert_eq!(syntax_for_path(set, "main.rs").unwrap().name, "Rust");
        assert!(syntax_for_path(set, "unknown.rgitui-no-such-syntax").is_none());
        assert!(syntax_for_path(set, "").is_none());
    }

    #[test]
    fn full_filenames_take_precedence_over_suffixes() {
        let set = syntaxes();
        // These are filename entries in the bundled grammars, not extensions.
        for filename in ["Makefile", ".bashrc"] {
            let expected = set.find_syntax_by_extension(filename).unwrap();
            assert_eq!(syntax_for_path(set, filename).unwrap().name, expected.name);
        }
    }

    fn style_at(ranges: &[(Style, &str)], offset: usize) -> Style {
        let mut start = 0;
        for (style, text) in ranges {
            if offset < start + text.len() {
                return *style;
            }
            start += text.len();
        }
        panic!("no highlighted range at {offset}");
    }

    #[test]
    fn go_keywords_are_colored_with_the_viewers_fresh_per_line_parser() {
        let set = syntaxes();
        let syntax = syntax_for_path(set, "main.go").unwrap();
        let themes = ThemeSet::load_defaults();
        for theme in ["base16-ocean.dark", "base16-ocean.light"] {
            let highlighter = Highlighter::new(&themes.themes[theme]);
            // Match DiffViewer::highlight_text: no trailing newline, fresh
            // ParseState/HighlightState for every interleaved diff line.
            for line in [
                "\tif value == 42 { println(\"hello\") } // comment",
                "\treturn value",
                "func main() {",
                "package main",
                "import \"fmt\"",
                "\tvar value int",
            ] {
                let mut parser = ParseState::new(syntax);
                let operations = parser.parse_line(line, set).unwrap();
                let mut state = HighlightState::new(&highlighter, ScopeStack::new());
                let ranges: Vec<_> =
                    HighlightIterator::new(&mut state, &operations, line, &highlighter).collect();
                assert_eq!(
                    ranges.iter().map(|(_, text)| *text).collect::<String>(),
                    line
                );
                let keyword_offset = line.len() - line.trim_start().len();
                let keyword = style_at(&ranges, keyword_offset).foreground;
                let plain = themes.themes[theme].settings.foreground.unwrap();
                assert_ne!(keyword, plain, "{theme}: {line}");
                if line.contains("// comment") {
                    for token in ["42", "hello", "comment"] {
                        assert_ne!(
                            style_at(&ranges, line.find(token).unwrap()).foreground,
                            plain,
                            "{theme}: {token}"
                        );
                    }
                }
            }
        }
    }
}
