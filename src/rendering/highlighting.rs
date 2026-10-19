use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "syntax-" };
const LIGHT_STYLE: &str = include_str!("assets/highlighting/github-light.css");
const DARK_STYLE: &str = include_str!("assets/highlighting/github-dark.css");

pub(super) struct Highlighter {
    syntaxes: SyntaxSet,
}

impl Highlighter {
    pub(super) fn new() -> Self {
        Self {
            syntaxes: two_face::syntax::extra_newlines(),
        }
    }

    pub(super) fn render_signature(&self, source: &str, language: Option<&str>) -> String {
        let source = super::signatures::layout_source(source, language.unwrap_or_default());
        self.render(&source, language)
            .replacen("<pre ", "<pre data-signature ", 1)
    }

    pub(super) fn render(&self, source: &str, language: Option<&str>) -> String {
        let syntax = language
            .and_then(|language| self.syntaxes.find_syntax_by_token(language.trim()))
            .filter(|syntax| syntax.name != "Plain Text");
        let Some(syntax) = syntax else {
            return super::render_preformatted_text(source);
        };
        let mut generator =
            ClassedHTMLGenerator::new_with_class_style(syntax, &self.syntaxes, CLASS_STYLE);
        for line in LinesWithEndings::from(source) {
            if generator
                .parse_html_for_line_which_includes_newline(line)
                .is_err()
            {
                return super::render_preformatted_text(source);
            }
        }
        format!(
            "<pre class=\"syntax-code\" tabindex=\"0\"><code>{}</code></pre>",
            generator.finalize()
        )
    }
}

pub(super) fn stylesheet() -> &'static str {
    LIGHT_STYLE
}

pub(super) fn fragment_stylesheet() -> String {
    let mut css = String::new();
    for (selector, rules) in [
        (".diplodocus-content", LIGHT_STYLE),
        (".dark .diplodocus-content", DARK_STYLE),
    ] {
        // Scope every selector, including comma-separated syntax alternatives,
        // because a host may retain this stylesheet across client navigation.
        for rule in rules.split_inclusive('}') {
            if let Some((selectors, body)) = rule.split_once('{') {
                css.push_str(
                    &selectors
                        .split(',')
                        .map(|part| format!("{selector} {}", part.trim()))
                        .collect::<Vec<_>>()
                        .join(",\n"),
                );
                css.push_str(" {");
                css.push_str(body);
                css.push('\n');
            }
        }
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_themes_meet_normal_text_contrast_with_a_light_default() {
        fn color(css: &str, property: &str) -> Option<syntect::highlighting::Color> {
            let prefix = format!("{property}: #");
            css.lines()
                .find_map(|line| line.trim().strip_prefix(&prefix))
                .map(|value| {
                    let value = value.strip_suffix(';').unwrap();
                    assert_eq!(value.len(), 6, "Expected opaque CSS colors: {value}");
                    let rgb = u32::from_str_radix(value, 16).unwrap();
                    syntect::highlighting::Color {
                        r: (rgb >> 16) as u8,
                        g: (rgb >> 8) as u8,
                        b: rgb as u8,
                        a: 255,
                    }
                })
        }

        fn luminance(color: syntect::highlighting::Color) -> f64 {
            let channel = |value: u8| {
                let value = f64::from(value) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
        }

        for (name, css) in [("light", stylesheet()), ("dark", DARK_STYLE)] {
            let background = color(css, "background-color").unwrap();
            let foreground = color(css, "color").unwrap();
            if name == "light" {
                let background = luminance(background);
                assert!(background > 0.5, "Code blocks should be light by default");
            }
            for rule in css.split('}') {
                let Some((selectors, body)) = rule.split_once('{') else {
                    continue;
                };
                // Diff tokens pair their own foreground and background colors.
                let foreground = luminance(color(body, "color").unwrap_or(foreground));
                let background = luminance(color(body, "background-color").unwrap_or(background));
                let contrast =
                    (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05);
                assert!(contrast >= 4.5, "{name} {selectors}: {contrast}:1");
            }
        }
    }

    fn code_text(html: &str) -> String {
        let document = roxmltree::Document::parse(html).unwrap();
        let code = document
            .descendants()
            .find(|node| node.has_tag_name("code"))
            .unwrap();
        code.descendants()
            .filter(|node| node.is_text())
            .map(|node| node.text().unwrap())
            .collect()
    }

    #[test]
    fn bundled_languages_and_aliases_produce_highlighted_escaped_source() {
        let highlighter = Highlighter::new();
        for (language, source) in [
            (
                "python",
                "def greet():\n    return '<script>& café</script>'\n",
            ),
            ("py", "x = 42"),
            ("R", "x <- function() 42\n"),
            ("julia", "function greet()\n    return \"café\"\nend"),
            ("jl", "x = 42\n"),
            ("rust", "fn main() { println!(\"hello\"); }"),
            ("rs", "let x = 42;"),
            ("toml", "[project]\nname = 'Example'\n"),
            ("nix", "{ enable = true; }"),
        ] {
            let html = highlighter.render(source, Some(language));
            assert!(
                html.starts_with("<pre class=\"syntax-code\" tabindex=\"0\">"),
                "{language}"
            );
            assert!(html.contains("<span class=\"syntax-"), "{language}");
            assert!(!html.contains("<script>"));
            assert_eq!(code_text(&html), source, "{language}");
        }
    }

    #[test]
    fn highlighting_preserves_whitespace_entities_and_final_newlines() {
        let highlighter = Highlighter::new();
        for source in [
            "",
            "\n",
            "\n\n",
            "\n\t  x = '<&>\\\" café &lt;b&gt;'\n\n",
            "x = 1",
            "x = 1\n",
            "x = 1\r\n\t# comment\r\n",
        ] {
            let html = highlighter.render(source, Some("python"));
            // XML normalizes CRLF, so compare against its equivalent text here.
            assert_eq!(code_text(&html), source.replace("\r\n", "\n"));
            if source.contains("\r\n") {
                assert_eq!(html.matches("\r\n").count(), 2);
            }
        }
    }

    #[test]
    fn multiline_strings_keep_their_scope_and_blocks_start_fresh() {
        let highlighter = Highlighter::new();
        let source = "message = \"\"\"first\nsecond\n\"\"\"\nx = 1";
        let html = highlighter.render(source, Some("python"));
        let document = roxmltree::Document::parse(&html).unwrap();
        let second_line = document
            .descendants()
            .find(|node| node.is_text() && node.text().unwrap().contains("second"))
            .unwrap();
        assert!(second_line.ancestors().any(|node| {
            node.attribute("class")
                .is_some_and(|classes| classes.split_whitespace().any(|c| c == "syntax-string"))
        }));
        highlighter.render("\"\"\"unfinished", Some("python"));
        assert_eq!(
            highlighter.render("x = 1", Some("python")),
            Highlighter::new().render("x = 1", Some("python"))
        );
    }

    #[test]
    fn absent_plain_and_unknown_languages_use_literal_rendering() {
        let highlighter = Highlighter::new();
        let source = "\n<script>& 'unsafe'</script>\n";
        for language in [None, Some(""), Some("text"), Some("txt"), Some("unknown")] {
            assert_eq!(
                highlighter.render(source, language),
                super::super::render_preformatted_text(source)
            );
        }
    }
}
