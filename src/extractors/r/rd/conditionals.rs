use super::*;

impl Context<'_> {
    pub(super) fn prepare_nodes(&mut self, nodes: RdNodesRef<'_>, os_type: &str) -> Vec<RdNode> {
        let mut selected = Vec::new();
        for cursor in nodes {
            let node = cursor.node();
            match node {
                RdNode::Tagged(tagged) if matches!(tagged.tag(), RdTag::If | RdTag::IfElse) => {
                    match cursor.inspect_conditional() {
                        Ok(Some(conditional)) => {
                            let formats: Vec<_> =
                                conditional.format().split(',').map(str::trim).collect();
                            if formats.iter().any(|format| {
                                !matches!(
                                    *format,
                                    "html" | "latex" | "text" | "example" | "TRUE" | "FALSE"
                                )
                            }) {
                                selected.push(node.clone());
                                continue;
                            }
                            let matches = formats
                                .iter()
                                .any(|format| matches!(*format, "html" | "TRUE"));
                            let branch = if matches {
                                Some(conditional.then_branch_ref())
                            } else {
                                conditional.else_branch_ref()
                            };
                            if let Some(branch) = branch {
                                selected.extend(self.prepare_nodes(branch, os_type));
                            }
                        }
                        Err(error) => {
                            self.shape(error);
                            selected.push(node.clone());
                        }
                        _ => selected.push(node.clone()),
                    }
                }
                RdNode::Tagged(tagged) if matches!(tagged.tag(), RdTag::IfDef | RdTag::IfNDef) => {
                    let parts = tagged.children();
                    let format = parts
                        .first()
                        .and_then(RdNode::as_group)
                        .and_then(|group| plain(group.children()));
                    let body = parts.get(1).and_then(RdNode::as_group);
                    if tagged.option().is_some()
                        || parts.len() != 2
                        || body.is_none()
                        || !format
                            .as_ref()
                            .is_some_and(|format| matches!(format.trim(), "unix" | "windows"))
                    {
                        self.error(
                            DiagnosticCode::RRdInformationLoss,
                            "Rd platform conditional requires unix or windows and a body.",
                        );
                        selected.push(node.clone());
                        continue;
                    }
                    let matches = format.unwrap().trim() == os_type;
                    if matches == (tagged.tag() == &RdTag::IfDef) {
                        selected.extend(
                            self.prepare_nodes(
                                cursor.children().get(1).unwrap().children(),
                                os_type,
                            ),
                        );
                    }
                }
                RdNode::Tagged(tagged) => {
                    let mut children = self.prepare_nodes(cursor.children(), os_type);
                    if tagged.tag() == &RdTag::Tabular
                        && let Some(RdNode::Group(body)) = children.get_mut(1)
                    {
                        let mut nodes = body.children().to_vec();
                        let end = nodes.iter().rposition(|node| {
                            !matches!(node, RdNode::Text(text) if text.trim().is_empty())
                                && !matches!(node, RdNode::Comment(_))
                        });
                        // R ignores whitespace after the final row separator. Normalize it
                        // before creating the document whose cursors inspect the table.
                        if let Some(end) = end
                            && matches!(&nodes[end], RdNode::Tagged(tagged) if tagged.tag() == &RdTag::Cr)
                        {
                            nodes.truncate(end + 1);
                            children[1] = RdNode::group(nodes);
                        }
                    }
                    selected.push(RdNode::tagged(
                        tagged.tag().clone(),
                        tagged.option().map(<[RdNode]>::to_vec),
                        children,
                    ));
                }
                RdNode::Group(_) => selected.push(RdNode::group(
                    self.prepare_nodes(cursor.children(), os_type),
                )),
                _ => selected.push(node.clone()),
            }
        }
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse;
    use crate::diagnostics::Severity;
    use crate::ir::SourceLocation;

    #[test]
    fn platform_selection_precedes_topic_and_section_validation() {
        let text = "#ifdef unix\n\\name{unix_topic}\\alias{unix_topic}\n#endif\n#ifdef windows\n\\name{windows_topic}\\alias{windows_topic}\n#endif\n\\title{Platform topic}\n#ifndef windows\n\\description{Unix description.}\n#endif\n#ifndef unix\n\\description{Windows description.}\n#endif\n";
        let source = SourceLocation {
            repository: "r".into(),
            path: "man/platform.Rd".try_into().unwrap(),
            span: None,
        };
        for os_type in ["unix", "windows"] {
            let mut diagnostics = Vec::new();
            let topic = parse(text, &source, &mut diagnostics, os_type).unwrap();
            assert!(topic.valid);
            assert_eq!(topic.name, format!("{os_type}_topic"));
            assert!(
                !diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.severity == Severity::Error),
                "{diagnostics:?}"
            );
        }
    }
}
