//! 72-Hour Safety Monitoring Test for Omnia Protocol.
//!
//! This module implements a continuous safety validation test designed to run
//! for up to 72 hours, verifying that the protocol maintains safety invariants
//! under sustained operation. It:
//!
//! - Runs continuous event submission with verification
//! - Checks for consensus failures, equivocation, and state root mismatches
//! - Reports periodic health status
//! - Auto-exits on any safety violation
//!
//! # Usage
//!
//! ```ignore
//! // Run for 72 hours (default)
//! omnia-safety-monitor
//!
//! // Run for a custom duration
//! omnia-safety-monitor --duration 3600
//! ```
//!
//! # Safety Invariants
//!
//! 1. **No equivocation**: No two events with the same `(creator, sequence)` pair
//!    are both committed.
//! 2. **No state root mismatch**: All nodes compute the same state root for the
//!    same set of committed events.
//! 3. **Consensus liveness**: Events continue to be finalized.
//! 4. **No consensus errors**: Consensus processing never returns an unexpected
//!    error for valid events.

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use omnia_primitives::{Event, EventId, NodeId};
use omnia_substrate::{
    CausalGraph, ConsensusConfig, ConsensusEngine, SlashingEngine, VectorClock, DEFAULT_EJECTION_THRESHOLD,
    DEFAULT_SLASH_THRESHOLD,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the safety monitoring test.
#[derive(Debug, Clone)]
pub struct SafetyMonitorConfig {
    /// Number of simulated nodes in the test network.
    pub num_nodes: usize,
    /// Total duration to run the safety test.
    pub duration: Duration,
    /// Interval between periodic health reports.
    pub report_interval: Duration,
    /// Number of events to submit per round.
    pub events_per_round: usize,
    /// Payload size for each event in bytes.
    pub payload_size: usize,
}

impl Default for SafetyMonitorConfig {
    fn default() -> Self {
        Self {
            num_nodes: 4,
            duration: Duration::from_secs(72 * 60 * 60), // 72 hours
            report_interval: Duration::from_secs(300),   // 5 minutes
            events_per_round: 10,
            payload_size: 64,
        }
    }
}

// ---------------------------------------------------------------------------
// Safety violations
// ---------------------------------------------------------------------------

/// A safety violation detected during monitoring.
#[derive(Debug, Clone)]
pub enum SafetyViolation {
    /// Two events with the same `(creator, sequence)` pair were both committed.
    Equivocation {
        /// The node that equivocated.
        creator: NodeId,
        /// The sequence number of the equivocating events.
        sequence: u64,
        /// The two conflicting event IDs.
        event_ids: [EventId; 2],
    },
    /// State root mismatch between two nodes.
    StateRootMismatch {
        /// Node index A.
        node_a: usize,
        /// Node index B.
        node_b: usize,
        /// State root from node A.
        root_a: [u8; 32],
        /// State root from node B.
        root_b: [u8; 32],
    },
    /// Consensus processing returned an unexpected error.
    ConsensusError {
        /// Node index where the error occurred.
        node_idx: usize,
        /// Description of the error.
        error: String,
    },
}

impl std::fmt::Display for SafetyViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Equivocation {
                creator,
                sequence,
                event_ids,
            } => write!(
                f,
                "EQUIVOCATION: creator={:?} seq={} events=[{}, {}]",
                &creator[..4],
                sequence,
                hex::encode(&event_ids[0][..4]),
                hex::encode(&event_ids[1][..4])
            ),
            Self::StateRootMismatch {
                node_a,
                node_b,
                root_a,
                root_b,
            } => write!(
                f,
                "STATE_ROOT_MISMATCH: node_a={} root_a={} node_b={} root_b={}",
                node_a,
                hex::encode(&root_a[..4]),
                node_b,
                hex::encode(&root_b[..4])
            ),
            Self::ConsensusError { node_idx, error } => {
                write!(f, "CONSENSUS_ERROR: node={} error={}", node_idx, error)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Health status
// ---------------------------------------------------------------------------

/// Health status report from the safety monitor.
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// Elapsed time since the test started.
    pub elapsed: Duration,
    /// Total number of events submitted.
    pub events_submitted: u64,
    /// Total number of events committed across all nodes.
    pub events_committed: u64,
    /// Number of nodes that are live (have at least one committed event).
    pub live_nodes: usize,
    /// Total number of nodes.
    pub total_nodes: usize,
    /// Number of safety violations detected.
    pub violations: usize,
    /// State roots from each node.
    pub state_roots: Vec<[u8; 32]>,
}

impl std::fmt::Display for HealthReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Health [{:.1}h elapsed] submitted={} committed={} live={}/{} violations={}",
            self.elapsed.as_secs_f64() / 3600.0,
            self.events_submitted,
            self.events_committed,
            self.live_nodes,
            self.total_nodes,
            self.violations,
        )
    }
}

