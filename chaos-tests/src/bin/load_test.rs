//! Omnia Protocol load test binary.
//!
//! Runs a configurable load test against the in-memory consensus engine.

use std::time::Duration;

use omnia_chaos_tests::load_test::{run_load_test, LoadTestConfig};

#[tokio::main]
async fn main() {
    let config = LoadTestConfig {
        num_nodes: std::env::var("NUM_NODES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4),
        duration: Duration::from_secs(
            std::env::var("DURATION_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
        ),
        events_per_second: std::env::var("EVENTS_PER_SEC")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100),
        event_size_bytes: std::env::var("EVENT_SIZE_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(256),
        warmup_duration: Duration::from_secs(5),
    };

    println!("Starting Omnia Protocol load test");
    println!("  Nodes: {}", config.num_nodes);
    println!("  Duration: {:?}", config.duration);
    println!("  Target rate: {} events/sec", config.events_per_second);
    println!("  Event size: {} bytes", config.event_size_bytes);

    match run_load_test(&config).await {
        Ok(result) => {
            println!("\n=== Load Test Results ===");
            println!("Events submitted: {}", result.total_events_submitted);
            println!("Events finalized: {}", result.total_events_finalized);
            println!(
                "Finalization rate: {:.1} events/sec",
                result.finalization_rate
            );
            println!("Avg latency: {:.2} ms", result.avg_latency_ms);
            println!("P50 latency: {:.2} ms", result.p50_latency_ms);
            println!("P99 latency: {:.2} ms", result.p99_latency_ms);
            println!("Bandwidth: {:.2} Mbps", result.network_bandwidth_mbps);
            println!("Actual duration: {:?}", result.actual_duration);
        }
        Err(e) => {
            eprintln!("Load test failed: {}", e);
            std::process::exit(1);
        }
    }
}
