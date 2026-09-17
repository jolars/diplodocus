use super::*;

impl Builder<'_> {
    pub(super) fn declarations(
        &mut self,
        module: &ParsedModule,
        source: Option<&ParsedModule>,
        parent: &str,
        selected: &[ParsedDeclaration],
        implementation: &[ParsedDeclaration],
        in_class: bool,
    ) -> Vec<String> {
        let imports = bindings::imports(module);
        let source_imports = source.map(bindings::imports).unwrap_or_default();
        let mut groups: BTreeMap<&str, Vec<&ParsedDeclaration>> = BTreeMap::new();
        for declaration in selected {
            groups
                .entry(&declaration.name)
                .or_default()
                .push(declaration);
        }
        let mut result = vec![];
        for (name, group) in groups {
            if name == "__all__" {
                continue;
            }
            let originals: Vec<_> = implementation.iter().filter(|d| d.name == name).collect();
            let preferred = group[0];
            let original = originals
                .iter()
                .find(|d| d.docstring.is_some())
                .copied()
                .or_else(|| originals.first().copied());
            let qualified = format!("{parent}.{name}");
            if source.is_some_and(|source| source.kind != module.kind)
                && originals
                    .iter()
                    .filter(|d| {
                        !is_overload(d, &source_imports)
                            && !accessor(d, name, "setter")
                            && !accessor(d, name, "deleter")
                    })
                    .count()
                    > 1
            {
                self.diagnostic(
                    DiagnosticCode::PythonConflictingStub,
                    Severity::Error,
                    format!("A stub cannot reconcile multiple implementation definitions of {qualified}."),
                    &preferred.source,
                );
                continue;
            }
            if group.iter().any(|d| d.kind != preferred.kind)
                || originals.iter().any(|d| d.kind != preferred.kind)
            {
                self.diagnostic(
                    DiagnosticCode::PythonConflictingStub,
                    Severity::Error,
                    format!("Incompatible declaration kinds for {qualified}."),
                    &preferred.source,
                );
                continue;
            }
            if let Some(Signature::Value {
                annotation: None,
                value: Some(SignatureExpression::Name { name: target, .. }),
            }) = preferred.signature.as_ref().map(|s| &s.signature)
                && preferred.kind == DeclarationKind::Assignment
            {
                let target = qualify(target, parent, &imports);
                self.bindings
                    .entry(qualified.clone())
                    .or_default()
                    .push(Binding {
                        target,
                        kind: ItemAliasKind::PythonAssignment,
                        source: preferred.source.clone(),
                        explicit: true,
                    });
                continue;
            }
            let mut decorators = self.decorators(preferred, &imports);
            if let Some(original) = original {
                let source_decorators = self.decorators(original, &source_imports);
                if preferred.kind == DeclarationKind::Function
                    && (callable_kind(name, in_class, &decorators)
                        != callable_kind(name, in_class, &source_decorators)
                        || decorators
                            .iter()
                            .any(|d| d.semantics == PythonDecoratorSemantics::Property)
                            != source_decorators
                                .iter()
                                .any(|d| d.semantics == PythonDecoratorSemantics::Property))
                {
                    self.diagnostic(
                        DiagnosticCode::PythonConflictingStub,
                        Severity::Error,
                        format!("Source and stub disagree about callable binding for {qualified}."),
                        &preferred.source,
                    );
                    continue;
                }
                if decorators.is_empty() {
                    decorators = source_decorators;
                }
            }
            let property = group.iter().any(|d| {
                d.decorators.iter().any(|v| {
                    bindings::decorator_name(&v.expression, &imports).as_deref() == Some("property")
                })
            });
            let overload = group.iter().any(|d| is_overload(d, &imports));
            if group.len() > 1
                && !(preferred.kind == DeclarationKind::Function && (property || overload))
            {
                self.diagnostic(
                    DiagnosticCode::PythonDuplicateIdentity,
                    Severity::Error,
                    format!("Multiple definitions of {qualified} cannot establish one identity."),
                    &preferred.source,
                );
                continue;
            }
            let (kind, identity_kind, declaration) = match preferred.kind {
                DeclarationKind::Function if property => {
                    let setter = group.iter().any(|d| accessor(d, name, "setter"));
                    let deleter = group.iter().any(|d| accessor(d, name, "deleter"));
                    (
                        ItemKind::Field,
                        PythonIdentityKind::Property,
                        PythonDeclaration::Property {
                            has_setter: setter,
                            has_deleter: deleter,
                        },
                    )
                }
                DeclarationKind::Function => {
                    let binding = callable_kind(name, in_class, &decorators);
                    if originals.iter().any(|d| d.is_async != preferred.is_async) {
                        self.diagnostic(
                            DiagnosticCode::PythonConflictingStub,
                            Severity::Error,
                            format!(
                                "Source and stub disagree about async behavior for {qualified}."
                            ),
                            &preferred.source,
                        );
                        continue;
                    }
                    let role = match binding {
                        PythonCallableKind::Function => PythonIdentityKind::Function,
                        PythonCallableKind::Constructor => PythonIdentityKind::Constructor,
                        _ => PythonIdentityKind::Method,
                    };
                    (
                        if in_class {
                            ItemKind::Method
                        } else {
                            ItemKind::Function
                        },
                        role,
                        PythonDeclaration::Callable {
                            binding,
                            is_async: preferred.is_async,
                            role: PythonCallableRole::Family { overloads: vec![] },
                        },
                    )
                }
                DeclarationKind::Class => (
                    ItemKind::Class,
                    PythonIdentityKind::Class,
                    PythonDeclaration::Class {
                        bases: if preferred.bases.is_empty() {
                            original.map(|d| d.bases.clone()).unwrap_or_default()
                        } else {
                            preferred.bases.clone()
                        },
                        constructor: PythonConstructor::Unspecified,
                    },
                ),
                DeclarationKind::Assignment => {
                    if in_class {
                        (
                            ItemKind::Field,
                            PythonIdentityKind::Field,
                            PythonDeclaration::Field,
                        )
                    } else {
                        (
                            ItemKind::Constant,
                            PythonIdentityKind::Constant,
                            PythonDeclaration::Constant,
                        )
                    }
                }
                DeclarationKind::TypeAlias => {
                    let target = preferred
                        .signature
                        .as_ref()
                        .and_then(|s| match &s.signature {
                            Signature::Value { value, .. } => value.clone(),
                            _ => None,
                        });
                    let Some(target) = target else {
                        self.diagnostic(
                            DiagnosticCode::PythonUnsupportedSurface,
                            Severity::Error,
                            format!("Type alias {qualified} has no supported target."),
                            &preferred.source,
                        );
                        continue;
                    };
                    (
                        ItemKind::Type,
                        PythonIdentityKind::TypeAlias,
                        PythonDeclaration::TypeAlias { target },
                    )
                }
            };
            let Some(id) = self.identity(&qualified, identity_kind, &preferred.source) else {
                continue;
            };
            let mut item = new_item(
                kind,
                name,
                &qualified,
                declaration,
                original.map(|d| &d.source).unwrap_or(&preferred.source),
            );
            python_mut(&mut item).decorators = decorators;
            item.provenance = original
                .into_iter()
                .map(|d| provenance(&d.source))
                .chain(group.iter().map(|d| provenance(&d.source)))
                .collect();
            normalize_provenance(&mut item.provenance);
            let docstring = original
                .and_then(|d| d.docstring.clone())
                .or_else(|| preferred.docstring.clone());
            if let Some(docstring) = docstring {
                self.docstrings.insert(id.clone(), docstring);
            }
            if overload {
                let mut overload_ids = vec![];
                let implementation_count =
                    group.iter().filter(|d| !is_overload(d, &imports)).count();
                if implementation_count > 1 {
                    self.diagnostic(
                        DiagnosticCode::PythonDuplicateIdentity,
                        Severity::Error,
                        format!("Multiple overload implementations for {qualified}."),
                        &preferred.source,
                    );
                }
                for declaration in group.iter().filter(|d| is_overload(d, &imports)) {
                    let Some(signature) = &declaration.signature else {
                        continue;
                    };
                    let identity = match SemanticIdentity::python_overload(
                        &qualified,
                        identity_kind,
                        &signature.signature,
                    ) {
                        Ok(identity) => identity,
                        Err(_) => {
                            self.diagnostic(
                                DiagnosticCode::PythonInvalidIdentity,
                                Severity::Error,
                                format!(
                                    "Unsupported normalized overload signature for {qualified}."
                                ),
                                &declaration.source,
                            );
                            continue;
                        }
                    };
                    let reference = match self.registry.register(&identity) {
                        Ok(reference) => reference,
                        Err(_) => {
                            self.diagnostic(
                                DiagnosticCode::PythonDuplicateIdentity,
                                Severity::Error,
                                format!("Duplicate normalized overload signature for {qualified}."),
                                &declaration.source,
                            );
                            continue;
                        }
                    };
                    let mut member = item.clone();
                    member.source_location = Some(declaration.source.clone());
                    member.provenance = vec![provenance(&declaration.source)];
                    member.signatures = vec![signature.clone()];
                    python_mut(&mut member).decorators = self.decorators(declaration, &imports);
                    if let PythonDeclaration::Callable { role, .. } =
                        &mut python_mut(&mut member).declaration
                    {
                        *role = PythonCallableRole::Overload {
                            family: ItemReference {
                                package: self.package.into(),
                                item: id.clone(),
                            },
                        };
                    }
                    item.signatures.push(signature.clone());
                    self.items.insert(reference.item.clone(), member);
                    overload_ids.push(reference);
                }
                if let PythonDeclaration::Callable { role, .. } =
                    &mut python_mut(&mut item).declaration
                {
                    *role = PythonCallableRole::Family {
                        overloads: overload_ids,
                    };
                }
            } else if let Some(signature) = &preferred.signature {
                let mut signature = signature.clone();
                if let (Signature::Value { annotation, value }, Some(original)) = (
                    &mut signature.signature,
                    original.and_then(|d| d.signature.as_ref()),
                ) && let Signature::Value {
                    annotation: source_annotation,
                    value: source_value,
                } = &original.signature
                {
                    if annotation.is_none() {
                        *annotation = source_annotation.clone();
                    }
                    if value.is_none() {
                        *value = source_value.clone();
                        signature.sources.extend(original.sources.clone());
                    }
                }
                item.signatures.push(signature);
            }
            if preferred.kind == DeclarationKind::Class {
                item.children = self.declarations(
                    module,
                    source,
                    &qualified,
                    &preferred.members,
                    original.map(|d| d.members.as_slice()).unwrap_or_default(),
                    true,
                );
                let constructor = item
                    .children
                    .iter()
                    .find(|child| {
                        matches!(
                            &python(&self.items[*child]).declaration,
                            PythonDeclaration::Callable {
                                binding: PythonCallableKind::Constructor,
                                ..
                            }
                        )
                    })
                    .cloned();
                let dataclass = python(&item)
                    .decorators
                    .iter()
                    .find(|d| d.semantics == PythonDecoratorSemantics::Dataclass)
                    .cloned();
                let resolved_constructor = if let Some(decorator) = dataclass {
                    let (init, frozen) = dataclass_options(&decorator.expression);
                    PythonConstructor::Dataclass {
                        fields: item
                            .children
                            .iter()
                            .filter(|child| {
                                matches!(
                                    python(&self.items[*child]).declaration,
                                    PythonDeclaration::Field
                                )
                            })
                            .map(|id| ItemReference {
                                package: self.package.into(),
                                item: id.clone(),
                            })
                            .collect(),
                        init,
                        frozen,
                    }
                } else if let Some(item) = constructor {
                    PythonConstructor::Explicit {
                        item: ItemReference {
                            package: self.package.into(),
                            item,
                        },
                    }
                } else {
                    PythonConstructor::Unspecified
                };
                if let PythonDeclaration::Class { constructor, .. } =
                    &mut python_mut(&mut item).declaration
                {
                    *constructor = resolved_constructor;
                }
            }
            self.direct.entry(qualified).or_default().insert(id.clone());
            self.items.insert(id.clone(), item);
            result.push(id);
        }
        // Lexical order matters for dataclass fields and child presentation,
        // even though the enclosing item table uses canonical-key order.
        result.sort_by_key(|id| selected.iter().position(|d| d.name == self.items[id].name));
        result
    }

    fn decorators(
        &mut self,
        declaration: &ParsedDeclaration,
        imports: &BTreeMap<String, Vec<Binding>>,
    ) -> Vec<PythonDecorator> {
        declaration
            .decorators
            .iter()
            .map(|decorator| {
                let name =
                    bindings::decorator_name(&decorator.expression, imports).unwrap_or_default();
                let semantics = match name.as_str() {
                    "typing.overload" | "typing_extensions.overload" => {
                        PythonDecoratorSemantics::Overload
                    }
                    "property" => PythonDecoratorSemantics::Property,
                    "staticmethod" => PythonDecoratorSemantics::StaticMethod,
                    "classmethod" => PythonDecoratorSemantics::ClassMethod,
                    "dataclasses.dataclass" if valid_dataclass(&decorator.expression) => {
                        PythonDecoratorSemantics::Dataclass
                    }
                    name if name == format!("{}.setter", declaration.name) => {
                        PythonDecoratorSemantics::PropertySetter
                    }
                    name if name == format!("{}.deleter", declaration.name) => {
                        PythonDecoratorSemantics::PropertyDeleter
                    }
                    _ => PythonDecoratorSemantics::Unknown,
                };
                if semantics == PythonDecoratorSemantics::Unknown {
                    self.diagnostic(
                        DiagnosticCode::PythonUnsupportedSurface,
                        Severity::Error,
                        format!("Unsupported decorator semantics for {}.", declaration.name),
                        &decorator.source,
                    );
                }
                PythonDecorator {
                    expression: decorator.expression.clone(),
                    semantics,
                    sources: vec![evidence(&decorator.source, SourceRole::Definition)],
                }
            })
            .collect()
    }
}

