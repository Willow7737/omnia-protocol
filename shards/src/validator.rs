//! Domain validator trait
//!
//! The [`DomainValidator`] trait provides a uniform interface for validating
//! operations against shard state. Each shard has its own validator ZST
//! (zero-sized type) with domain-specific validation logic.
//!
//! The trait provides a default no-op implementation so that shards that
//! don't need pre-flight validation can use `DomainValidator<Op>` directly.
//! Shards with specific validation logic implement their own validators
//! that override the default.

use crate::shard::ShardError;

/// Trait for domain-specific operation validators.
///
/// Each shard domain (Financial, Identity, etc.) can implement this trait
/// to provide pre-flight validation that checks whether an operation would
/// succeed without actually mutating state.
///
/// The default implementation returns `Ok(())`, meaning no extra validation
/// is performed. This is useful for shards where the `apply` method itself
/// performs all necessary checks.
pub trait DomainValidator<O>: Send + Sync {
    /// Validate an operation against the current state.
    ///
    /// Returns `Ok(())` if the operation would succeed, or a `ShardError`
    /// explaining why it would fail.
    ///
    /// The default implementation performs no validation.
    fn validate_op(_op: &O) -> Result<(), ShardError> {
        Ok(())
    }
}

/// No-op validator for domains that don't need pre-flight checks.
///
/// Use this as `DomainValidator<MyOp>` when the domain's `apply` method
/// already performs all necessary validation internally.
pub struct NoopValidator<O>(std::marker::PhantomData<O>);

impl<O: Send + Sync> DomainValidator<O> for NoopValidator<O> {}
