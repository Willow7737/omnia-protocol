//! Event submission and retrieval API handlers
//!
//! Provides endpoints for submitting new events to the substrate
//! and retrieving events by their identifier:
//! - `POST /api/v1/events` — submit a new event
//! - `GET /api/v1/events` — list recently submitted events
//! - `GET /api/v1/events/:id` — retrieve an event by ID

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use omnia_primitives::blake3_hash_domain;
use omnia_substrate::Event;
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::state::AppState;

/// Request body for submitting a new event.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SubmitEventRequest {
    /// Optional hex-encoded payload data.
    #[serde(default)]
    pub payload: String,
    /// Optional event type hint (e.g., "generic", "transfer", "governance").
    #[serde(default = "default_event_type")]
    pub event_type: String,
}

fn default_event_type() -> String {
    "generic".to_string()
}

/// A stored event in the in-memory event store.
///
/// This is a simplified representation suitable for API responses.
/// The full substrate `Event` type includes vector clocks and
/// cryptographic signatures that are not exposed to API consumers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema)]
pub struct StoredEvent {
    /// Hex-encoded unique event identifier.
    pub id: String,
    /// Hex-encoded creator node identifier.
    pub creator: String,
    /// Monotonic sequence number from the creator.
    pub sequence: u64,
    /// Unix-millisecond timestamp when the event was created.
    pub timestamp: u64,
    /// Hex-encoded event payload.
    pub payload: String,
    /// Event type hint.
    pub event_type: String,
    /// Current event status.
    pub status: String,
}

/// Handler for `POST /api/v1/events`.
///
/// Accepts a JSON event submission, creates a signed substrate event,
/// attempts to submit it to the causal graph, and stores a simplified
/// representation in the in-memory event store for later retrieval.
///
/// Returns 201 on success with the event ID.
#[utoipa::path(
    post,
    path = "/api/v1/events",
    request_body = SubmitEventRequest,
    responses(
        (status = 201, description = "Event submitted successfully"),
        (status = 400, description = "Invalid request"),
    )
)]
pub async fn submit_event(
    State(state): State<AppState>,
    Json(body): Json<SubmitEventRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    // Decode the payload from hex, defaulting to empty bytes
    let payload_bytes = if body.payload.is_empty() {
        Vec::new()
    } else {
        hex::decode(&body.payload).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Invalid hex payload: {e}")})),
            )
        })?
    };

    // Reject oversized payloads at the HTTP layer before creating an event
    if payload_bytes.len() > omnia_substrate::MAX_PAYLOAD_SIZE {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": format!(
                    "Payload too large: {} bytes (max {})",
                    payload_bytes.len(),
                    omnia_substrate::MAX_PAYLOAD_SIZE
                )
            })),
        ));
    }

    // Use the node's persistent keypair for signing — events must be
    // verifiable as originating from this node. Ephemeral keypairs would
    // make signature verification meaningless.
    let keypair = state.keypair.clone().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "No persistent keypair configured"})),
        )
    })?;

    // The on-chain creator identity is derived from the signing key by
    // `sign_with_keypair` (creator = blake3("omnia-creator", pubkey)), not
    // the configured node ID. Chain lookups must use the same identity.
    let creator = blake3_hash_domain(b"omnia-creator", &keypair.verifying_key().to_bytes());

    // Build and submit under a single substrate write lock so that two
    // concurrent submissions cannot mint the same sequence number.
    //
    // Every event must extend this creator's previous event: re-using a
    // (creator, sequence) pair is equivocation and the consensus layer
    // slashes the validator for it. The previous implementation created
    // every API event via `Event::genesis()` — creator + sequence 0 each
    // time — so the *second* submission was indistinguishable from a
    // Byzantine fork and permanently slashed the node's own validator.
    let (submit_result, event_id_hex, event_sequence, timestamp) = {
        let mut substrate = state.substrate.write().await;

        let mut event = {
            let graph = substrate.graph().await;
            let own_events = graph.by_creator(&creator);
            match own_events.into_iter().max_by_key(|e| e.sequence) {
                // First event from this validator — a genuine genesis.
                None => Event::genesis(creator, payload_bytes.clone()),
                Some(latest) => {
                    let sequence = latest.sequence + 1;
                    let self_parent = latest.id;
                    // Advance our own clock entry on top of everything
                    // observed (convention: own entry = sequence + 1, so
                    // genesis is seq 0 / clock 1).
                    let mut vector_clock = graph.frontier().clone();
                    vector_clock.set(creator, sequence + 1);
                    // Two-parent, Hashgraph-style: reference the newest
                    // tip from another creator when one exists.
                    let other_parent = graph
                        .tips()
                        .filter_map(|id| graph.get(id))
                        .filter(|e| e.creator != creator)
                        .max_by_key(|e| e.timestamp)
                        .map(|e| e.id);
                    Event::new(
                        creator,
                        sequence,
                        vector_clock,
                        Some(self_parent),
                        other_parent,
                        payload_bytes.clone(),
                    )
                }
            }
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("Invalid event: {e}")})),
                )
            })?
        };

        event.sign_with_keypair(&keypair).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Signing failed: {e}")})),
            )
        })?;

        let event_id_hex = hex::encode(event.id);
        let event_sequence = event.sequence;
        let timestamp = event.timestamp;
        let result = substrate.submit_event(event).await;
        (result, event_id_hex, event_sequence, timestamp)
    };
    let creator_hex = hex::encode(&creator[..4]);

    match submit_result {
        Ok(()) => {
            // Store the simplified event representation with "submitted" status
            let stored = StoredEvent {
                id: event_id_hex.clone(),
                creator: creator_hex,
                sequence: event_sequence,
                timestamp,
                payload: body.payload.clone(),
                event_type: body.event_type.clone(),
                status: "submitted".to_string(),
            };

            crate::state::store_event(&state.event_store, event_id_hex.clone(), stored).await;

            // Increment the events counter
            #[cfg(feature = "metrics")]
            state.metrics.events_submitted.inc();

            tracing::info!(event_id = %event_id_hex, status = "submitted", "Event submitted via API");

            Ok((
                StatusCode::CREATED,
                Json(json!({
                    "event_id": event_id_hex,
                    "status": "submitted",
                })),
            ))
        }
        Err(e) => {
            tracing::warn!(event_id = %event_id_hex, error = %e, "Event submission failed");

            // Store the event with "submission_failed" status for diagnostics
            let stored = StoredEvent {
                id: event_id_hex.clone(),
                creator: creator_hex,
                sequence: event_sequence,
                timestamp,
                payload: body.payload.clone(),
                event_type: body.event_type.clone(),
                status: "submission_failed".to_string(),
            };

            crate::state::store_event(&state.event_store, event_id_hex.clone(), stored).await;

            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Event submission failed: {e}"),
                    "event_id": event_id_hex,
                })),
            ))
        }
    }
}

