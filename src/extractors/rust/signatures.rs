//! Translate compiler types into portable syntax with linked identifiers.

use std::collections::BTreeMap;

use rustdoc_types as rd;

use crate::ir::{ItemReference, SignatureExpression};

pub(super) struct Signatures<'a> {
    pub references: &'a BTreeMap<rd::Id, ItemReference>,
    pub krate: &'a rd::Crate,
}

pub(super) fn literal(text: impl Into<String>) -> SignatureExpression {
    SignatureExpression::Literal { text: text.into() }
}

pub(super) fn name(text: impl Into<String>) -> SignatureExpression {
    SignatureExpression::Name {
        name: text.into(),
        target: None,
    }
}

pub(super) fn tokens(children: Vec<SignatureExpression>) -> SignatureExpression {
    SignatureExpression::LanguageSpecific {
        language: "rust".into(),
        name: "tokens".into(),
        children,
        source: None,
    }
}

fn separated(
    items: impl IntoIterator<Item = SignatureExpression>,
    separator: &str,
) -> Vec<SignatureExpression> {
    let mut result = Vec::new();
    for item in items {
        if !result.is_empty() {
            result.push(literal(separator));
        }
        result.push(item);
    }
    result
}

fn delimited(
    open: &str,
    items: impl IntoIterator<Item = SignatureExpression>,
    close: &str,
) -> SignatureExpression {
    let mut result = vec![literal(open)];
    result.extend(separated(items, ", "));
    result.push(literal(close));
    tokens(result)
}