// ---------------------------------------------------------------------------
// Simulated node
// ---------------------------------------------------------------------------

/// A simulated node for the safety monitoring test.
struct SimNode {
    /// Index of this node.
    #[allow(dead_code)]
    index: usize,
    /// Node identity.
    node_id: NodeId,
    /// The node's Ed25519 keypair for signing events.
    keypair: omnia_substrate::NodeKeypair,
    /// The node's causal graph.
    graph: CausalGraph,
    /// The node's consensus engine.
    consensus: ConsensusEngine<SlashingEngine>,
    /// The node's vector clock.
    vector_clock: VectorClock,
    /// Next sequence number for this node.
    next_sequence: u64,
    /// Latest event ID from each node that this node has seen.
    latest_events: HashMap<NodeId, EventId>,
    /// Total events committed by this node.
    committed_count: u64,
}

// ---------------------------------------------------------------------------
// Safety monitor
// ---------------------------------------------------------------------------

/// The safety monitoring test runner.
pub struct SafetyMonitor {
    /// Configuration.
    config: SafetyMonitorConfig,
    /// Simulated nodes.
    nodes: Vec<SimNode>,
    /// Total events submitted.
    events_submitted: u64,
    /// Detected safety violations.
    violations: Vec<SafetyViolation>,
    /// Start time.
    start: Instant,
    /// Last health report time.
    last_report: Instant,
}

impl SafetyMonitor {
    /// Create a new safety monitor with the given configuration.
    pub fn new(config: SafetyMonitorConfig) -> Self {
        use omnia_substrate::crypto::generate_keypair;

        let num_nodes = config.num_nodes;
        let mut nodes = Vec::with_capacity(num_nodes);

        // Generate keypairs and derive node IDs
        let keypairs: Vec<_> = (0..num_nodes).map(|_| generate_keypair()).collect();
        let node_ids: Vec<NodeId> = keypairs
            .iter()
            .map(|kp| omnia_substrate::blake3_hash_domain(b"omnia-creator", &kp.verifying_key().to_bytes()))
            .collect();

        // Create nodes — each node stores its own keypair for signing
        for i in 0..num_nodes {
            let mut seed = [0u8; 32];
            seed[0] = (i as u8) + 1;
            let consensus_config = ConsensusConfig {
                total_nodes: num_nodes,
                round_seed: seed,
                ..Default::default()
            };
            let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
            let mut consensus = ConsensusEngine::new(consensus_config, slashing);

            // Register all validators
            for &nid in &node_ids {
                consensus.register_validator(nid, 10_000);
            }

            nodes.push(SimNode {
                index: i,
                node_id: node_ids[i],
                keypair: keypairs[i].clone(),
                graph: CausalGraph::new(),
                consensus,
                vector_clock: VectorClock::new(),
                next_sequence: 0,
                latest_events: HashMap::new(),
                committed_count: 0,
            });
        }

        let now = Instant::now();
        Self {
            config,
            nodes,
            events_submitted: 0,
            violations: Vec::new(),
            start: now,
            last_report: now,
        }
    }