/// Handler for `GET /api/v1/events/:id`.
///
/// Looks up an event by its hex-encoded ID in the in-memory
/// event store. Returns 404 if the event is not found.
#[utoipa::path(
    get,
    path = "/api/v1/events/{id}",
    params(
        ("id" = String, Path, description = "Hex-encoded event ID")
    ),
    responses(
        (status = 200, description = "Event found"),
        (status = 404, description = "Event not found"),
    )
)]
pub async fn get_event(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.event_store.read().await;
    match store.get(&id) {
        Some(event) => Ok(Json(
            serde_json::to_value(event).unwrap_or_else(|_| json!({"error": "Serialization failed"})),
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("Event not found: {id}")})),
        )),
    }
}

/// Query parameters for `GET /api/v1/events`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListEventsQuery {
    /// Maximum number of events to return (default: 100, max: 1000).
    ///
    /// The most recently submitted events are returned first.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    100
}

/// Maximum number of events the API will return in a single request.
const MAX_LIST_LIMIT: usize = 1000;

/// Handler for `GET /api/v1/events`.
///
/// Returns the most recently submitted events from the in-memory event
/// store, newest first. The store preserves insertion order via
/// `IndexMap`, so "newest" is well-defined.
///
/// # Query parameters
///
/// - `limit` — maximum number of events to return (default 100, max 1000)
#[utoipa::path(
    get,
    path = "/api/v1/events",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum number of events to return (default 100, max 1000)")
    ),
    responses(
        (status = 200, description = "List of events", body = [StoredEvent]),
    )
)]
pub async fn list_events(State(state): State<AppState>, Query(query): Query<ListEventsQuery>) -> Json<Value> {
    let limit = query.limit.min(MAX_LIST_LIMIT);
    let store = state.event_store.read().await;

    // IndexMap preserves insertion order. Iterate in reverse so the
    // newest events come first. `.values().rev()` gives us only the
    // StoredEvent references (not the keys).
    let events: Vec<&StoredEvent> = store.values().rev().take(limit).collect();

    Json(json!({
        "events": events,
        "count": events.len(),
        "total_in_store": store.len(),
    }))
}