/// Concatenate syntax only for a stable implementation discriminator.
pub(super) fn spelling(expression: &SignatureExpression) -> String {
    match expression {
        SignatureExpression::Name { name, .. } => name.clone(),
        SignatureExpression::Literal { text } => text.clone(),
        SignatureExpression::LanguageSpecific { children, .. }
        | SignatureExpression::Sequence { items: children } => {
            children.iter().map(spelling).collect()
        }
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => {
            format!(
                "{}<{}>",
                spelling(constructor),
                arguments
                    .iter()
                    .map(spelling)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

impl Signatures<'_> {
    pub fn path(&self, path: &rd::Path) -> SignatureExpression {
        let spelling = if path.path.starts_with("$crate::") {
            self.krate
                .paths
                .get(&path.id)
                .map_or_else(|| path.path.clone(), |summary| summary.path.join("::"))
        } else {
            path.path.clone()
        };
        let mut result = vec![SignatureExpression::Name {
            name: spelling,
            target: self.references.get(&path.id).cloned(),
        }];
        if let Some(args) = &path.args {
            result.push(self.args(args));
        }
        tokens(result)
    }

    pub fn type_(&self, type_: &rd::Type) -> SignatureExpression {
        match type_ {
            rd::Type::ResolvedPath(path) => self.path(path),
            rd::Type::Generic(value) | rd::Type::Primitive(value) => name(value),
            rd::Type::Tuple(types) => {
                let mut result = delimited("(", types.iter().map(|type_| self.type_(type_)), ")");
                if types.len() == 1
                    && let SignatureExpression::LanguageSpecific { children, .. } = &mut result
                {
                    children.insert(children.len() - 1, literal(","));
                }
                result
            }
            rd::Type::Slice(type_) => tokens(vec![literal("["), self.type_(type_), literal("]")]),
            rd::Type::Array { type_, len } => tokens(vec![
                literal("["),
                self.type_(type_),
                literal("; "),
                literal(len),
                literal("]"),
            ]),
            rd::Type::Pat {
                type_,
                __pat_unstable_do_not_use,
            } => tokens(vec![
                self.type_(type_),
                literal(" is "),
                literal(__pat_unstable_do_not_use),
            ]),
            rd::Type::BorrowedRef {
                lifetime,
                is_mutable,
                type_,
            } => {
                let mut result = vec![literal("&")];
                if let Some(lifetime) = lifetime {
                    result.extend([name(lifetime), literal(" ")]);
                }
                if *is_mutable {
                    result.push(literal("mut "));
                }
                result.push(self.pointee(type_));
                tokens(result)
            }
            rd::Type::RawPointer { is_mutable, type_ } => tokens(vec![
                literal(if *is_mutable { "*mut " } else { "*const " }),
                self.pointee(type_),
            ]),
            rd::Type::Infer => name("_"),
            rd::Type::ImplTrait(bounds) => tokens(vec![literal("impl "), self.bounds(bounds)]),
            rd::Type::DynTrait(value) => {
                let mut result = vec![literal("dyn ")];
                result.extend(separated(
                    value.traits.iter().map(|bound| {
                        tokens(vec![
                            self.for_params(&bound.generic_params),
                            self.path(&bound.trait_),
                        ])
                    }),
                    " + ",
                ));
                if let Some(lifetime) = &value.lifetime {
                    if !value.traits.is_empty() {
                        result.push(literal(" + "));
                    }
                    result.push(name(lifetime));
                }
                tokens(result)
            }
            rd::Type::QualifiedPath {
                name: value,
                args,
                self_type,
                trait_,
            } => {
                let mut result = Vec::new();
                if let Some(trait_) = trait_ {
                    result.extend([
                        literal("<"),
                        self.type_(self_type),
                        literal(" as "),
                        self.path(trait_),
                        literal(">"),
                    ]);
                } else {
                    result.push(self.type_(self_type));
                }
                result.extend([literal("::"), name(value)]);
                if let Some(args) = args {
                    result.push(self.args(args));
                }
                tokens(result)
            }
            rd::Type::FunctionPointer(pointer) => tokens(vec![
                self.for_params(&pointer.generic_params),
                self.header(&pointer.header),
                literal("fn"),
                self.function_parameters(&pointer.sig, false),
            ]),
        }
    }

    fn pointee(&self, type_: &rd::Type) -> SignatureExpression {
        let needs_parentheses = match type_ {
            rd::Type::DynTrait(value) => value.traits.len() > 1 || value.lifetime.is_some(),
            rd::Type::ImplTrait(bounds) => bounds.len() > 1,
            _ => false,
        };
        if needs_parentheses {
            tokens(vec![literal("("), self.type_(type_), literal(")")])
        } else {
            self.type_(type_)
        }
    }

    fn args(&self, args: &rd::GenericArgs) -> SignatureExpression {
        match args {
            rd::GenericArgs::AngleBracketed { args, constraints } => {
                if args.is_empty() && constraints.is_empty() {
                    return tokens(vec![]);
                }
                delimited(
                    "<",
                    args.iter()
                        .map(|arg| match arg {
                            rd::GenericArg::Lifetime(value) => name(value),
                            rd::GenericArg::Type(type_) => self.type_(type_),
                            rd::GenericArg::Const(value) => literal(&value.expr),
                            rd::GenericArg::Infer => name("_"),
                        })
                        .chain(constraints.iter().map(|constraint| {
                            let mut result = vec![name(&constraint.name)];
                            if let Some(args) = &constraint.args {
                                result.push(self.args(args));
                            }
                            match &constraint.binding {
                                rd::AssocItemConstraintKind::Equality(term) => {
                                    result.extend([literal(" = "), self.term(term)])
                                }
                                rd::AssocItemConstraintKind::Constraint(bounds) => {
                                    result.extend([literal(": "), self.bounds(bounds)])
                                }
                            }
                            tokens(result)
                        })),
                    ">",
                )
            }
            rd::GenericArgs::Parenthesized { inputs, output } => {
                let mut result = vec![delimited(
                    "(",
                    inputs.iter().map(|type_| self.type_(type_)),
                    ")",
                )];
                if let Some(output) = output {
                    result.extend([literal(" -> "), self.type_(output)]);
                }
                tokens(result)
            }
            rd::GenericArgs::ReturnTypeNotation => literal("(..)"),
        }
    }

    fn term(&self, term: &rd::Term) -> SignatureExpression {
        match term {
            rd::Term::Type(type_) => self.type_(type_),
            rd::Term::Constant(constant) => literal(&constant.expr),
        }
    }

    pub fn bounds(&self, bounds: &[rd::GenericBound]) -> SignatureExpression {
        tokens(separated(
            bounds.iter().map(|bound| match bound {
                rd::GenericBound::TraitBound {
                    trait_,
                    generic_params,
                    modifier,
                } => {
                    let modifier = match modifier {
                        rd::TraitBoundModifier::None => "",
                        rd::TraitBoundModifier::Maybe => "?",
                        rd::TraitBoundModifier::MaybeConst => "~const ",
                    };
                    tokens(vec![
                        self.for_params(generic_params),
                        literal(modifier),
                        self.path(trait_),
                    ])
                }
                rd::GenericBound::Outlives(value) => name(value),
                rd::GenericBound::Use(args) => delimited(
                    "use<",
                    args.iter().map(|arg| match arg {
                        rd::PreciseCapturingArg::Lifetime(value)
                        | rd::PreciseCapturingArg::Param(value) => name(value),
                    }),
                    ">",
                ),
            }),
            " + ",
        ))
    }

    fn param(&self, param: &rd::GenericParamDef) -> SignatureExpression {
        let mut result = vec![];
        if matches!(param.kind, rd::GenericParamDefKind::Const { .. }) {
            result.push(literal("const "));
        }
        result.push(name(&param.name));
        match &param.kind {
            rd::GenericParamDefKind::Lifetime { outlives } => {
                if !outlives.is_empty() {
                    result.push(literal(": "));
                    result.extend(separated(outlives.iter().map(name), " + "));
                }
            }
            rd::GenericParamDefKind::Type {
                bounds, default, ..
            } => {
                if !bounds.is_empty() {
                    result.extend([literal(": "), self.bounds(bounds)]);
                }
                if let Some(default) = default {
                    result.extend([literal(" = "), self.type_(default)]);
                }
            }
            rd::GenericParamDefKind::Const { type_, default } => {
                result.extend([literal(": "), self.type_(type_)]);
                if let Some(default) = default {
                    result.extend([literal(" = "), literal(default)]);
                }
            }
        }
        tokens(result)
    }

    pub fn params(&self, params: &[rd::GenericParamDef]) -> SignatureExpression {
        let params: Vec<_> = params
            .iter()
            .filter(|param| {
                !matches!(
                    &param.kind,
                    rd::GenericParamDefKind::Type {
                        is_synthetic: true,
                        ..
                    }
                )
            })
            .collect();
        if params.is_empty() {
            return tokens(vec![]);
        }
        delimited("<", params.into_iter().map(|param| self.param(param)), ">")
    }

    fn for_params(&self, params: &[rd::GenericParamDef]) -> SignatureExpression {
        if params.is_empty() {
            tokens(vec![])
        } else {
            tokens(vec![literal("for"), self.params(params), literal(" ")])
        }
    }

    pub fn where_(&self, generics: &rd::Generics) -> SignatureExpression {
        if generics.where_predicates.is_empty() {
            return tokens(vec![]);
        }
        let mut result = vec![literal(" where ")];
        result.extend(separated(
            generics
                .where_predicates
                .iter()
                .map(|predicate| match predicate {
                    rd::WherePredicate::BoundPredicate {
                        type_,
                        bounds,
                        generic_params,
                    } => tokens(vec![
                        self.for_params(generic_params),
                        self.type_(type_),
                        literal(": "),
                        self.bounds(bounds),
                    ]),
                    rd::WherePredicate::LifetimePredicate { lifetime, outlives } => {
                        let mut result = vec![name(lifetime), literal(": ")];
                        result.extend(separated(outlives.iter().map(name), " + "));
                        tokens(result)
                    }
                    rd::WherePredicate::EqPredicate { lhs, rhs } => {
                        tokens(vec![self.type_(lhs), literal(" = "), self.term(rhs)])
                    }
                }),
            ", ",
        ));
        tokens(result)
    }

    fn header(&self, header: &rd::FunctionHeader) -> SignatureExpression {
        let mut result = vec![];
        if header.is_const {
            result.push(literal("const "));
        }
        if header.is_async {
            result.push(literal("async "));
        }
        if header.is_unsafe {
            result.push(literal("unsafe "));
        }
        let abi = match &header.abi {
            rd::Abi::Rust => None,
            rd::Abi::C { unwind } => Some(if *unwind {
                "C-unwind".into()
            } else {
                "C".into()
            }),
            rd::Abi::Other(abi) => Some(abi.clone()),
            rd::Abi::Cdecl { unwind } => {
                Some(format!("cdecl{}", if *unwind { "-unwind" } else { "" }))
            }
            rd::Abi::Stdcall { unwind } => {
                Some(format!("stdcall{}", if *unwind { "-unwind" } else { "" }))
            }
            rd::Abi::Fastcall { unwind } => {
                Some(format!("fastcall{}", if *unwind { "-unwind" } else { "" }))
            }
            rd::Abi::Aapcs { unwind } => {
                Some(format!("aapcs{}", if *unwind { "-unwind" } else { "" }))
            }
            rd::Abi::Win64 { unwind } => {
                Some(format!("win64{}", if *unwind { "-unwind" } else { "" }))
            }
            rd::Abi::SysV64 { unwind } => {
                Some(format!("sysv64{}", if *unwind { "-unwind" } else { "" }))
            }
            rd::Abi::System { unwind } => {
                Some(format!("system{}", if *unwind { "-unwind" } else { "" }))
            }
        };
        if let Some(abi) = abi {
            result.push(literal(format!("extern {abi:?} ")));
        }
        tokens(result)
    }

    fn function_parameters(
        &self,
        sig: &rd::FunctionSignature,
        include_names: bool,
    ) -> SignatureExpression {
        let mut params: Vec<_> = sig
            .inputs
            .iter()
            .map(|(value, type_)| {
                if value == "self" {
                    match type_ {
                        rd::Type::Generic(value) if value == "Self" => return name("self"),
                        rd::Type::BorrowedRef {
                            lifetime,
                            is_mutable,
                            type_,
                        } if matches!(&**type_, rd::Type::Generic(value) if value == "Self") => {
                            let mut result = vec![literal("&")];
                            if let Some(lifetime) = lifetime {
                                result.extend([name(lifetime), literal(" ")]);
                            }
                            if *is_mutable {
                                result.push(literal("mut "));
                            }
                            result.push(name("self"));
                            return tokens(result);
                        }
                        _ => {}
                    }
                }
                if include_names {
                    tokens(vec![name(value), literal(": "), self.type_(type_)])
                } else {
                    self.type_(type_)
                }
            })
            .collect();
        if sig.is_c_variadic {
            params.push(literal("..."));
        }
        let mut result = vec![delimited("(", params, ")")];
        if let Some(output) = &sig.output {
            result.extend([literal(" -> "), self.type_(output)]);
        }
        tokens(result)
    }

    pub fn implementation(&self, implementation: &rd::Impl) -> SignatureExpression {
        let mut result = vec![];
        if implementation.is_unsafe {
            result.push(literal("unsafe "));
        }
        result.extend([
            literal("impl"),
            self.params(&implementation.generics.params),
            literal(" "),
        ]);
        if let Some(trait_) = &implementation.trait_ {
            if implementation.is_negative {
                result.push(literal("!"));
            }
            result.extend([self.path(trait_), literal(" for ")]);
        }
        // Rustdoc specializes `for_` to the type whose page lists a blanket
        // implementation. Its original receiver retains the bound parameters.
        let receiver = implementation
            .blanket_impl
            .as_ref()
            .unwrap_or(&implementation.for_);
        result.extend([self.type_(receiver), self.where_(&implementation.generics)]);
        tokens(result)
    }

    pub fn declaration(&self, item: &rd::Item) -> SignatureExpression {
        let item_name = item.name.as_deref().unwrap_or("_");
        let visibility = if !matches!(item.visibility, rd::Visibility::Public) {
            ""
        } else {
            "pub "
        };
        let mut result = vec![literal(visibility)];
        match &item.inner {
            rd::ItemEnum::Function(function) => result.extend([
                self.header(&function.header),
                literal("fn "),
                name(item_name),
                self.params(&function.generics.params),
                self.function_parameters(&function.sig, true),
                self.where_(&function.generics),
            ]),
            rd::ItemEnum::Struct(value) => {
                result.extend([
                    literal("struct "),
                    name(item_name),
                    self.params(&value.generics.params),
                ]);
                if let rd::StructKind::Tuple(fields) = &value.kind {
                    result.push(self.tuple_fields(fields));
                }
                result.push(self.where_(&value.generics));
                if matches!(value.kind, rd::StructKind::Unit) {
                    result.push(literal(";"));
                }
            }
            rd::ItemEnum::Enum(value) => result.extend([
                literal("enum "),
                name(item_name),
                self.params(&value.generics.params),
                self.where_(&value.generics),
            ]),
            rd::ItemEnum::Union(value) => result.extend([
                literal("union "),
                name(item_name),
                self.params(&value.generics.params),
                self.where_(&value.generics),
            ]),
            rd::ItemEnum::Trait(value) => {
                if value.is_unsafe {
                    result.push(literal("unsafe "));
                }
                if value.is_auto {
                    result.push(literal("auto "));
                }
                result.extend([
                    literal("trait "),
                    name(item_name),
                    self.params(&value.generics.params),
                ]);
                if !value.bounds.is_empty() {
                    result.extend([literal(": "), self.bounds(&value.bounds)]);
                }
                result.push(self.where_(&value.generics));
            }
            rd::ItemEnum::TraitAlias(value) => result.extend([
                literal("trait "),
                name(item_name),
                self.params(&value.generics.params),
                literal(" = "),
                self.bounds(&value.params),
                self.where_(&value.generics),
            ]),
            rd::ItemEnum::TypeAlias(value) => result.extend([
                literal("type "),
                name(item_name),
                self.params(&value.generics.params),
                literal(" = "),
                self.type_(&value.type_),
                self.where_(&value.generics),
                literal(";"),
            ]),
            rd::ItemEnum::StructField(type_) => {
                result.extend([name(item_name), literal(": "), self.type_(type_)])
            }
            rd::ItemEnum::Constant { type_, const_ } => result.extend([
                literal("const "),
                name(item_name),
                literal(": "),
                self.type_(type_),
                literal(" = "),
                literal(&const_.expr),
                literal(";"),
            ]),
            rd::ItemEnum::Static(value) => {
                if value.is_unsafe {
                    result.push(literal("unsafe "));
                }
                result.push(literal(if value.is_mutable {
                    "static mut "
                } else {
                    "static "
                }));
                result.extend([
                    name(item_name),
                    literal(": "),
                    self.type_(&value.type_),
                    literal(" = "),
                    literal(&value.expr),
                    literal(";"),
                ]);
            }
            rd::ItemEnum::AssocConst { type_, value, .. } => {
                result.extend([
                    literal("const "),
                    name(item_name),
                    literal(": "),
                    self.type_(type_),
                ]);
                if let Some(value) = value {
                    result.extend([literal(" = "), literal(value)]);
                }
                result.push(literal(";"));
            }
            rd::ItemEnum::AssocType {
                generics,
                bounds,
                type_,
                ..
            } => {
                result.extend([
                    literal("type "),
                    name(item_name),
                    self.params(&generics.params),
                ]);
                if !bounds.is_empty() {
                    result.extend([literal(": "), self.bounds(bounds)]);
                }
                if let Some(type_) = type_ {
                    result.extend([literal(" = "), self.type_(type_)]);
                }
                result.extend([self.where_(generics), literal(";")]);
            }
            rd::ItemEnum::Variant(value) => {
                result.push(name(item_name));
                match &value.kind {
                    rd::VariantKind::Tuple(fields) => result.push(self.tuple_fields(fields)),
                    rd::VariantKind::Struct { fields, .. } => result.push(delimited(
                        " { ",
                        fields.iter().filter_map(|id| {
                            let field = self.krate.index.get(id)?;
                            let rd::ItemEnum::StructField(type_) = &field.inner else {
                                return None;
                            };
                            Some(tokens(vec![
                                name(field.name.as_deref().unwrap_or("_")),
                                literal(": "),
                                self.type_(type_),
                            ]))
                        }),
                        " }",
                    )),
                    rd::VariantKind::Plain => {}
                }
                if let Some(discriminant) = &value.discriminant {
                    result.extend([literal(" = "), literal(&discriminant.expr)]);
                }
            }
            rd::ItemEnum::Module(_) => result.extend([literal("mod "), name(item_name)]),
            rd::ItemEnum::Macro(value) => result.push(literal(value)),
            rd::ItemEnum::ProcMacro(_) => result.extend([literal("macro "), name(item_name)]),
            rd::ItemEnum::Impl(implementation) => return self.implementation(implementation),
            _ => result.push(name(item_name)),
        }
        tokens(result)
    }

    fn tuple_fields(&self, fields: &[Option<rd::Id>]) -> SignatureExpression {
        delimited(
            "(",
            fields.iter().map(|field| {
                if let Some(id) = field
                    && let Some(rd::Item {
                        inner: rd::ItemEnum::StructField(type_),
                        ..
                    }) = self.krate.index.get(id)
                {
                    self.type_(type_)
                } else {
                    name("_")
                }
            }),
            ")",
        )
    }
}
