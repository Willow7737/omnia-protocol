//! Omnia Protocol load test binary.
//!
//! Runs a configurable load test against the in-memory consensus engine.
//!
//! # Usage
//!
//! ```bash
//! # Default: 4 nodes, 100 events/sec, 60s
//! cargo run --release --bin omnia-load-test
//!
//! # Custom configuration via CLI arguments
//! cargo run --release --bin omnia-load-test -- --nodes 5 --rate 1000 --duration 60s
//!
//! # Legacy environment variable support still works
//! NUM_NODES=8 DURATION_SECS=120 EVENTS_PER_SEC=500 cargo run --bin omnia-load-test
//! ```

use std::time::Duration;

use clap::Parser;
use omnia_chaos_tests::load_test::{run_load_test, LoadTestConfig};

/// Omnia Protocol load test runner.
#[derive(Parser, Debug)]
#[command(
    name = "omnia-load-test",
    version,
    about = "Run load tests against the Omnia Protocol consensus engine"
)]
struct Args {
    /// Number of simulated consensus nodes.
    #[arg(long, default_value_t = 4)]
    nodes: usize,

    /// Target event submission rate (events per second).
    #[arg(long, default_value_t = 100)]
    rate: usize,

    /// Test duration (e.g., "60s", "2m").
    #[arg(long, default_value = "60s")]
    duration: String,

    /// Size of each event payload in bytes.
    #[arg(long, default_value_t = 256)]
    event_size: usize,

    /// Warmup duration before measurement begins (e.g., "5s").
    #[arg(long, default_value = "5s")]
    warmup: String,

    /// Number of consensus nodes for BFT quorum calculation (default: 3).
    /// Use 1 for single-node trivial finalization, 3+ for BFT.
    #[arg(long, default_value_t = 3)]
    total_nodes: usize,
}

/// Parse a duration string like "60s", "2m", "1h", or a plain number of seconds.
fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Try plain number (seconds)
    if let Ok(secs) = s.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    // Try suffix-based parsing
    if let Some(num) = s.strip_suffix('s') {
        if let Ok(secs) = num.parse::<u64>() {
            return Some(Duration::from_secs(secs));
        }
    }
    if let Some(num) = s.strip_suffix('m') {
        if let Ok(mins) = num.parse::<u64>() {
            return Some(Duration::from_secs(mins * 60));
        }
    }
    if let Some(num) = s.strip_suffix('h') {
        if let Ok(hours) = num.parse::<u64>() {
            return Some(Duration::from_secs(hours * 3600));
        }
    }

    None
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let duration = parse_duration(&args.duration).unwrap_or_else(|| {
        eprintln!(
            "Invalid duration '{}'. Use e.g., '60s', '2m', or a plain number of seconds.",
            args.duration
        );
        std::process::exit(1);
    });

    let warmup = parse_duration(&args.warmup).unwrap_or_else(|| {
        eprintln!("Invalid warmup duration '{}'. Use e.g., '5s'.", args.warmup);
        std::process::exit(1);
    });

    let config = LoadTestConfig {
        num_nodes: args.nodes,
        duration,
        events_per_second: args.rate,
        event_size_bytes: args.event_size,
        warmup_duration: warmup,
        total_nodes: args.total_nodes,
    };

    println!("Starting Omnia Protocol load test");
    println!("  Nodes: {}", config.num_nodes);
    println!("  BFT total_nodes: {}", config.total_nodes);
    println!("  Duration: {:?}", config.duration);
    println!("  Warmup: {:?}", config.warmup_duration);
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
            println!("P90 latency: {:.2} ms", result.p90_latency_ms);
            println!("P99 latency: {:.2} ms", result.p99_latency_ms);
            println!("Peak memory: {:.1} MB", result.max_memory_mb);
            println!("Bandwidth: {:.2} Mbps", result.network_bandwidth_mbps);
            println!("Actual duration: {:?}", result.actual_duration);
        }
        Err(e) => {
            eprintln!("Load test failed: {}", e);
            std::process::exit(1);
        }
    }
}
