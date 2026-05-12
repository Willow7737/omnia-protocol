//! AI agent identity with capability-based access control
//!
//! AI agents are non-human entities with limited, revocable capabilities.
//! Each agent has its own DID, an owner DID (the human or organization that
//! controls it), and a set of typed capabilities that define what the agent
//! can do. Capabilities can be expired and revoked at any time.

use omnia_substrate::{CausalOrder, VectorClock};
use serde::{Deserialize, Serialize};

/// An AI agent identity linked to an owner DID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    /// The agent's own DID (e.g., "did:omnia:agent:abc123").
    pub did: String,
    /// The DID of the human or organization that owns this agent.
    pub owner_did: String,
    /// The set of capabilities granted to this agent.
    pub capabilities: Vec<AgentCapability>,
    /// Creation timestamp as a vector clock.
    pub created_at: VectorClock,
    /// Optional expiration — if set, the agent is inactive after this time.
    pub expires_at: Option<VectorClock>,
    /// Whether this agent's capabilities have been revoked.
    pub revoked: bool,
}

/// A typed capability granted to an AI agent.
///
/// Capabilities follow the principle of least privilege: each capability
/// is narrowly scoped to a specific domain with explicit limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentCapability {
    /// Can transfer up to `max_amount` per transaction in the given currency.
    FinancialTransfer {
        /// Maximum amount allowed per transfer.
        max_amount: u64,
        /// Currency identifier (e.g., "UBC", "ETH").
        currency: String,
    },
    /// Can process data in the specified domains.
    DataProcessing {
        /// Domains the agent is authorized to process (e.g., "health", "financial").
        domains: Vec<String>,
        /// Maximum number of records the agent can process.
        max_records: u64,
    },
    /// Can execute specific contract types.
    ContractExecution {
        /// Contract type identifiers the agent can execute.
        contract_types: Vec<String>,
    },
    /// Can submit computational proofs.
    ComputeProof {
        /// Maximum compute units the agent can consume.
        max_compute_units: u64,
    },
    /// Can vote in governance with a quadratic weight limit.
    GovernanceVote {
        /// Maximum quadratic voting weight.
        max_weight: u64,
    },
}

impl AgentIdentity {
    /// Check if the agent has a specific capability.
    ///
    /// The agent must not be revoked, and it must have at least one
    /// capability that covers (is a superset of) the required capability.
    pub fn has_capability(&self, required: &AgentCapability) -> bool {
        if self.revoked {
            return false;
        }
        self.capabilities.iter().any(|cap| cap.covers(required))
    }

    /// Revoke all capabilities for this agent.
    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    /// Check if the agent is active (not revoked and not expired).
    pub fn is_active(&self, current_time: &VectorClock) -> bool {
        if self.revoked {
            return false;
        }
        if let Some(ref expiry) = self.expires_at {
            // Agent is active if current_time is before or equal to expiry
            let order = current_time.compare(expiry);
            return order == CausalOrder::Before || order == CausalOrder::Equal;
        }
        true
    }
}

