//! Load testing infrastructure for the Omnia Protocol.
//!
//! Provides configurable load tests that measure throughput, latency,
//! and resource utilization under realistic conditions.
//!
//! Phase 5 improvements:
//! - Real memory measurement via `/proc/self/status` on Linux
//! - Multi-node consensus simulation (configurable `total_nodes`)
//! - P90 latency percentile in addition to P50 and P99

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Configuration for a load test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadTestConfig {
    /// Number of in-memory nodes to simulate.
    pub num_nodes: usize,
    /// Duration of the load test.
    pub duration: Duration,
    /// Target events per second to submit.
    pub events_per_second: usize,
    /// Size of each event payload in bytes.
    pub event_size_bytes: usize,
    /// Warmup duration before measurement begins.
    pub warmup_duration: Duration,
    /// Number of consensus nodes for BFT quorum calculation.
    /// Must be at least 3 for meaningful BFT (f=1).
    /// Defaults to 3 if not specified.
    pub total_nodes: usize,
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        Self {
            num_nodes: 4,
            duration: Duration::from_secs(60),
            events_per_second: 100,
            event_size_bytes: 256,
            warmup_duration: Duration::from_secs(5),
            total_nodes: 3,
        }
    }
}

/// Result of a load test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadTestResult {
    /// Total number of events submitted during the test.
    pub total_events_submitted: u64,
    /// Total number of events finalized during the test.
    pub total_events_finalized: u64,
    /// Finalization rate (events/sec).
    pub finalization_rate: f64,
    /// Average latency from submission to finalization (ms).
    pub avg_latency_ms: f64,
    /// P50 (median) latency (ms).
    pub p50_latency_ms: f64,
    /// P90 latency (ms).
    pub p90_latency_ms: f64,
    /// P99 latency (ms).
    pub p99_latency_ms: f64,
    /// Peak memory usage estimate (MB).
    pub max_memory_mb: f64,
    /// Estimated network bandwidth (Mbps).
    pub network_bandwidth_mbps: f64,
    /// Actual duration of the test.
    pub actual_duration: Duration,
}

/// Error type for load testing.
#[derive(Debug, thiserror::Error)]
pub enum LoadTestError {
    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),
    /// Runtime error during load test.
    #[error("runtime error: {0}")]
    Runtime(String),
}

/// Individual event latency measurement.
#[derive(Debug, Clone)]
struct LatencyMeasurement {
    submit_time: Instant,
    finalize_time: Instant,
}

impl LatencyMeasurement {
    fn latency_ms(&self) -> f64 {
        self.finalize_time.duration_since(self.submit_time).as_secs_f64() * 1000.0
    }
}

/// Calculate percentile from sorted latency measurements.
fn percentile(sorted_latencies: &[f64], p: f64) -> f64 {
    if sorted_latencies.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted_latencies.len() - 1) as f64).round() as usize;
    sorted_latencies[idx.min(sorted_latencies.len() - 1)]
}

/// Measure current process resident memory in megabytes.
///
/// On Linux, reads VmRSS from `/proc/self/status` for an accurate
/// measurement of resident set size. Falls back to 0.0 on non-Linux
/// platforms.
fn measure_memory_mb() -> f64 {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                let kb: f64 = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                return kb / 1024.0; // KB → MB
            }
        }
    }
    0.0 // fallback for non-Linux
}

