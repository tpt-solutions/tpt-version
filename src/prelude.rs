//! The types most programs need, in one import:
//!
//! ```
//! use tpt_version::prelude::*;
//! ```

pub use crate::Entity;
pub use crate::entity::UirValue;
pub use crate::error::{ObjectError, RepoError, StoreError};
pub use crate::merge::Resolution;
pub use crate::object::{ActorId, ContentHash, EntityId};
pub use crate::repo::{Clock, LogicalClock, Repository};
pub use crate::store::{CasStore, MemoryStore, RefStore};
pub use crate::uir::{Scalar, UirNode};