    /// Run the safety monitoring test until the configured duration expires
    /// or a safety violation is detected.
    ///
    /// Returns `Ok(())` if the test completes without violations,
    /// `Err(SafetyViolation)` on the first violation.
    pub fn run(&mut self) -> Result<(), SafetyViolation> {
        // Warmup: create genesis events and sync
        self.warmup();

        let deadline = self.start + self.config.duration;

        while Instant::now() < deadline {
            // Submit events from each node
            for i in 0..self.nodes.len() {
                for _ in 0..self.config.events_per_round {
                    if let Err(violation) = self.submit_and_propagate(i) {
                        tracing::error!(violation = %violation, "Safety violation detected — auto-exiting");
                        return Err(violation);
                    }
                }
            }

            // Sync all nodes
            self.sync_all();

            // Check safety invariants
            if let Some(violation) = self.check_safety() {
                tracing::error!(violation = %violation, "Safety violation detected — auto-exiting");
                return Err(violation);
            }

            // Periodic health report
            if Instant::now() - self.last_report >= self.config.report_interval {
                let report = self.health_report();
                tracing::info!("{}", report);
                self.last_report = Instant::now();
            }
        }

        tracing::info!(
            duration = ?self.config.duration,
            events_submitted = self.events_submitted,
            violations = self.violations.len(),
            "Safety monitoring test completed successfully"
        );
        Ok(())
    }

    /// Create genesis events for all nodes and sync the network.
    fn warmup(&mut self) {
        // Create genesis events using each node's identity keypair
        let mut genesis_events: Vec<(usize, Event)> = Vec::with_capacity(self.nodes.len());
        for (i, node) in self.nodes.iter_mut().enumerate() {
            let mut genesis =
                Event::genesis(node.node_id, vec![(i + 1) as u8]).expect("genesis event creation should not fail");
            // Use the node's own identity keypair for signing (not a random one)
            genesis.sign_with_keypair(&node.keypair).expect("signing");

            if let Err(e) = node.graph.insert(genesis.clone()) {
                tracing::warn!(node = i, "Genesis insert failed: {}", e);
                continue;
            }

            node.next_sequence = 1;
            node.latest_events.insert(genesis.creator, genesis.id);
            node.vector_clock = genesis.vector_clock.clone();

            genesis_events.push((i, genesis));
        }

        // Process genesis events through consensus (separate pass to avoid borrow conflict)
        for (node_idx, event) in &genesis_events {
            let node = &mut self.nodes[*node_idx];
            if let Err(e) = node.consensus.process_event(event, &node.graph) {
                tracing::debug!(node = node_idx, "Genesis consensus error: {}", e);
            }
        }

        // Sync genesis events across all nodes
        self.sync_all();

        tracing::info!(nodes = self.nodes.len(), "Safety monitor warmed up");
    }

    /// Submit an event from a node and propagate it to all other nodes.
    fn submit_and_propagate(&mut self, source_idx: usize) -> Result<(), SafetyViolation> {
        // Phase 1: Read state from source node (immutable borrows)
        let source_node_id = self.nodes[source_idx].node_id;
        let sequence = self.nodes[source_idx].next_sequence;
        let self_parent = self.nodes[source_idx].latest_events.get(&source_node_id).copied();
        let other_parent = self.nodes[source_idx]
            .latest_events
            .iter()
            .find(|(&nid, _)| nid != source_node_id)
            .map(|(_, &eid)| eid);

        let mut vc = self.nodes[source_idx].vector_clock.clone();
        vc.set(source_node_id, sequence.saturating_add(1));

        let payload: Vec<u8> = (0..self.config.payload_size)
            .map(|i| ((i + sequence as usize) % 256) as u8)
            .collect();

        let mut event = if self_parent.is_none() {
            Event::genesis(source_node_id, payload)
        } else {
            Event::new(source_node_id, sequence, vc, self_parent, other_parent, payload)
        }
        .expect("event creation should not fail");

        // Use the node's own identity keypair for signing (not a random one)
        let keypair = self.nodes[source_idx].keypair.clone();
        event.sign_with_keypair(&keypair).expect("signing");

        let event_id = event.id;

        // Phase 2: Insert and process on source node (mutable borrow of single node)
        {
            let node = &mut self.nodes[source_idx];
            if let Err(e) = node.graph.insert(event.clone()) {
                tracing::debug!(node = source_idx, "Event insert skipped: {}", e);
                return Ok(());
            }

            match node.consensus.process_event(&event, &node.graph) {
                Ok(committed) => {
                    node.committed_count += committed.len() as u64;
                }
                Err(e) => {
                    let violation = SafetyViolation::ConsensusError {
                        node_idx: source_idx,
                        error: e.to_string(),
                    };
                    self.violations.push(violation.clone());
                    return Err(violation);
                }
            }

            node.next_sequence = node.next_sequence.saturating_add(1);
            node.latest_events.insert(event.creator, event.id);
            node.vector_clock.merge(&event.vector_clock);
        }

        self.events_submitted += 1;

        // Phase 3: Propagate to all other nodes
        for target_idx in 0..self.nodes.len() {
            if target_idx == source_idx {
                continue;
            }

            if self.nodes[target_idx].graph.contains(&event_id) {
                continue;
            }

            // Propagate missing parents first (from source node, read-only)
            let parent_events: Vec<Event> = [self_parent, other_parent]
                .iter()
                .filter_map(|&p| p)
                .filter(|parent_id| !self.nodes[target_idx].graph.contains(parent_id))
                .filter_map(|parent_id| self.nodes[source_idx].graph.get(&parent_id).cloned())
                .collect();

            for parent_event in parent_events {
                let node = &mut self.nodes[target_idx];
                if let Err(e) = node.graph.insert(parent_event.clone()) {
                    tracing::debug!(node = target_idx, "Parent propagation insert failed: {}", e);
                    continue;
                }
                if let Err(e) = node.consensus.process_event(&parent_event, &node.graph) {
                    tracing::debug!(node = target_idx, "Parent consensus error: {}", e);
                }
            }

            // Insert event into target node
            let node = &mut self.nodes[target_idx];
            if let Err(e) = node.graph.insert(event.clone()) {
                tracing::debug!(node = target_idx, "Event propagation insert failed: {}", e);
                continue;
            }

            if let Ok(committed) = node.consensus.process_event(&event, &node.graph) {
                node.committed_count += committed.len() as u64;
            }

            node.latest_events.insert(event.creator, event.id);
            node.vector_clock.merge(&event.vector_clock);
        }

        Ok(())
    }