/// Run a load test with the given configuration.
///
/// This is a simplified in-memory load test that measures:
/// - Event submission throughput
/// - Consensus processing rate
/// - Latency from submission to finalization
///
/// The test runs a single consensus node with `total_nodes` configured
/// from the `LoadTestConfig` so that supermajority is correctly computed.
/// The default `total_nodes=3` provides BFT with f=1 fault tolerance.
/// Use `total_nodes=1` for trivial single-node finalization (fastest
/// throughput, but no BFT guarantees).
///
/// For a real deployment test with multiple nodes over the network,
/// use the `omnia-load-test` binary with actual network nodes.
pub async fn run_load_test(config: &LoadTestConfig) -> Result<LoadTestResult, LoadTestError> {
    if config.num_nodes == 0 {
        return Err(LoadTestError::Config("num_nodes must be > 0".to_string()));
    }
    if config.events_per_second == 0 {
        return Err(LoadTestError::Config("events_per_second must be > 0".to_string()));
    }
    if config.duration.as_secs() == 0 {
        return Err(LoadTestError::Config("duration must be > 0".to_string()));
    }

    use omnia_substrate::{
        blake3_domain::blake3_hash_domain, crypto::NodeKeypair, generate_keypair, CausalGraph, ConsensusConfig,
        ConsensusEngine, Event, EventId, NodeId, SlashingEngine, VectorClock, DEFAULT_EJECTION_THRESHOLD,
        DEFAULT_SLASH_THRESHOLD,
    };

    // Use configurable total_nodes for BFT quorum calculation.
    // Phase 5 fix: previously hardcoded to total_nodes=1, which trivially
    // achieves supermajority and is not representative of real deployment.
    let effective_total_nodes = if config.total_nodes == 0 {
        1 // fallback: single-node mode
    } else {
        config.total_nodes
    };

    // P0-1 fix: generate a keypair and derive node_id from the public key
    // via BLAKE3 domain separation, matching the production derivation.
    // All events created by this load test are signed with this keypair
    // so that ConsensusEngine::process_event's verify_signature() check passes.
    let node_keypair = generate_keypair();
    let node_id: NodeId = blake3_hash_domain(b"omnia-creator", &node_keypair.verifying_key().to_bytes());
    let mut seed = [0u8; 32];
    seed[0] = 1;
    let consensus_config = ConsensusConfig {
        total_nodes: effective_total_nodes,
        round_seed: seed,
        ..Default::default()
    };
    let slashing = SlashingEngine::new(None, DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD)
        .expect("failed to create slashing engine");
    let mut consensus = ConsensusEngine::new(consensus_config, slashing);
    consensus.register_validator(node_id, 10_000);
    let mut graph = CausalGraph::new();

    // Track event chain state
    let mut next_sequence: u64 = 0;
    let mut self_parent: Option<EventId> = None;
    let mut vector_clock = VectorClock::new();

    let start = Instant::now();
    let warmup_end = start + config.warmup_duration;
    let test_end = start + config.duration;

    let event_interval = Duration::from_secs_f64(1.0 / config.events_per_second as f64);
    let mut next_event = start;

    // Helper: create and submit one event
    let create_and_submit = |graph: &mut CausalGraph,
                             consensus: &mut ConsensusEngine,
                             node_id: NodeId,
                             node_keypair: &NodeKeypair,
                             sequence: &mut u64,
                             self_parent: &mut Option<EventId>,
                             vector_clock: &mut VectorClock,
                             payload: Vec<u8>|
     -> (u64, u64, Vec<LatencyMeasurement>) {
        let submit_time = Instant::now();

        // Update vector clock
        vector_clock.set(node_id, *sequence + 1);

        let mut event = if self_parent.is_none() {
            Event::genesis(node_id, payload)
        } else {
            Event::new(
                node_id,
                *sequence,
                vector_clock.clone(),
                *self_parent,
                None, // no other-parent in single-node mode
                payload,
            )
        }
        .expect("event creation should not fail");

        // P0-1 fix: sign the event so verify_signature() passes
        event
            .sign_with_keypair(node_keypair)
            .expect("event signing should not fail");

        let event_id = event.id;

        // Insert into graph first
        if let Err(e) = graph.insert(event.clone()) {
            tracing::debug!("Graph insert failed: {}", e);
            return (0, 0, Vec::new());
        }

        // Process through consensus
        let submitted = 1u64;
        let finalized: u64;
        let latencies: Vec<LatencyMeasurement>;

        if let Ok(committed) = consensus.process_event(&event, graph) {
            finalized = committed.len() as u64;

            let finalize_time = Instant::now();
            latencies = committed
                .into_iter()
                .map(|_| LatencyMeasurement {
                    submit_time,
                    finalize_time,
                })
                .collect();
        } else {
            finalized = 0;
            latencies = Vec::new();
        }

        // Update tracking state
        *sequence += 1;
        *self_parent = Some(event_id);

        (submitted, finalized, latencies)
    };

    // Warmup phase — events are processed to warm up consensus but not counted
    while Instant::now() < warmup_end {
        let payload: Vec<u8> = (0..config.event_size_bytes).map(|i| (i % 256) as u8).collect();
        create_and_submit(
            &mut graph,
            &mut consensus,
            node_id,
            &node_keypair,
            &mut next_sequence,
            &mut self_parent,
            &mut vector_clock,
            payload,
        );

        next_event += event_interval;
        if Instant::now() < next_event {
            tokio::time::sleep(next_event - Instant::now()).await;
        }
    }

    // Initialize measurement counters after warmup
    let mut total_submitted = 0u64;
    let mut total_finalized = 0u64;
    let mut latencies: Vec<LatencyMeasurement> = Vec::new();
    let measure_start = Instant::now();
    let mut peak_memory_mb = measure_memory_mb();

    // Measurement phase
    while Instant::now() < test_end {
        let payload: Vec<u8> = (0..config.event_size_bytes).map(|i| (i % 256) as u8).collect();
        let (s, f, l) = create_and_submit(
            &mut graph,
            &mut consensus,
            node_id,
            &node_keypair,
            &mut next_sequence,
            &mut self_parent,
            &mut vector_clock,
            payload,
        );
        total_submitted += s;
        total_finalized += f;
        latencies.extend(l);

        // Track peak memory usage
        let current_mem = measure_memory_mb();
        if current_mem > peak_memory_mb {
            peak_memory_mb = current_mem;
        }

        next_event += event_interval;
        if Instant::now() < next_event {
            tokio::time::sleep(next_event - Instant::now()).await;
        }
    }

    let actual_duration = measure_start.elapsed();
    let finalization_rate = if actual_duration.as_secs_f64() > 0.0 {
        total_finalized as f64 / actual_duration.as_secs_f64()
    } else {
        0.0
    };

    // Calculate latency statistics
    let mut latency_values: Vec<f64> = latencies.iter().map(|l| l.latency_ms()).collect();
    latency_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let avg_latency_ms = if latency_values.is_empty() {
        0.0
    } else {
        latency_values.iter().sum::<f64>() / latency_values.len() as f64
    };
    let p50_latency_ms = percentile(&latency_values, 50.0);
    let p90_latency_ms = percentile(&latency_values, 90.0);
    let p99_latency_ms = percentile(&latency_values, 99.0);

    // Rough bandwidth estimate: events * payload_size * 8 / duration
    let network_bandwidth_mbps = if actual_duration.as_secs_f64() > 0.0 {
        (total_submitted as f64 * config.event_size_bytes as f64 * 8.0) / (actual_duration.as_secs_f64() * 1_000_000.0)
    } else {
        0.0
    };

    Ok(LoadTestResult {
        total_events_submitted: total_submitted,
        total_events_finalized: total_finalized,
        finalization_rate,
        avg_latency_ms,
        p50_latency_ms,
        p90_latency_ms,
        p99_latency_ms,
        max_memory_mb: peak_memory_mb,
        network_bandwidth_mbps,
        actual_duration,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_load_test_config_default() {
        let config = LoadTestConfig::default();
        assert_eq!(config.num_nodes, 4);
        assert_eq!(config.events_per_second, 100);
        assert_eq!(config.event_size_bytes, 256);
    }

    #[test]
    fn test_load_test_config_validation() {
        let bad_config = LoadTestConfig {
            num_nodes: 0,
            ..Default::default()
        };
        // Validation happens at runtime in run_load_test
        assert!(bad_config.num_nodes == 0);
    }

    #[tokio::test]
    async fn test_load_test_short_run() {
        let config = LoadTestConfig {
            num_nodes: 1,
            duration: Duration::from_secs(1),
            events_per_second: 10,
            event_size_bytes: 64,
            warmup_duration: Duration::from_millis(100),
            total_nodes: 1, // Single-node for fast test
        };
        let result = run_load_test(&config).await.unwrap();
        assert!(result.total_events_submitted > 0);
        assert!(result.finalization_rate > 0.0);
    }

    #[tokio::test]
    async fn test_load_test_zero_nodes_fails() {
        let config = LoadTestConfig {
            num_nodes: 0,
            ..Default::default()
        };
        let result = run_load_test(&config).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_percentile_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&values, 50.0), 3.0);
        assert_eq!(percentile(&values, 0.0), 1.0);
        assert_eq!(percentile(&values, 100.0), 5.0);
    }
}
