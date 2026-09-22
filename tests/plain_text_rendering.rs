use diplodocus::rendering::render_preformatted_text;

#[test]
fn preformatted_output_escapes_markup_and_preserves_markdown_and_whitespace() {
    let text =
        "\n  # Heading\n\t<script a=\"&\">'unsafe'</script>\n```{python}\nraise Exception()\n```\n";
    assert_eq!(
        render_preformatted_text(text),
        "<pre><code>\n  # Heading\n\t&lt;script a=&quot;&amp;&quot;&gt;&#39;unsafe&#39;&lt;/script&gt;\n```{python}\nraise Exception()\n```\n</code></pre>"
    );
}

#[test]
fn preformatted_output_escapes_entities_once_and_keeps_unicode() {
    assert_eq!(
        render_preformatted_text("&lt;b&gt; café"),
        "<pre><code>&amp;lt;b&amp;gt; café</code></pre>"
    );
    assert_eq!(render_preformatted_text(""), "<pre><code></code></pre>");
}
