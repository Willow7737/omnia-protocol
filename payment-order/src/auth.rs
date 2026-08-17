//! Authenticated caller derivation — Audit Priority 3
//!
//! Per the Omnia Checkpoint Audit:
//! "Roles such as provider, treasury, refund service, and chain service
//!  must be derived from authenticated service credentials and server-side
//!  policy. A request body must never be allowed to declare itself
//!  treasury or provider."
//!
//! This module provides the `CallerResolver` which maps authenticated
//! credentials (JWT claims, API keys, mTLS certificates) to the
//! `Caller` enum used by `PaymentEngine`. The client never chooses
//! its own role.

use std::collections::HashSet;

use crate::engine::Caller;
use crate::PaymentError;

/// Credential types that can be presented by callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// A JWT token with claims.
    Jwt {
        /// Authenticated subject identifier.
        subject: String,
        /// Parsed claims used for expiry and scope checks.
        claims: JwtClaims,
    },
    /// An API key (service-to-service).
    ApiKey {
        /// Stable key identifier.
        key_id: String,
        /// Service name bound to the key.
        service_name: String,
    },
    /// An mTLS client certificate.
    Mtls {
        /// Certificate common name.
        common_name: String,
    },
}

/// Extracted JWT claims relevant to authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JwtClaims {
    /// The subject (who the token represents).
    pub sub: String,
    /// The scope or role claim.
    pub scope: Option<String>,
    /// The issuer (who issued the token).
    pub iss: Option<String>,
    /// Expiration timestamp (ms since epoch).
    pub exp: u64,
    /// Issued-at timestamp (ms since epoch).
    pub iat: u64,
}

/// Service role configuration. Maps credential identities to
/// authorized service roles.
#[derive(Debug, Clone, Default)]
pub struct ServiceRoleRegistry {
    /// Map from API key ID to service name.
    api_key_services: HashSet<(String, String)>,
    /// Map from mTLS common name to service name.
    mtls_services: HashSet<(String, String)>,
    /// Set of public keys authorized as mint authority.
    mint_authority_keys: HashSet<String>,
    /// Map from provider ID to the set of public keys authorized
    /// to issue callbacks for that provider.
    provider_callback_keys: std::collections::HashMap<String, HashSet<String>>,
    /// Set of public keys authorized as manual reviewers.
    reviewer_keys: HashSet<String>,
}