fn qualify(name: &str, parent: &str, imports: &BTreeMap<String, Vec<Binding>>) -> String {
    let head = name.split('.').next().unwrap_or(name);
    if imports.contains_key(head) || !name.contains('.') {
        format!("{parent}.{name}")
    } else {
        name.into()
    }
}

fn is_overload(declaration: &ParsedDeclaration, imports: &BTreeMap<String, Vec<Binding>>) -> bool {
    declaration.decorators.iter().any(|d| {
        matches!(
            bindings::decorator_name(&d.expression, imports).as_deref(),
            Some("typing.overload" | "typing_extensions.overload")
        )
    })
}

fn accessor(declaration: &ParsedDeclaration, name: &str, role: &str) -> bool {
    declaration.decorators.iter().any(|d| matches!(&d.expression, SignatureExpression::Name { name: expression, .. } if expression == &format!("{name}.{role}")))
}

fn callable_kind(name: &str, in_class: bool, decorators: &[PythonDecorator]) -> PythonCallableKind {
    if !in_class {
        PythonCallableKind::Function
    } else if matches!(name, "__init__" | "__new__") {
        PythonCallableKind::Constructor
    } else if decorators
        .iter()
        .any(|d| d.semantics == PythonDecoratorSemantics::StaticMethod)
    {
        PythonCallableKind::StaticMethod
    } else if decorators
        .iter()
        .any(|d| d.semantics == PythonDecoratorSemantics::ClassMethod)
    {
        PythonCallableKind::ClassMethod
    } else {
        PythonCallableKind::InstanceMethod
    }
}

fn valid_dataclass(expression: &SignatureExpression) -> bool {
    match expression {
        SignatureExpression::Name { .. } => true,
        SignatureExpression::LanguageSpecific { name, children, .. } if name == "call" => children.iter().skip(1).all(|child| matches!(child, SignatureExpression::LanguageSpecific { name, children, .. } if matches!(name.as_str(), "keyword:init" | "keyword:frozen") && matches!(children.as_slice(), [SignatureExpression::Literal { text }] if matches!(text.as_str(), "True" | "False")))),
        _ => false,
    }
}

fn dataclass_options(expression: &SignatureExpression) -> (bool, bool) {
    let mut result = (true, false);
    if let SignatureExpression::LanguageSpecific { children, .. } = expression {
        for child in children {
            if let SignatureExpression::LanguageSpecific { name, children, .. } = child {
                let value = matches!(children.first(), Some(SignatureExpression::Literal { text }) if text == "True");
                match name.as_str() {
                    "keyword:init" => result.0 = value,
                    "keyword:frozen" => result.1 = value,
                    _ => {}
                }
            }
        }
    }
    result
}
