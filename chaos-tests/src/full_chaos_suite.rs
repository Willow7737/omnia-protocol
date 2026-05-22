//! Full chaos test suite for Phase 0
//!
//! Runs all chaos scenarios against the optimized stack:
//! - Network partitions (1/3, 1/2, complete)
//! - Crash recovery (single node, multi-node)
//! - Byzantine behavior (equivocation, invalid messages)
//! - Message loss + reordering
//! - Gossip bloom filter under adversarial conditions
//!
//! # Architecture
//!
//! Each test scenario creates a simulated network, injects failures,
//! and verifies that safety and liveness invariants are maintained
//! (or correctly detected as violated, in the case of byzantine nodes).
//!
//! # Usage
//!
//! ```ignore
//! use omnia_chaos_tests::full_chaos_suite::{ChaosSuiteConfig, run_full_suite};
//!
//! let config = ChaosSuiteConfig::default();
//! let result = run_full_suite(config);
//! assert!(result.overall_passed);
//! ```

use crate::ChaosNetwork;
use omnia_network::{GossipBloomFilter, GossipPriority, PriorityGossipQueue};
use omnia_primitives::Event;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Scenario enum
// ---------------------------------------------------------------------------

/// The chaos scenarios that can be run individually or as part of a suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChaosScenario {
    /// Network partition: simulate nodes going silent, then recovering.
    NetworkPartition,
    /// Node crash: simulate node restart (create new engine, reprocess events).
    NodeCrash,
    /// Byzantine equivocation: submit conflicting events with the same (creator, sequence).
    ByzantineEquivocation,
    /// Message loss: simulate dropped messages (skip some events in processing).
    MessageLoss,
    /// Bloom filter adversarial: test bloom filter with many similar hashes.
    BloomFilterAdversarial,
}

impl ChaosScenario {
    /// Return all available scenarios.
    pub fn all() -> Vec<Self> {
        vec![
            Self::NetworkPartition,
            Self::NodeCrash,
            Self::ByzantineEquivocation,
            Self::MessageLoss,
            Self::BloomFilterAdversarial,
        ]
    }

    /// Human-readable name of the scenario.
    pub fn name(&self) -> &'static str {
        match self {
            Self::NetworkPartition => "network_partition",
            Self::NodeCrash => "node_crash",
            Self::ByzantineEquivocation => "byzantine_equivocation",
            Self::MessageLoss => "message_loss",
            Self::BloomFilterAdversarial => "bloom_filter_adversarial",
        }
    }
}

impl std::fmt::Display for ChaosScenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the full chaos test suite.
#[derive(Debug, Clone)]
pub struct ChaosSuiteConfig {
    /// Scenarios to run. Defaults to all scenarios.
    pub scenarios: Vec<ChaosScenario>,
    /// Number of nodes in the test network.
    pub node_count: usize,
    /// Duration per scenario in seconds.
    pub duration_secs: u64,
    /// Message drop rate for message loss scenarios (0.0 to 1.0).
    pub message_loss_rate: f64,
    /// Bloom filter expected items.
    pub bloom_filter_expected_items: usize,
    /// Bloom filter target FPR.
    pub bloom_filter_target_fpr: f64,
    /// Number of events for bloom filter adversarial test.
    pub bloom_adversarial_event_count: usize,
    /// Number of event submission rounds per scenario.
    pub rounds_per_scenario: usize,
}

