//! Build coherent documentation websites for polyglot software projects.

#![forbid(unsafe_code)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(missing_docs)]

pub mod assembly;
pub mod commands;
pub mod components;
pub mod configuration;
pub mod configuration_validation;
pub mod diagnostics;
pub mod documents;
pub mod execution;
pub mod extractors;
mod generated_storage;
pub mod ir;
pub mod paths;
pub mod provenance;
pub mod rendering;
pub mod reporting;
pub mod site;
mod site_assets;
pub mod snapshots;
pub mod validation;
