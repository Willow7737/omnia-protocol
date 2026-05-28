//! Event submission and retrieval API handlers
//!
//! Provides endpoints for submitting new events to the substrate
//! and retrieving events by their identifier:
//! - `POST /api/v1/events` — submit a new event
//! - `GET /api/v1/events/:id` — retrieve an event by ID

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use omnia_substrate::{generate_keypair, Event};
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

    let node_id = state.config.node_id_bytes();

    // Create and sign a substrate event
    let keypair = generate_keypair();
    let mut event = Event::genesis(node_id, payload_bytes.clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid event: {e}")}))))?;
    event.sign_with_keypair(&keypair);

    let event_id_hex = hex::encode(event.id);
    let creator_hex = hex::encode(&event.creator[..4]);
    let timestamp = event.timestamp;

    // Attempt to submit to the substrate
    let status = {
        let mut substrate = state.substrate.write().await;
        match substrate.submit_event(event).await {
            Ok(()) => "submitted".to_string(),
            Err(e) => {
                tracing::warn!(event_id = %event_id_hex, error = %e, "Event submission failed");
                "submission_failed".to_string()
            }
        }
    };

    // Store the simplified event representation
    let stored = StoredEvent {
        id: event_id_hex.clone(),
        creator: creator_hex,
        sequence: 0,
        timestamp,
        payload: body.payload.clone(),
        event_type: body.event_type.clone(),
        status: status.clone(),
    };

    state.event_store.write().await.insert(event_id_hex.clone(), stored);

    // Increment the events counter
    #[cfg(feature = "metrics")]
    state.metrics.events_submitted.inc();

    tracing::info!(event_id = %event_id_hex, status = %status, "Event submitted via API");

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "event_id": event_id_hex,
            "status": status,
        })),
    ))
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
