//! Canonical execution identity and local input observations, without a kernel or cache.

mod codec;
mod key;
mod launch;
mod local;
mod snapshot;

pub use codec::*;
pub use key::validate_key_input;
pub use launch::*;
pub use local::{BuildObservation, IdentityError, RepositoryFile};
pub use snapshot::*;

pub(crate) use snapshot::validate_prepared;