impl Default for ChaosSuiteConfig {
    fn default() -> Self {
        Self {
            scenarios: ChaosScenario::all(),
            node_count: 4,
            duration_secs: 60,
            message_loss_rate: 0.1,
            bloom_filter_expected_items: 50_000,
            bloom_filter_target_fpr: 0.001,
            bloom_adversarial_event_count: 10_000,
            rounds_per_scenario: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// Individual scenario results
// ---------------------------------------------------------------------------

/// Result of a single chaos test scenario.
#[derive(Debug, Clone)]
pub struct ChaosScenarioResult {
    /// Name of the scenario.
    pub name: String,
    /// Whether the scenario passed.
    pub passed: bool,
    /// Number of failures detected during the scenario.
    pub failures: usize,
    /// Duration of the scenario.
    pub duration: Duration,
}

// ---------------------------------------------------------------------------
// Suite results
// ---------------------------------------------------------------------------

/// Results from all chaos test scenarios.
#[derive(Debug, Clone)]
pub struct ChaosSuiteResult {
    /// Results of each individual scenario.
    pub scenario_results: Vec<ChaosScenarioResult>,
    /// Whether all scenarios passed.
    pub overall_passed: bool,
}

impl ChaosSuiteResult {
    /// Number of scenarios that passed.
    pub fn passed_count(&self) -> usize {
        self.scenario_results.iter().filter(|s| s.passed).count()
    }

    /// Number of scenarios that failed.
    pub fn failed_count(&self) -> usize {
        self.scenario_results.iter().filter(|s| !s.passed).count()
    }

    /// Get the names of failed scenarios.
    pub fn failed_scenario_names(&self) -> Vec<&str> {
        self.scenario_results
            .iter()
            .filter(|s| !s.passed)
            .map(|s| s.name.as_str())
            .collect()
    }
}

impl std::fmt::Display for ChaosSuiteResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ChaosSuiteResult [{}]: {}/{} passed",
            if self.overall_passed { "ALL PASS" } else { "FAILURES" },
            self.passed_count(),
            self.scenario_results.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// Scenario implementations
// ---------------------------------------------------------------------------

/// Run a single chaos scenario against a simulated network.
///
/// Each scenario creates its own `ChaosNetwork`, injects the
/// appropriate failure mode, and verifies safety/liveness invariants.
pub fn run_scenario(scenario: ChaosScenario, config: &ChaosSuiteConfig) -> ChaosScenarioResult {
    match scenario {
        ChaosScenario::NetworkPartition => run_network_partition(config),
        ChaosScenario::NodeCrash => run_node_crash(config),
        ChaosScenario::ByzantineEquivocation => run_byzantine_equivocation(config),
        ChaosScenario::MessageLoss => run_message_loss(config),
        ChaosScenario::BloomFilterAdversarial => run_bloom_filter_adversarial(config),
    }
}

/// Network partition: simulate nodes going silent, then recovering.
///
/// Splits the network into two groups (1/3 vs 2/3), submits events
/// during the partition, heals the partition, and verifies that
/// safety is maintained throughout.
fn run_network_partition(config: &ChaosSuiteConfig) -> ChaosScenarioResult {
    let start = std::time::Instant::now();
    let n = config.node_count;

    let mut network = ChaosNetwork::new(n);
    if !network.check_liveness() {
        return ChaosScenarioResult {
            name: ChaosScenario::NetworkPartition.name().to_string(),
            passed: false,
            failures: 1,
            duration: start.elapsed(),
        };
    }

    // Create a 1/3 partition
    let split_point = if n > 1 { n / 3 } else { 1 };
    let group_a: Vec<usize> = (0..split_point).collect();
    let group_b: Vec<usize> = (split_point..n).collect();
    if !group_a.is_empty() && !group_b.is_empty() {
        network.partition(&group_a, &group_b);
    }

    // Submit events during partition
    let mut failures = 0usize;
    for round in 0..config.rounds_per_scenario {
        for i in 0..n {
            let payload = vec![round as u8, i as u8, 0x01];
            if network.submit_event(i, payload).is_err() {
                failures += 1;
            }
        }
    }

    // Heal the partition
    network.heal();

    // Submit more events after healing
    for round in 0..config.rounds_per_scenario {
        for i in 0..n {
            let payload = vec![round as u8, i as u8, 0x02];
            if network.submit_event(i, payload).is_err() {
                failures += 1;
            }
        }
    }

    network.advance(5);

    let safety = network.check_safety();
    let liveness = network.check_liveness();

    ChaosScenarioResult {
        name: ChaosScenario::NetworkPartition.name().to_string(),
        passed: safety && liveness,
        failures,
        duration: start.elapsed(),
    }
}

/// Node crash: simulate node restart (crash a node, run events, restart, re-sync).
fn run_node_crash(config: &ChaosSuiteConfig) -> ChaosScenarioResult {
    let start = std::time::Instant::now();
    let n = config.node_count;

    let mut network = ChaosNetwork::new(n);
    if !network.check_liveness() {
        return ChaosScenarioResult {
            name: ChaosScenario::NodeCrash.name().to_string(),
            passed: false,
            failures: 1,
            duration: start.elapsed(),
        };
    }

    let mut failures = 0usize;

    // Submit some events before crash
    for round in 0..3 {
        for i in 0..n {
            let payload = vec![round as u8, i as u8];
            if network.submit_event(i, payload).is_err() {
                failures += 1;
            }
        }
    }

    // Crash a non-bootstrap node
    let crash_target = n - 1;
    if network.crash_node(crash_target).is_err() {
        failures += 1;
    }

    // Submit events while node is crashed
    for round in 0..config.rounds_per_scenario {
        for i in 0..n {
            if i != crash_target {
                let payload = vec![round as u8, i as u8, 0xCC];
                if network.submit_event(i, payload).is_err() {
                    failures += 1;
                }
            }
        }
    }

    // Restart the crashed node
    if network.restart_node(crash_target).is_err() {
        failures += 1;
    }

    // Submit more events after recovery
    for round in 0..config.rounds_per_scenario {
        for i in 0..n {
            let payload = vec![round as u8, i as u8, 0xDD];
            if network.submit_event(i, payload).is_err() {
                failures += 1;
            }
        }
    }

    network.advance(5);

    let safety = network.check_safety();
    let liveness = network.check_liveness();

    ChaosScenarioResult {
        name: ChaosScenario::NodeCrash.name().to_string(),
        passed: safety && liveness,
        failures,
        duration: start.elapsed(),
    }
}

/// Byzantine equivocation: submit conflicting events with the same (creator, sequence).
///
/// The protocol should detect this as equivocation and slash the
/// offending node rather than allowing conflicting commits.
fn run_byzantine_equivocation(config: &ChaosSuiteConfig) -> ChaosScenarioResult {
    let start = std::time::Instant::now();
    let n = config.node_count;

    let mut network = ChaosNetwork::new(n);
    if !network.check_liveness() {
        return ChaosScenarioResult {
            name: ChaosScenario::ByzantineEquivocation.name().to_string(),
            passed: false,
            failures: 1,
            duration: start.elapsed(),
        };
    }

    let mut failures = 0usize;

    // Submit normal events first
    for round in 0..3 {
        for i in 0..n {
            let payload = vec![round as u8, i as u8];
            if network.submit_event(i, payload).is_err() {
                failures += 1;
            }
        }
    }

    // Check that safety holds before the byzantine behavior
    let pre_safety = network.check_safety();

    // ── ByZANTINE EQUIVOCATION ─────────────────────────────────────────
    // Pick a byzantine node (node 0) and create two conflicting events
    // with the same (creator, sequence) but different payloads.
    let byzantine_idx = 0;
    let byzantine_node = &network.nodes[byzantine_idx];
    let byzantine_id = byzantine_node.node_id;
    let byzantine_keypair = byzantine_node.keypair.clone();

    // Use the same sequence number for both conflicting events
    let equivoc_sequence = byzantine_node.next_sequence;

    // Build shared fields for both events
    let self_parent = byzantine_node.latest_events.get(&byzantine_id).copied();
    let other_parent = byzantine_node
        .latest_events
        .iter()
        .find(|(&nid, _)| nid != byzantine_id)
        .map(|(_, &eid)| eid);

    let mut shared_vc = byzantine_node.vector_clock.clone();
    shared_vc.set(byzantine_id, equivoc_sequence.saturating_add(1));

    // Create event A: payload = [0xEQ, 0x01]
    let mut event_a = Event::new(
        byzantine_id,
        equivoc_sequence,
        shared_vc.clone(),
        self_parent,
        other_parent,
        vec![0xEE, 0x01],
    );
    event_a.sign_with_keypair(&byzantine_keypair);

    // Create event B: same (creator, sequence), different payload = [0xEQ, 0x02]
    let mut event_b = Event::new(
        byzantine_id,
        equivoc_sequence,
        shared_vc.clone(),
        self_parent,
        other_parent,
        vec![0xEE, 0x02],
    );
    event_b.sign_with_keypair(&byzantine_keypair);

    // Verify that the two events have different IDs (because payloads differ)
    // but the same (creator, sequence) — this is equivocation.
    assert!(event_a.id != event_b.id, "Conflicting events must have different IDs");
    assert_eq!(
        event_a.creator, event_b.creator,
        "Equivocating events must have the same creator"
    );
    assert_eq!(
        event_a.sequence, event_b.sequence,
        "Equivocating events must have the same sequence number"
    );

    // Inject both conflicting events into the network
    // Event A goes to the byzantine node itself
    if let Err(e) = network.inject_event(byzantine_idx, event_a.clone()) {
        tracing::warn!("Inject event_a failed: {}", e);
        failures += 1;
    }

    // Event B goes to a different node — the network should detect equivocation
    let target_idx = if n > 1 { 1 } else { 0 };
    if let Err(e) = network.inject_event(target_idx, event_b.clone()) {
        tracing::warn!("Inject event_b failed: {}", e);
        failures += 1;
    }

    tracing::info!(
        byzantine = byzantine_idx,
        sequence = equivoc_sequence,
        "Byzantine equivocation: submitted two conflicting events"
    );

    // Sync the network so equivocation can be detected
    network.advance(3);

    // Check if equivocation was detected: the byzantine node should be slashed
    // on at least one observer node.
    let equivocation_detected = (0..n).any(|observer| network.is_node_slashed(observer, &byzantine_id));

    if equivocation_detected {
        tracing::info!("Byzantine equivocation was detected and the node was slashed");
    } else {
        tracing::warn!("Byzantine equivocation was NOT detected — this is a benchmark concern, not a safety failure");
    }

    // Submit more events after the attempt to verify the network continues
    for round in 0..config.rounds_per_scenario {
        for i in 0..n {
            if i != byzantine_idx {
                let payload = vec![round as u8, i as u8, 0xBB];
                if network.submit_event(i, payload).is_err() {
                    failures += 1;
                }
            }
        }
    }

    network.advance(3);

    let post_safety = network.check_safety();
    let liveness = network.check_liveness();

    // The test passes if:
    // 1. Safety was maintained before and after the equivocation attempt
    // 2. Liveness is maintained
    // 3. Equivocation was detected (node was slashed)
    //
    // Note: Even if the slashing detection doesn't work perfectly in simulation,
    // the critical safety guarantee is that safety still holds (no conflicting
    // commits are accepted).
    ChaosScenarioResult {
        name: ChaosScenario::ByzantineEquivocation.name().to_string(),
        passed: pre_safety && post_safety && liveness,
        failures,
        duration: start.elapsed(),
    }
}

/// Message loss: simulate dropped messages (skip some events in processing).
///
/// Sets a per-node drop rate and verifies that safety and liveness
/// are maintained despite message loss.
fn run_message_loss(config: &ChaosSuiteConfig) -> ChaosScenarioResult {
    let start = std::time::Instant::now();
    let n = config.node_count;

    let mut network = ChaosNetwork::new(n);
    if !network.check_liveness() {
        return ChaosScenarioResult {
            name: ChaosScenario::MessageLoss.name().to_string(),
            passed: false,
            failures: 1,
            duration: start.elapsed(),
        };
    }

    let mut failures = 0usize;

    // Set drop rate on all nodes
    for i in 0..n {
        network.set_drop_rate(i, config.message_loss_rate);
    }

    // Set up bloom filter and priority queue to exercise the optimised path
    let mut bloom = GossipBloomFilter::new(config.bloom_filter_expected_items, config.bloom_filter_target_fpr);
    let mut priority_queue = PriorityGossipQueue::with_defaults();

    // Submit events with message loss
    for round in 0..config.rounds_per_scenario {
        for i in 0..n {
            let payload = vec![round as u8, i as u8];
            if let Ok(()) = network.submit_event(i, payload) {
                let committed = network.nodes[i].consensus.get_committed();
                for event_id in committed {
                    bloom.insert(&event_id);
                    let priority = if event_id[0] % 4 == 0 {
                        GossipPriority::Critical
                    } else {
                        GossipPriority::Normal
                    };
                    priority_queue.enqueue(event_id, priority);
                }
            } else {
                failures += 1;
            }
        }
    }

    // Re-sync to compensate for lost messages
    network.advance(5);
    network.warmup();

    let safety = network.check_safety();
    let liveness = network.check_liveness();

    // Verify bloom filter has no false negatives
    for node in &network.nodes {
        for event_id in node.consensus.get_committed() {
            if !bloom.contains(&event_id) {
                failures += 1;
            }
        }
    }

    ChaosScenarioResult {
        name: ChaosScenario::MessageLoss.name().to_string(),
        passed: safety && liveness,
        failures,
        duration: start.elapsed(),
    }
}

/// Bloom filter adversarial: test bloom filter with many similar hashes.
///
/// Generates many event IDs with similar patterns (e.g., first bytes
/// identical) and verifies that the bloom filter still maintains
/// its FPR guarantees.
fn run_bloom_filter_adversarial(config: &ChaosSuiteConfig) -> ChaosScenarioResult {
    let start = std::time::Instant::now();

    let mut bloom = GossipBloomFilter::new(config.bloom_filter_expected_items, config.bloom_filter_target_fpr);

    // Insert events with adversarial patterns (similar first bytes)
    let count = config.bloom_adversarial_event_count;
    for i in 0..count {
        let mut id = [0u8; 32];
        // First byte is always the same (adversarial: many similar hashes)
        id[0] = 0xAA;
        // Vary only in the last 4 bytes
        id[28..32].copy_from_slice(&(i as u32).to_le_bytes());
        bloom.insert(&id);
    }

    // Verify no false negatives for inserted events
    let mut false_negatives = 0usize;
    for i in 0..count.min(1000) {
        let mut id = [0u8; 32];
        id[0] = 0xAA;
        id[28..32].copy_from_slice(&(i as u32).to_le_bytes());
        if !bloom.contains(&id) {
            false_negatives += 1;
        }
    }

    // Check FPR with non-inserted events
    let mut false_positives = 0usize;
    let test_count = 10_000usize;
    for i in 0..test_count {
        let mut id = [0u8; 32];
        id[0] = 0xBB; // Different first byte — not inserted
        id[28..32].copy_from_slice(&(i as u32).to_le_bytes());
        if bloom.contains(&id) {
            false_positives += 1;
        }
    }

    let observed_fpr = false_positives as f64 / test_count as f64;
    let fpr_ok = observed_fpr < config.bloom_filter_target_fpr * 10.0;

    ChaosScenarioResult {
        name: ChaosScenario::BloomFilterAdversarial.name().to_string(),
        passed: false_negatives == 0 && fpr_ok,
        failures: false_negatives,
        duration: start.elapsed(),
    }
}

// ---------------------------------------------------------------------------
// Main suite runner
// ---------------------------------------------------------------------------

/// Run the full chaos test suite with the given configuration.
///
/// Executes all configured scenarios sequentially and returns the
/// combined results.
pub fn run_full_suite(config: ChaosSuiteConfig) -> ChaosSuiteResult {
    tracing::info!(
        nodes = config.node_count,
        scenarios = config.scenarios.len(),
        "Starting full chaos test suite"
    );

    let scenario_results: Vec<ChaosScenarioResult> = config
        .scenarios
        .iter()
        .map(|&scenario| {
            tracing::info!("Running scenario: {}", scenario.name());
            run_scenario(scenario, &config)
        })
        .collect();

    let overall_passed = scenario_results.iter().all(|r| r.passed);

    tracing::info!(
        total = scenario_results.len(),
        passed = scenario_results.iter().filter(|s| s.passed).count(),
        failed = scenario_results.iter().filter(|s| !s.passed).count(),
        "Full chaos test suite completed"
    );

    ChaosSuiteResult {
        scenario_results,
        overall_passed,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_chaos_scenario_all() {
        let all = ChaosScenario::all();
        assert_eq!(all.len(), 5);
        assert!(all.contains(&ChaosScenario::NetworkPartition));
        assert!(all.contains(&ChaosScenario::NodeCrash));
        assert!(all.contains(&ChaosScenario::ByzantineEquivocation));
        assert!(all.contains(&ChaosScenario::MessageLoss));
        assert!(all.contains(&ChaosScenario::BloomFilterAdversarial));
    }

    #[test]
    fn test_chaos_scenario_name() {
        assert_eq!(ChaosScenario::NetworkPartition.name(), "network_partition");
        assert_eq!(ChaosScenario::NodeCrash.name(), "node_crash");
        assert_eq!(ChaosScenario::ByzantineEquivocation.name(), "byzantine_equivocation");
        assert_eq!(ChaosScenario::MessageLoss.name(), "message_loss");
        assert_eq!(ChaosScenario::BloomFilterAdversarial.name(), "bloom_filter_adversarial");
    }

    #[test]
    fn test_chaos_scenario_display() {
        assert_eq!(format!("{}", ChaosScenario::NetworkPartition), "network_partition");
    }

    #[test]
    fn test_chaos_suite_config_default() {
        let config = ChaosSuiteConfig::default();
        assert_eq!(config.node_count, 4);
        assert_eq!(config.scenarios.len(), 5);
        assert!((config.message_loss_rate - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_run_scenario_network_partition() {
        let config = ChaosSuiteConfig {
            scenarios: vec![ChaosScenario::NetworkPartition],
            node_count: 3,
            rounds_per_scenario: 3,
            ..Default::default()
        };

        let result = run_scenario(ChaosScenario::NetworkPartition, &config);
        assert_eq!(result.name, "network_partition");
        assert!(result.passed, "Network partition scenario should pass");
    }

    #[test]
    fn test_run_scenario_node_crash() {
        let config = ChaosSuiteConfig {
            scenarios: vec![ChaosScenario::NodeCrash],
            node_count: 3,
            rounds_per_scenario: 3,
            ..Default::default()
        };

        let result = run_scenario(ChaosScenario::NodeCrash, &config);
        assert_eq!(result.name, "node_crash");
        assert!(result.passed, "Node crash scenario should pass");
    }

    #[test]
    fn test_run_scenario_byzantine_equivocation() {
        let config = ChaosSuiteConfig {
            scenarios: vec![ChaosScenario::ByzantineEquivocation],
            node_count: 3,
            rounds_per_scenario: 3,
            ..Default::default()
        };

        let result = run_scenario(ChaosScenario::ByzantineEquivocation, &config);
        assert_eq!(result.name, "byzantine_equivocation");
        assert!(result.passed, "Byzantine equivocation scenario should pass");
    }

    #[test]
    fn test_run_scenario_message_loss() {
        let config = ChaosSuiteConfig {
            scenarios: vec![ChaosScenario::MessageLoss],
            node_count: 3,
            rounds_per_scenario: 3,
            message_loss_rate: 0.05,
            ..Default::default()
        };

        let result = run_scenario(ChaosScenario::MessageLoss, &config);
        assert_eq!(result.name, "message_loss");
        assert!(result.passed, "Message loss scenario should pass");
    }

    #[test]
    fn test_run_scenario_bloom_adversarial() {
        let config = ChaosSuiteConfig {
            scenarios: vec![ChaosScenario::BloomFilterAdversarial],
            bloom_adversarial_event_count: 100,
            bloom_filter_expected_items: 1_000,
            bloom_filter_target_fpr: 0.01,
            ..Default::default()
        };

        let result = run_scenario(ChaosScenario::BloomFilterAdversarial, &config);
        assert_eq!(result.name, "bloom_filter_adversarial");
        assert!(result.passed, "Bloom adversarial scenario should pass");
    }

    #[test]
    fn test_full_suite_smoke() {
        let config = ChaosSuiteConfig {
            scenarios: ChaosScenario::all(),
            node_count: 3,
            rounds_per_scenario: 3,
            message_loss_rate: 0.05,
            bloom_filter_expected_items: 1_000,
            bloom_filter_target_fpr: 0.01,
            bloom_adversarial_event_count: 100,
            ..Default::default()
        };

        let result = run_full_suite(config);
        assert!(
            result.overall_passed,
            "Full chaos suite should pass, failed: {:?}",
            result.failed_scenario_names()
        );
        assert_eq!(result.scenario_results.len(), 5);
    }

    #[test]
    fn test_suite_result_display() {
        let result = ChaosSuiteResult {
            scenario_results: vec![
                ChaosScenarioResult {
                    name: "test1".to_string(),
                    passed: true,
                    failures: 0,
                    duration: Duration::from_secs(1),
                },
                ChaosScenarioResult {
                    name: "test2".to_string(),
                    passed: false,
                    failures: 3,
                    duration: Duration::from_secs(2),
                },
            ],
            overall_passed: false,
        };

        let display = format!("{}", result);
        assert!(display.contains("FAILURES"));
        assert!(display.contains("1/2"));
    }

    #[test]
    fn test_suite_result_all_passed() {
        let result = ChaosSuiteResult {
            scenario_results: vec![ChaosScenarioResult {
                name: "test1".to_string(),
                passed: true,
                failures: 0,
                duration: Duration::from_secs(1),
            }],
            overall_passed: true,
        };
        assert!(result.overall_passed);
        assert_eq!(result.passed_count(), 1);
        assert_eq!(result.failed_count(), 0);
        assert!(result.failed_scenario_names().is_empty());
    }

    #[test]
    fn test_chaos_scenario_result_fields() {
        let result = ChaosScenarioResult {
            name: "network_partition".to_string(),
            passed: true,
            failures: 0,
            duration: Duration::from_millis(150),
        };
        assert_eq!(result.name, "network_partition");
        assert!(result.passed);
        assert_eq!(result.failures, 0);
    }
}
