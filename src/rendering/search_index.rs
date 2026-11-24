//! Plain-text search sections from the rendered documentation body.

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Section {
    path: String,
    headings: Vec<String>,
    text: String,
}

pub(super) fn content(html: &str, path: &str) -> (String, Vec<Section>) {
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let mut index = Index {
        path,
        sections: Vec::new(),
        headings: Vec::new(),
        active: None,
        intro: String::new(),
    };
    index.visit(&dom.document, None);
    for section in &mut index.sections {
        section.text = normalize(&section.text);
    }
    (normalize(&index.intro), index.sections)
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Index<'a> {
    path: &'a str,
    sections: Vec<Section>,
    headings: Vec<(u8, String)>,
    active: Option<usize>,
    intro: String,
}

impl Index<'_> {
    fn push(&mut self, text: &str) {
        match self.active {
            Some(index) => self.sections[index].text.push_str(text),
            None => self.intro.push_str(text),
        }
    }

    fn visit(&mut self, node: &Handle, parent_anchor: Option<&str>) {
        let NodeData::Element { name, attrs, .. } = &node.data else {
            if let NodeData::Text { contents } = &node.data {
                self.push(&contents.borrow());
            }
            for child in node.children.borrow().iter() {
                self.visit(child, None);
            }
            return;
        };
        let tag = name.local.as_ref();
        let attrs = attrs.borrow();
        let attr = |name: &str| {
            attrs
                .iter()
                .find(|attr| &*attr.name.local == name)
                .map(|attr| attr.value.as_ref())
        };
        if matches!(
            tag,
            "script" | "style" | "template" | "button" | "svg" | "head"
        ) || attr("hidden").is_some()
            || attr("aria-hidden") == Some("true")
            || attr("class").is_some_and(|classes| {
                classes.split_whitespace().any(|class| {
                    matches!(
                        class,
                        "source-links" | "package" | "api-name" | "home-actions"
                    )
                })
            })
        {
            return;
        }
        if tag == "h1"
            && !attr("class").is_some_and(|classes| {
                classes
                    .split_whitespace()
                    .any(|class| class == "tabset-heading")
            })
        {
            return;
        }
        if let [b'h', level @ b'1'..=b'6'] = tag.as_bytes()
            && let Some(anchor) = attr("id").or(parent_anchor)
        {
            let mut title = String::new();
            heading_text(node, &mut title);
            let title = normalize(&title);
            self.headings.retain(|(ancestor, _)| ancestor < level);
            self.headings.push((*level, title));
            self.active = Some(self.sections.len());
            self.sections.push(Section {
                path: format!("{}#{}", self.path, crate::site::encode(anchor)),
                headings: self
                    .headings
                    .iter()
                    .map(|(_, title)| title.clone())
                    .collect(),
                text: String::new(),
            });
            return;
        }
        let block = matches!(
            tag,
            "p" | "div"
                | "section"
                | "li"
                | "pre"
                | "br"
                | "td"
                | "th"
                | "blockquote"
                | "figcaption"
                | "dt"
                | "dd"
        );
        if block {
            self.push(" ");
        }
        let scope = (tag == "section").then(|| (self.active, self.headings.clone()));
        for child in node.children.borrow().iter() {
            self.visit(child, if tag == "section" { attr("id") } else { None });
        }
        if let Some((active, headings)) = scope {
            self.active = active;
            self.headings = headings;
        }
        if block {
            self.push(" ");
        }
    }
}

fn heading_text(node: &Handle, output: &mut String) {
    match &node.data {
        NodeData::Text { contents } => output.push_str(&contents.borrow()),
        NodeData::Element { name, attrs, .. } if &*name.local == "img" => {
            if let Some(alt) = attrs
                .borrow()
                .iter()
                .find(|attr| &*attr.name.local == "alt")
            {
                output.push_str(&alt.value);
            }
        }
        _ => {}
    }
    for child in node.children.borrow().iter() {
        heading_text(child, output);
    }
}

#[cfg(test)]
mod tests {
    use super::content;

    #[test]
    fn indexes_title_level_tabs_and_image_headings() {
        let (intro, sections) = content(
            r#"<h1 id="guide">Guide</h1><p>Introduction.</p>
            <h2 id="setup">Setup</h2><p>Before the tabs.</p>
            <section class="tabset-panel">
              <h1 class="tabset-heading" id="linux">Linux</h1><p>Linux steps.</p>
            </section>
            <section class="tabset-panel">
              <h1 class="tabset-heading" id="macos">macOS</h1><p>macOS steps.</p>
              <h2 id="recovery"><img src="recovery.svg" alt="Recovery"></h2>
              <p>Reset instructions.</p>
            </section>"#,
            "guide.html",
        );
        assert_eq!(intro, "Introduction.");
        assert_eq!(sections.len(), 4);
        assert_eq!(sections[0].text, "Before the tabs.");
        assert_eq!(sections[2].path, "guide.html#macos");
        assert_eq!(sections[2].text, "macOS steps.");
        assert_eq!(sections[3].path, "guide.html#recovery");
        assert_eq!(sections[3].headings, ["macOS", "Recovery"]);
        assert_eq!(sections[3].text, "Reset instructions.");
    }
}
