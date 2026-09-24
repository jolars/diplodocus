//! Build coherent documentation websites for polyglot software projects.

#![forbid(unsafe_code)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(missing_docs)]

pub mod assembly;
pub mod commands;
pub mod configuration;
pub mod configuration_validation;
pub mod diagnostics;
pub mod documents;
pub mod execution;
pub mod extractors;
pub mod ir;
pub mod paths;
pub mod provenance;
pub mod rendering;
pub mod site;
pub mod snapshots;
pub mod validation;
