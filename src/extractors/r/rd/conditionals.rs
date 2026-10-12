use super::*;

impl Context<'_> {
    pub(super) fn select_conditionals(
        &mut self,
        nodes: &[RdNode],
        path: &RdPath,
        os_type: &str,
    ) -> Vec<RdNode> {
        let mut selected = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            let path = path.with_child(index);
            match node {
                RdNode::Tagged(tagged) if matches!(tagged.tag(), RdTag::If | RdTag::IfElse) => {
                    match node.inspect_conditional(&path) {
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
                                conditional.then_branch()
                            } else {
                                conditional.else_branch().unwrap_or_default()
                            };
                            selected.extend(self.select_conditionals(branch, &path, os_type));
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
                        selected.extend(self.select_conditionals(
                            body.unwrap().children(),
                            &path,
                            os_type,
                        ));
                    }
                }
                RdNode::Tagged(tagged) => selected.push(RdNode::tagged(
                    tagged.tag().clone(),
                    tagged.option().map(<[RdNode]>::to_vec),
                    self.select_conditionals(tagged.children(), &path, os_type),
                )),
                RdNode::Group(group) => selected.push(RdNode::group(self.select_conditionals(
                    group.children(),
                    &path,
                    os_type,
                ))),
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
