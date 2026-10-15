use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "syntax-" };

pub(super) struct Highlighter {
    syntaxes: SyntaxSet,
}

impl Highlighter {
    pub(super) fn new() -> Self {
        Self {
            syntaxes: two_face::syntax::extra_newlines(),
        }
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
            "<pre class=\"syntax-code\"><code>{}</code></pre>",
            generator.finalize()
        )
    }
}

pub(super) fn stylesheet() -> Result<String, syntect::Error> {
    let themes = two_face::theme::extra();
    let mut theme = themes[two_face::theme::EmbeddedThemeName::Github].clone();
    // Keep source and output blocks on the site's shared background.
    theme.settings.background = None;
    css_for_theme_with_class_style(&theme, CLASS_STYLE)
}

pub(super) fn fragment_stylesheet() -> Result<String, syntect::Error> {
    let themes = two_face::theme::extra();
    let mut css = String::new();
    for (selector, name) in [
        (
            ".diplodocus-content",
            two_face::theme::EmbeddedThemeName::Github,
        ),
        (
            ".dark .diplodocus-content",
            two_face::theme::EmbeddedThemeName::OneHalfDark,
        ),
    ] {
        let mut theme = themes[name].clone();
        theme.settings.background = None;
        let rules = css_for_theme_with_class_style(&theme, CLASS_STYLE)?;
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
    Ok(css)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                html.starts_with("<pre class=\"syntax-code\">"),
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