impl ServiceRoleRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an API key for a specific service role.
    pub fn register_api_key(&mut self, key_id: String, service_name: String) {
        self.api_key_services.insert((key_id, service_name));
    }

    /// Register an mTLS certificate for a specific service role.
    pub fn register_mtls(&mut self, common_name: String, service_name: String) {
        self.mtls_services.insert((common_name, service_name));
    }

    /// Register a public key as mint authority.
    pub fn register_mint_authority(&mut self, public_key: String) {
        self.mint_authority_keys.insert(public_key);
    }

    /// Register a public key as authorized to issue callbacks for a provider.
    pub fn register_provider_callback_key(&mut self, provider_id: String, public_key: String) {
        self.provider_callback_keys
            .entry(provider_id)
            .or_default()
            .insert(public_key);
    }

    /// Register a public key as a manual reviewer.
    pub fn register_reviewer(&mut self, public_key: String) {
        self.reviewer_keys.insert(public_key);
    }

    /// Resolve a credential to a `Caller`.
    /// Returns `Err` if the credential is not recognized or expired.
    pub fn resolve(&self, credential: &Credential, now_ms: u64) -> Result<Caller, PaymentError> {
        match credential {
            Credential::ApiKey { key_id, service_name } => {
                if self.api_key_services.contains(&(key_id.clone(), service_name.clone())) {
                    Ok(Caller::System {
                        service: service_name.clone(),
                    })
                } else {
                    Err(PaymentError::Unauthorized {
                        actor: format!("apikey:{key_id}"),
                        state: crate::state::PaymentState::Created, // placeholder
                        required: "registered service".into(),
                    })
                }
            }
            Credential::Mtls { common_name } => {
                // Find the service for this CN
                for (cn, service) in &self.mtls_services {
                    if cn == common_name {
                        return Ok(Caller::System {
                            service: service.clone(),
                        });
                    }
                }
                Err(PaymentError::Unauthorized {
                    actor: format!("mtls:{common_name}"),
                    state: crate::state::PaymentState::Created,
                    required: "registered mtls service".into(),
                })
            }
            Credential::Jwt { claims, .. } => {
                // Check expiration
                if now_ms >= claims.exp {
                    return Err(PaymentError::QuoteExpired {
                        expiry_ms: claims.exp,
                        now_ms,
                    });
                }

                // Check if the subject matches a known service via scope
                if let Some(scope) = &claims.scope {
                    // scope format: "service:quote-service" or "provider:MTN:authenticated"
                    let parts: Vec<&str> = scope.split(':').collect();
                    match parts.first() {
                        Some(&"service") if parts.len() >= 2 => Ok(Caller::System {
                            service: parts[1].to_string(),
                        }),
                        Some(&"provider") if parts.len() >= 3 => {
                            let provider_id = parts[1].to_string();
                            let authenticated = parts[2] == "authenticated";
                            Ok(Caller::Provider {
                                provider_id,
                                authenticated,
                            })
                        }
                        Some(&"treasury") => Ok(Caller::Treasury),
                        Some(&"reviewer") => Ok(Caller::ManualReview {
                            reviewer: claims.sub.clone(),
                        }),
                        Some(&"sender") => Ok(Caller::Sender),
                        _ => Err(PaymentError::Unauthorized {
                            actor: format!("jwt:{}", claims.sub),
                            state: crate::state::PaymentState::Created,
                            required: "recognized scope".into(),
                        }),
                    }
                } else {
                    // No scope — treat as sender
                    Ok(Caller::Sender)
                }
            }
        }
    }

    /// Check if a public key is authorized for provider callbacks.
    pub fn is_authorized_provider_callback(&self, provider_id: &str, public_key: &str) -> bool {
        self.provider_callback_keys
            .get(provider_id)
            .is_some_and(|keys| keys.contains(public_key))
    }

    /// Check if a public key is a mint authority.
    pub fn is_mint_authority(&self, public_key: &str) -> bool {
        self.mint_authority_keys.contains(public_key)
    }

    /// Check if a public key is a reviewer.
    pub fn is_reviewer(&self, public_key: &str) -> bool {
        self.reviewer_keys.contains(public_key)
    }
}

