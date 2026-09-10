//! Durable content-addressed execution for relative interfaces.
//!
//! A store writes verified shard artifacts under their SHA-256 identifiers.
//! The coordinator folds one shard at a time while protecting the common
//! separator. Each fold is durable before the next one starts. An atomic
//! manifest publishes the final result. [`DurableInterfaceStore`] is local
//! and does not provide networking, authentication, or multi-writer
//! coordination.

mod compose;
mod fs;
mod manifest;
mod model;
mod store;
mod wire;

pub use model::{
    ArtifactId, DistributedInterfaceCommit, DistributedInterfaceError,
    DistributedInterfaceManifest, DistributedInterfaceWork, DurableInterfaceStore,
};

#[cfg(all(test, holos_repository_tests))]
mod tests;
