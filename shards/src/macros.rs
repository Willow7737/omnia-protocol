// The `impl_shard!` macro
//
// Generates the boilerplate for a domain shard: the struct definition,
// `new()`, `Default`, and the full `Shard` trait implementation.
//
// # Usage
//
// ```ignore
// impl_shard!(
//     /// The Financial shard.
//     FinancialShard,
//     FinancialState,
//     FinancialOp,
//     ShardOp::Financial,
//     FinancialValidator,
//     ShardId::financial()
// );
// ```
//
// # What it generates
//
// For each invocation, the macro generates:
//
// 1. A `pub struct $Name { state: $State }`
// 2. `fn new() -> Self`
// 3. `fn state(&self) -> &$State`
// 4. `impl Default`
// 5. `impl Shard` with:
//    - `shard_id()` returning the provided expression
//    - `process_event()` that extracts the op from `ShardOp`, validates,
//      and applies via [`crate::domain_state::ShardState`]
//    - `state_snapshot()` that calls [`crate::domain_state::ShardState::snapshot`]
//    - `validate()` that extracts the op and calls the validator
//
// # Extra methods
//
// If a shard needs methods beyond what the macro generates (e.g.,
// `EconomicsShard::state()` returning `&EconomicsState`), add them
// in a separate `impl` block after the macro invocation.

/// Generate a domain shard struct and its `Shard` trait implementation.
///
/// See the [module-level documentation](self) for details.
#[macro_export]
macro_rules! impl_shard {
    (
        $(#[$meta:meta])*
        $name:ident,
        $state:ty,
        $op:ty,
        $variant:path,
        $validator:ty,
        $shard_id:expr
    ) => {
        $(#[$meta])*
        pub struct $name {
            state: $state,
        }

        impl $name {
            /// Create a new shard with default state.
            pub fn new() -> Self {
                Self {
                    state: <$state>::new(),
                }
            }

            /// Get a reference to the internal state.
            pub fn state(&self) -> &$state {
                &self.state
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $crate::shard::Shard for $name {
            fn shard_id(&self) -> $crate::shard::ShardId {
                $shard_id
            }

            fn process_event(
                &mut self,
                event: &omnia_substrate::Event,
                op: $crate::payload::ShardOp,
            ) -> Result<(), $crate::shard::ShardError> {
                match op {
                    $variant(inner) => {
                        <$validator>::validate(&self.state, &inner)?;
                        $crate::domain_state::ShardState::apply_op(&mut self.state, &inner, event)
                    }
                    _ => Err($crate::shard::ShardError::TypeMismatch),
                }
            }

            fn state_snapshot(&self) -> Result<Vec<u8>, $crate::shard::ShardError> {
                $crate::domain_state::ShardState::snapshot(&self.state)
            }

            fn validate(&self, op: &$crate::payload::ShardOp) -> Result<(), $crate::shard::ShardError> {
                match op {
                    $variant(inner) => <$validator>::validate(&self.state, inner),
                    _ => Err($crate::shard::ShardError::TypeMismatch),
                }
            }
        }
    };
}