/// Resolves a provider callback credential to an authenticated `Caller::Provider`.
/// This is the only way to obtain an authenticated provider caller —
/// the client cannot self-declare as an authenticated provider.
pub fn resolve_provider_callback(
    registry: &ServiceRoleRegistry,
    provider_id: &str,
    callback_signing_key: &str,
) -> Result<Caller, PaymentError> {
    let authenticated = registry.is_authorized_provider_callback(provider_id, callback_signing_key);
    Ok(Caller::Provider {
        provider_id: provider_id.to_string(),
        authenticated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> ServiceRoleRegistry {
        let mut reg = ServiceRoleRegistry::new();
        reg.register_api_key("key-001".into(), "quote-service".into());
        reg.register_api_key("key-002".into(), "risk-engine".into());
        reg.register_api_key("key-treasury".into(), "treasury".into());
        reg.register_mtls("svc.chain.omnia".into(), "chain-service".into());
        reg.register_provider_callback_key("MTN".into(), "mtn-pubkey-001".into());
        reg.register_reviewer("reviewer-alice".into());
        reg
    }

    #[test]
    fn resolve_api_key_to_system_service() {
        let reg = make_registry();
        let cred = Credential::ApiKey {
            key_id: "key-001".into(),
            service_name: "quote-service".into(),
        };
        let caller = reg.resolve(&cred, 1_700_000_000_000).expect("test assertion failed");
        assert_eq!(
            caller,
            Caller::System {
                service: "quote-service".into()
            }
        );
    }

    #[test]
    fn reject_unknown_api_key() {
        let reg = make_registry();
        let cred = Credential::ApiKey {
            key_id: "unknown".into(),
            service_name: "quote-service".into(),
        };
        let err = reg.resolve(&cred, 1_700_000_000_000).expect_err("should fail");
        assert!(matches!(err, PaymentError::Unauthorized { .. }));
    }

    #[test]
    fn resolve_mtls_to_system_service() {
        let reg = make_registry();
        let cred = Credential::Mtls {
            common_name: "svc.chain.omnia".into(),
        };
        let caller = reg.resolve(&cred, 1_700_000_000_000).expect("test assertion failed");
        assert_eq!(
            caller,
            Caller::System {
                service: "chain-service".into()
            }
        );
    }

    #[test]
    fn resolve_jwt_with_service_scope() {
        let reg = make_registry();
        let cred = Credential::Jwt {
            subject: "quote-svc".into(),
            claims: JwtClaims {
                sub: "quote-svc".into(),
                scope: Some("service:quote-service".into()),
                iss: Some("omnia-internal".into()),
                exp: 2_000_000_000_000,
                iat: 1_700_000_000_000,
            },
        };
        let caller = reg.resolve(&cred, 1_700_000_000_000).expect("test assertion failed");
        assert_eq!(
            caller,
            Caller::System {
                service: "quote-service".into()
            }
        );
    }

    #[test]
    fn resolve_jwt_with_provider_scope_authenticated() {
        let reg = make_registry();
        let cred = Credential::Jwt {
            subject: "mtn-callback".into(),
            claims: JwtClaims {
                sub: "mtn-callback".into(),
                scope: Some("provider:MTN:authenticated".into()),
                iss: Some("mtn".into()),
                exp: 2_000_000_000_000,
                iat: 1_700_000_000_000,
            },
        };
        let caller = reg.resolve(&cred, 1_700_000_000_000).expect("test assertion failed");
        assert_eq!(
            caller,
            Caller::Provider {
                provider_id: "MTN".into(),
                authenticated: true,
            }
        );
    }

    #[test]
    fn reject_expired_jwt() {
        let reg = make_registry();
        let cred = Credential::Jwt {
            subject: "svc".into(),
            claims: JwtClaims {
                sub: "svc".into(),
                scope: Some("service:quote-service".into()),
                iss: None,
                exp: 1_600_000_000_000, // expired
                iat: 1_599_999_000_000,
            },
        };
        let err = reg.resolve(&cred, 1_700_000_000_000).expect_err("should fail");
        assert!(matches!(err, PaymentError::QuoteExpired { .. }));
    }

    #[test]
    fn resolve_jwt_no_scope_is_sender() {
        let reg = make_registry();
        let cred = Credential::Jwt {
            subject: "user-123".into(),
            claims: JwtClaims {
                sub: "user-123".into(),
                scope: None,
                iss: Some("omnia-auth".into()),
                exp: 2_000_000_000_000,
                iat: 1_700_000_000_000,
            },
        };
        let caller = reg.resolve(&cred, 1_700_000_000_000).expect("test assertion failed");
        assert_eq!(caller, Caller::Sender);
    }

    #[test]
    fn provider_callback_authorization() {
        let reg = make_registry();
        // Known key for MTN → authenticated
        assert!(reg.is_authorized_provider_callback("MTN", "mtn-pubkey-001"));
        // Unknown key for MTN → not authenticated
        assert!(!reg.is_authorized_provider_callback("MTN", "unknown-key"));
        // Wrong provider
        assert!(!reg.is_authorized_provider_callback("Telecel", "mtn-pubkey-001"));
    }

    #[test]
    fn resolve_provider_callback_function() {
        let reg = make_registry();
        let caller = resolve_provider_callback(&reg, "MTN", "mtn-pubkey-001").expect("test assertion failed");
        assert_eq!(
            caller,
            Caller::Provider {
                provider_id: "MTN".into(),
                authenticated: true,
            }
        );

        let caller = resolve_provider_callback(&reg, "MTN", "attacker-key").expect("test assertion failed");
        assert_eq!(
            caller,
            Caller::Provider {
                provider_id: "MTN".into(),
                authenticated: false,
            }
        );
    }
}