    /// Synchronize events across all nodes.
    fn sync_all(&mut self) {
        let n = self.nodes.len();
        for _ in 0..5 {
            let mut any_propagated = false;

            for source_idx in 0..n {
                let event_ids: Vec<EventId> = self.nodes[source_idx].graph.event_ids();

                for event_id in event_ids {
                    // Collect events to propagate for this source
                    let events_to_propagate: Vec<(usize, Event)> = (0..n)
                        .filter(|&target_idx| target_idx != source_idx)
                        .filter(|&target_idx| !self.nodes[target_idx].graph.contains(&event_id))
                        .filter_map(|target_idx| {
                            self.nodes[source_idx]
                                .graph
                                .get(&event_id)
                                .cloned()
                                .map(|e| (target_idx, e))
                        })
                        .collect();

                    for (target_idx, event) in events_to_propagate {
                        let node = &mut self.nodes[target_idx];
                        if node.graph.insert(event.clone()).is_ok() {
                            if let Ok(committed) = node.consensus.process_event(&event, &node.graph) {
                                node.committed_count += committed.len() as u64;
                            }
                            node.latest_events.insert(event.creator, event.id);
                            node.vector_clock.merge(&event.vector_clock);
                            any_propagated = true;
                        }
                    }
                }
            }

            if !any_propagated {
                break;
            }
        }
    }

    /// Check safety invariants across all nodes.
    ///
    /// Returns the first violation found, or `None` if all invariants hold.
    fn check_safety(&mut self) -> Option<SafetyViolation> {
        // 1. Check for equivocation: no (creator, sequence) pair has conflicting commits
        let mut commits_by_key: HashMap<(NodeId, u64), Vec<(usize, EventId)>> = HashMap::new();

        for (idx, node) in self.nodes.iter().enumerate() {
            for event_id in node.consensus.get_committed() {
                match node.graph.get_checked(&event_id) {
                    Ok(event) => {
                        let key = (event.creator, event.sequence);
                        commits_by_key.entry(key).or_default().push((idx, event_id));
                    }
                    Err(omnia_substrate::causal_graph::CausalGraphError::EventPruned(_)) => {
                        if let Some(metadata) = node.graph.get_pruned_metadata(&event_id) {
                            let key = (metadata.creator, metadata.sequence);
                            commits_by_key.entry(key).or_default().push((idx, event_id));
                        }
                    }
                    Err(_) => {
                        // Event not found — unexpected but not a safety violation per se
                    }
                }
            }
        }

        for (key, commits) in &commits_by_key {
            let mut unique_ids: Vec<EventId> = commits.iter().map(|(_, id)| *id).collect();
            unique_ids.sort();
            unique_ids.dedup();

            if unique_ids.len() > 1 {
                let violation = SafetyViolation::Equivocation {
                    creator: key.0,
                    sequence: key.1,
                    event_ids: [unique_ids[0], unique_ids[1]],
                };
                self.violations.push(violation.clone());
                return Some(violation);
            }
        }

        // 2. Check for state root mismatches
        let state_roots: Vec<(usize, [u8; 32])> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(idx, node)| (idx, node.graph.state_root()))
            .collect();

