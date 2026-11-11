//! Canonical execution identity and local input observations, without a kernel or cache.

mod codec;
mod key;
mod launch;
mod local;
mod snapshot;
mod source;

pub use codec::*;
pub(crate) use key::key_digest;
pub use key::validate_key_input;
pub use launch::*;
pub use local::{BuildObservation, IdentityError, RepositoryFile};
pub use snapshot::*;

pub(crate) use snapshot::validate_prepared;
pub(crate) use source::SourceSnapshot;
