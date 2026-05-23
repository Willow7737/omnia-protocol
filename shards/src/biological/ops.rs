//! Biological shard operations
//!
//! Defines operations for managing consent, accessing health records,
//! and performing zero-knowledge queries.

use serde::{Deserialize, Serialize};

/// Subject identifier (the person whose data is being managed).
pub type SubjectId = [u8; 32];

/// Data consumer identifier (e.g., a hospital, research institution).
pub type ConsumerId = [u8; 32];

/// Operations supported by the Biological shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BiologicalOp {
    /// Grant a data consumer access to the subject's data.
    GrantAccess {
        /// The subject granting access.
        subject: SubjectId,
        /// The consumer receiving access.
        consumer: ConsumerId,
        /// Scope of access (e.g., "lab-results", "genomics").
        scope: String,
        /// Expiration timestamp (epoch millis, 0 = no expiry).
        expires_at: u64,
    },
    /// Revoke a previously granted access.
    RevokeAccess {
        /// The subject revoking access.
        subject: SubjectId,
        /// The consumer whose access is being revoked.
        consumer: ConsumerId,
    },
    /// Query data using a zero-knowledge proof.
    QueryWithZkProof {
        /// The subject whose data is being queried.
        subject: SubjectId,
        /// The consumer performing the query.
        consumer: ConsumerId,
        /// The ZK proof authorizing this query.
        zk_proof: Vec<u8>,
        /// The query description (what is being verified).
        query: String,
    },
}