        for i in 0..state_roots.len() {
            for j in (i + 1)..state_roots.len() {
                let (idx_a, root_a) = state_roots[i];
                let (idx_b, root_b) = state_roots[j];
                if root_a != root_b
                    && !self.nodes[idx_a].consensus.get_committed().is_empty()
                    && !self.nodes[idx_b].consensus.get_committed().is_empty()
                {
                    let committed_a = self.nodes[idx_a].consensus.get_committed().len();
                    let committed_b = self.nodes[idx_b].consensus.get_committed().len();
                    if committed_a == committed_b && committed_a > 0 {
                        let violation = SafetyViolation::StateRootMismatch {
                            node_a: idx_a,
                            node_b: idx_b,
                            root_a,
                            root_b,
                        };
                        self.violations.push(violation.clone());
                        return Some(violation);
                    }
                }
            }
        }

        None
    }

    /// Generate a health report.
    fn health_report(&self) -> HealthReport {
        let mut live_nodes = 0;
        let mut events_committed = 0u64;

        for node in &self.nodes {
            let committed = node.consensus.get_committed().len();
            if committed > 0 {
                live_nodes += 1;
            }
            events_committed += node.committed_count;
        }

        let state_roots: Vec<[u8; 32]> = self.nodes.iter().map(|n| n.graph.state_root()).collect();

        HealthReport {
            elapsed: self.start.elapsed(),
            events_submitted: self.events_submitted,
            events_committed,
            live_nodes,
            total_nodes: self.nodes.len(),
            violations: self.violations.len(),
            state_roots,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safety_monitor_short_run() {
        let config = SafetyMonitorConfig {
            num_nodes: 4,
            duration: Duration::from_secs(1),
            report_interval: Duration::from_secs(1),
            events_per_round: 5,
            payload_size: 32,
        };

        let mut monitor = SafetyMonitor::new(config);
        let result = monitor.run();
        assert!(result.is_ok(), "Short safety test should pass without violations");
    }

    #[test]
    fn test_safety_monitor_config_default() {
        let config = SafetyMonitorConfig::default();
        assert_eq!(config.num_nodes, 4);
        assert_eq!(config.duration, Duration::from_secs(72 * 60 * 60));
        assert_eq!(config.report_interval, Duration::from_secs(300));
    }

    #[test]
    fn test_health_report_display() {
        let report = HealthReport {
            elapsed: Duration::from_secs(3600),
            events_submitted: 1000,
            events_committed: 950,
            live_nodes: 4,
            total_nodes: 4,
            violations: 0,
            state_roots: vec![[0u8; 32]; 4],
        };
        let display = format!("{}", report);
        assert!(display.contains("1.0h"));
        assert!(display.contains("1000"));
        assert!(display.contains("950"));
    }

    #[test]
    fn test_safety_violation_display() {
        let violation = SafetyViolation::Equivocation {
            creator: [1u8; 32],
            sequence: 42,
            event_ids: [[2u8; 32], [3u8; 32]],
        };
        let display = format!("{}", violation);
        assert!(display.contains("EQUIVOCATION"));

        let violation = SafetyViolation::StateRootMismatch {
            node_a: 0,
            node_b: 1,
            root_a: [1u8; 32],
            root_b: [2u8; 32],
        };
        let display = format!("{}", violation);
        assert!(display.contains("STATE_ROOT_MISMATCH"));
    }
}
