//! Shared presentation of API members embedded in an owning page.

use crate::ir::{Item, ItemReference, RustImplementation};
use crate::validation::ResolvedDocument;

pub(crate) struct ApiPage<'a> {
    pub sections: Vec<ApiSection<'a>>,
}

pub(crate) struct ApiSection<'a> {
    pub title: String,
    pub anchor: String,
    pub implementation: Option<&'a RustImplementation>,
    pub documentation: Option<ApiMember<'a>>,
    pub members: Vec<ApiMember<'a>>,
}

pub(crate) struct ApiMember<'a> {
    pub reference: ItemReference,
    pub item: &'a Item,
    pub anchor: String,
    pub resolved: Option<&'a ResolvedDocument>,
    pub children: Vec<ApiMember<'a>>,
}

impl ApiPage<'_> {
    pub fn members(&self) -> impl Iterator<Item = &ApiMember<'_>> {
        self.sections
            .iter()
            .flat_map(|section| section.documentation.iter().chain(&section.members))
            .flat_map(|member| std::iter::once(member).chain(&member.children))
    }
}