impl AgentCapability {
    /// Check if this capability covers (is a superset of) another capability.
    ///
    /// A capability covers another if it grants at least as much power in
    /// the same domain. For example, a FinancialTransfer with max_amount=1000
    /// covers a FinancialTransfer with max_amount=500 in the same currency.
    pub fn covers(&self, other: &AgentCapability) -> bool {
        use AgentCapability::*;
        match (self, other) {
            (
                FinancialTransfer {
                    max_amount: a,
                    currency: c1,
                },
                FinancialTransfer {
                    max_amount: b,
                    currency: c2,
                },
            ) => a >= b && c1 == c2,

            (
                DataProcessing {
                    domains: d1,
                    max_records: r1,
                },
                DataProcessing {
                    domains: d2,
                    max_records: r2,
                },
            ) => r1 >= r2 && d2.iter().all(|d| d1.contains(d)),

            (
                ContractExecution {
                    contract_types: t1,
                },
                ContractExecution {
                    contract_types: t2,
                },
            ) => t2.iter().all(|t| t1.contains(t)),

            (
                ComputeProof {
                    max_compute_units: a,
                },
                ComputeProof {
                    max_compute_units: b,
                },
            ) => a >= b,

            (
                GovernanceVote { max_weight: a },
                GovernanceVote { max_weight: b },
            ) => a >= b,

            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_covering() {
        let cap = AgentCapability::FinancialTransfer {
            max_amount: 1000,
            currency: "UBC".to_string(),
        };

        // Lower amount, same currency → covered
        assert!(cap.covers(&AgentCapability::FinancialTransfer {
            max_amount: 500,
            currency: "UBC".to_string(),
        }));

        // Higher amount, same currency → NOT covered
        assert!(!cap.covers(&AgentCapability::FinancialTransfer {
            max_amount: 2000,
            currency: "UBC".to_string(),
        }));

        // Same amount, different currency → NOT covered
        assert!(!cap.covers(&AgentCapability::FinancialTransfer {
            max_amount: 500,
            currency: "ETH".to_string(),
        }));
    }

    #[test]
    fn test_data_processing_capability() {
        let cap = AgentCapability::DataProcessing {
            domains: vec!["health".to_string(), "financial".to_string()],
            max_records: 1000,
        };

        // Subset of domains, fewer records → covered
        assert!(cap.covers(&AgentCapability::DataProcessing {
            domains: vec!["health".to_string()],
            max_records: 500,
        }));

        // Domain not in the list → NOT covered
        assert!(!cap.covers(&AgentCapability::DataProcessing {
            domains: vec!["legal".to_string()],
            max_records: 100,
        }));
    }

    #[test]
    fn test_agent_has_capability() {
        let agent = AgentIdentity {
            did: "did:omnia:agent1".to_string(),
            owner_did: "did:omnia:human1".to_string(),
            capabilities: vec![
                AgentCapability::FinancialTransfer {
                    max_amount: 1000,
                    currency: "UBC".to_string(),
                },
                AgentCapability::DataProcessing {
                    domains: vec!["health".to_string()],
                    max_records: 100,
                },
            ],
            created_at: VectorClock::new(),
            expires_at: None,
            revoked: false,
        };

        // Within limits → has capability
        assert!(agent.has_capability(&AgentCapability::FinancialTransfer {
            max_amount: 500,
            currency: "UBC".to_string(),
        }));

        // Exceeds limits → does not have capability
        assert!(!agent.has_capability(&AgentCapability::FinancialTransfer {
            max_amount: 2000,
            currency: "UBC".to_string(),
        }));

        // Wrong currency → does not have capability
        assert!(!agent.has_capability(&AgentCapability::FinancialTransfer {
            max_amount: 500,
            currency: "ETH".to_string(),
        }));
    }

    #[test]
    fn test_revoked_agent_has_no_capabilities() {
        let mut agent = AgentIdentity {
            did: "did:omnia:agent1".to_string(),
            owner_did: "did:omnia:human1".to_string(),
            capabilities: vec![AgentCapability::FinancialTransfer {
                max_amount: 1000,
                currency: "UBC".to_string(),
            }],
            created_at: VectorClock::new(),
            expires_at: None,
            revoked: false,
        };

        assert!(agent.has_capability(&AgentCapability::FinancialTransfer {
            max_amount: 500,
            currency: "UBC".to_string(),
        }));

        agent.revoke();

        assert!(!agent.has_capability(&AgentCapability::FinancialTransfer {
            max_amount: 500,
            currency: "UBC".to_string(),
        }));
    }

    #[test]
    fn test_agent_is_active() {
        let agent = AgentIdentity {
            did: "did:omnia:agent1".to_string(),
            owner_did: "did:omnia:human1".to_string(),
            capabilities: vec![],
            created_at: VectorClock::new(),
            expires_at: None,
            revoked: false,
        };

        // No expiry → always active
        assert!(agent.is_active(&VectorClock::new()));

        // Revoked → inactive
        let mut revoked_agent = agent.clone();
        revoked_agent.revoke();
        assert!(!revoked_agent.is_active(&VectorClock::new()));
    }

    #[test]
    fn test_contract_execution_capability() {
        let cap = AgentCapability::ContractExecution {
            contract_types: vec!["escrow".to_string(), "auction".to_string()],
        };

        // Subset of contract types → covered
        assert!(cap.covers(&AgentCapability::ContractExecution {
            contract_types: vec!["escrow".to_string()],
        }));

        // Contract type not in the list → NOT covered
        assert!(!cap.covers(&AgentCapability::ContractExecution {
            contract_types: vec!["swap".to_string()],
        }));
    }

    #[test]
    fn test_different_capability_types_never_cover() {
        let cap = AgentCapability::FinancialTransfer {
            max_amount: 1000,
            currency: "UBC".to_string(),
        };

        assert!(!cap.covers(&AgentCapability::DataProcessing {
            domains: vec!["health".to_string()],
            max_records: 100,
        }));
    }
}
