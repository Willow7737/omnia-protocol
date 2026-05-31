//! Chaos testing framework for the Omnia Protocol.
//!
//! This framework simulates real-world failure modes including network
//! partitions, message loss, node crashes, and Byzantine behavior.
//! It verifies that the protocol maintains safety (no conflicting commits)
//! and liveness (events eventually finalize) under adverse conditions.
//!
//! # Architecture
//!
//! The framework works at the Rust API level — not over the network — calling
//! substrate methods directly on each simulated node. Failures are injected by
//! selectively not propagating events between nodes.
//!
//! Each [`ChaosNode`] owns its own [`CausalGraph`] and [`ConsensusEngine`],
//! giving full control over what each node can see. The [`ChaosNetwork`]
//! orchestrates the simulated gossip, respecting partitions, drop rates,
//! and crash status.
//!
//! # Example
//!
//! ```ignore
//! use omnia_chaos_tests::ChaosNetwork;
//!
//! let mut net = ChaosNetwork::new(4);
//! net.warmup();
//!
//! net.partition(&[0, 1], &[2, 3]);
//! // ... submit events ...
//! net.heal();
//!
//! assert!(net.check_safety());
//! assert!(net.check_liveness());
//! ```

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod full_chaos_suite;
pub mod gossip_chaos;
pub mod load_test;
pub mod safety_monitoring;
pub mod stability_test;

use omnia_substrate::{
    generate_keypair, CausalGraph, ConsensusConfig, ConsensusEngine, Event, EventId, NodeId, NodeKeypair, SlashOutcome,
    SlashingEngine, VectorClock, DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD,
};
use rand::Rng;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// ChaosNode
// ---------------------------------------------------------------------------

/// A single node in the chaos testing network.
///
/// Each node has its own causal graph, consensus engine, and keypair.
/// The node can be crashed (simulating process failure) and can have
/// a configurable message drop rate.
///
/// A separate [`SlashingEngine`] is maintained for liveness tracking,
/// since the `ConsensusEngine`'s internal slashing engine is not
/// directly accessible for `check_liveness` calls.
pub struct ChaosNode {
    /// Index of this node in the network (0-based).
    pub index: usize,
    /// The node's identity in the protocol.
    pub node_id: NodeId,
    /// The node's Ed25519 keypair for signing events.
    pub keypair: NodeKeypair,
    /// The node's causal graph (DAG of events).
    pub graph: CausalGraph,
    /// The node's consensus engine.
    pub consensus: ConsensusEngine,
    /// Separate slashing engine for liveness tracking.
    ///
    /// The `ConsensusEngine` has its own internal `SlashingEngine` for
    /// equivocation detection during `process_event`, but it does not
    /// expose `check_liveness`. This separate engine is used for
    /// liveness violation detection in chaos tests.
    pub slashing: SlashingEngine,
    /// Whether this node is currently crashed.
    pub crashed: bool,
    /// Probability of dropping an incoming message (0.0 to 1.0).
    ///
    /// This is a test-only parameter and is acceptable as `f64` here
    /// because it is not used in consensus-critical code paths.
    pub drop_rate: f64,
    /// The node's next sequence number for event creation.
    pub next_sequence: u64,
    /// The node's current vector clock.
    pub vector_clock: VectorClock,
    /// Latest event ID from each node that this node has seen.
    pub latest_events: HashMap<NodeId, EventId>,
}

impl ChaosNode {
    /// Create a new chaos node with the given index, identity, and keypair.
    ///
    /// The node starts with an empty graph and default consensus configuration
    /// for `total_nodes` validators.
    ///
    /// # Arguments
    ///
    /// * `index` — 0-based index of this node in the network.
    /// * `keypair` — Ed25519 signing keypair for this node (node_id is derived as blake3(pubkey)).
    /// * `total_nodes` — Total number of validators (used for consensus thresholds).
    // Kept for potential future use in standalone chaos node construction; currently
    // ChaosNetwork::new() builds nodes inline instead of calling this constructor.
    #[allow(dead_code)]
    pub fn new(index: usize, keypair: NodeKeypair, total_nodes: usize) -> Self {
        // Derive node_id from the keypair: node_id = blake3_hash_domain("omnia-creator", pubkey)
        // This matches Event::sign_with_keypair() which sets creator = blake3_hash_domain("omnia-creator", pubkey)
        let node_id: NodeId =
            omnia_substrate::blake3_hash_domain(b"omnia-creator", &keypair.verifying_key().to_bytes());
        let mut seed = [0u8; 32];
        seed[0] = (index as u8) + 1; // Non-zero to avoid debug-build panic
        let config = ConsensusConfig {
            total_nodes,
            round_seed: seed,
            ..Default::default()
        };
        let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut consensus = ConsensusEngine::new(config, slashing.clone());
        consensus.register_validator(node_id, 10_000);

        let mut slashing_separate = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        slashing_separate.register_validator(node_id, 10_000);

        Self {
            index,
            node_id,
            keypair,
            graph: CausalGraph::new(),
            consensus,
            slashing: slashing_separate,
            crashed: false,
            drop_rate: 0.0,
            next_sequence: 0,
            vector_clock: VectorClock::new(),
            latest_events: HashMap::new(),
        }
    }

    /// Get the node's latest event ID (its self-parent for the next event).
    pub fn last_event_id(&self) -> Option<EventId> {
        self.latest_events.get(&self.node_id).copied()
    }
}

// ---------------------------------------------------------------------------
// ChaosNetwork
// ---------------------------------------------------------------------------

/// A simulated network of Omnia nodes for chaos testing.
///
/// The `ChaosNetwork` provides methods to:
/// - Submit events and simulate gossip propagation
/// - Create network partitions between groups of nodes
/// - Crash and restart nodes
/// - Set per-node message drop rates
/// - Verify safety and liveness invariants
///
/// # Failures
///
/// Failures are injected by controlling which events are propagated between
/// nodes. This is done through three mechanisms:
///
/// 1. **Partitions** — Groups of nodes that cannot communicate. Messages
///    between partitioned groups are silently dropped.
/// 2. **Crash** — A crashed node cannot send or receive messages. Its state
///    is preserved and can be restored on restart.
/// 3. **Drop rate** — Per-node probability of dropping incoming messages.
///    Simulates unreliable network links.
pub struct ChaosNetwork {
    /// The nodes in the network.
    pub nodes: Vec<ChaosNode>,
    /// Active partitions: each tuple contains two groups that cannot communicate.
    partitions: Vec<(HashSet<usize>, HashSet<usize>)>,
}

impl ChaosNetwork {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Create a new chaos network with `n` nodes.
    ///
    /// Each node gets a unique `NodeId` derived from BLAKE3(pubkey), an Ed25519 keypair,
    /// and is registered as a validator on all nodes' consensus engines.
    /// Genesis events are created for each node and synced across the network.
    pub fn new(n: usize) -> Self {
        // Generate keypairs first, then derive node IDs from BLAKE3(pubkey).
        // After Sprint 4 Task A1, Event::sign_with_keypair() sets
        // creator = blake3(creator_pubkey), so node IDs must match this
        // derivation for equivocation detection and slashing to work correctly.
        let mut keypairs: Vec<NodeKeypair> = Vec::with_capacity(n);
        for _ in 0..n {
            keypairs.push(generate_keypair());
        }

        // Derive node IDs from the domain-separated BLAKE3 hash of each keypair's public key
        let node_ids: Vec<NodeId> = keypairs
            .iter()
            .map(|kp| omnia_substrate::blake3_hash_domain(b"omnia-creator", &kp.verifying_key().to_bytes()))
            .collect();

        // Create nodes, each with all validators registered
        let mut nodes = Vec::with_capacity(n);
        for i in 0..n {
            let mut seed = [0u8; 32];
            seed[0] = (i as u8) + 1; // Non-zero to avoid debug-build panic
            let config = ConsensusConfig {
                total_nodes: n,
                round_seed: seed,
                ..Default::default()
            };
            let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
            let mut consensus = ConsensusEngine::new(config, slashing.clone());
            let mut slashing_separate =
                SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);

            // Register all nodes as validators on this node's engines
            for &nid in &node_ids {
                consensus.register_validator(nid, 10_000);
                slashing_separate.register_validator(nid, 10_000);
            }

            nodes.push(ChaosNode {
                index: i,
                node_id: node_ids[i],
                keypair: keypairs[i].clone(),
                graph: CausalGraph::new(),
                consensus,
                slashing: slashing_separate,
                crashed: false,
                drop_rate: 0.0,
                next_sequence: 0,
                vector_clock: VectorClock::new(),
                latest_events: HashMap::new(),
            });
        }

        // Create genesis events for each node
        // We collect (node_index, event) pairs, then process them in a
        // separate pass to avoid borrowing `nodes` mutably and immutably
        // at the same time.
        let mut genesis_events: Vec<(usize, Event)> = Vec::with_capacity(n);
        for i in 0..n {
            let mut event =
                Event::genesis(node_ids[i], vec![(i + 1) as u8]).expect("genesis event creation should not fail");
            event.sign_with_keypair(&keypairs[i]).expect("signing");

            // Insert into the node's own graph (graph only needs &mut graph)
            if let Err(e) = nodes[i].graph.insert(event.clone()) {
                panic!("Genesis insert failed for node {i}: {e}");
            }

            // Track state — use event.creator (which is blake3(pubkey) after signing)
            nodes[i].next_sequence = 1;
            nodes[i].latest_events.insert(event.creator, event.id);
            nodes[i].vector_clock = event.vector_clock.clone();

            genesis_events.push((i, event));
        }

        // Process genesis events through each node's consensus engine
        // This must be done in a separate loop to avoid the borrow conflict
        // between `node.consensus` (mut) and `node.graph` (immut).
        for (node_idx, event) in &genesis_events {
            let node = &mut nodes[*node_idx];
            if let Err(e) = node.consensus.process_event(event, &node.graph) {
                tracing::warn!(node = node_idx, "Genesis consensus error: {}", e);
            }
        }

        let mut network = Self {
            nodes,
            partitions: Vec::new(),
        };

        // Propagate genesis events so all nodes know about each other
        network.sync_all();

        tracing::info!(nodes = n, "ChaosNetwork created and synced");

        network
    }

    // -----------------------------------------------------------------------
    // Failure injection
    // -----------------------------------------------------------------------

    /// Simulate a network partition between two groups of nodes.
    ///
    /// After calling this method, nodes in `group_a` cannot send messages
    /// to nodes in `group_b` and vice versa. Nodes not in either group
    /// can communicate with both sides.
    ///
    /// Multiple partitions can be active simultaneously.
    ///
    /// # Arguments
    ///
    /// * `group_a` — Indices of nodes in the first partition.
    /// * `group_b` — Indices of nodes in the second partition.
    pub fn partition(&mut self, group_a: &[usize], group_b: &[usize]) {
        let a: HashSet<usize> = group_a.iter().copied().collect();
        let b: HashSet<usize> = group_b.iter().copied().collect();
        self.partitions.push((a, b));
        tracing::info!(
            group_a = ?group_a,
            group_b = ?group_b,
            "Network partition created"
        );
    }

    /// Heal all network partitions.
    ///
    /// After calling this method, all nodes can communicate freely again.
    /// A full sync is performed to propagate any events that were blocked
    /// by the partition.
    pub fn heal(&mut self) {
        self.partitions.clear();
        tracing::info!("All partitions healed, syncing nodes");
        self.sync_all();
    }

    /// Simulate a node crash.
    ///
    /// A crashed node cannot send or receive messages. Its internal state
    /// (graph, consensus) is preserved and can be restored with
    /// [`restart_node()`](Self::restart_node).
    ///
    /// # Arguments
    ///
    /// * `id` — Index of the node to crash.
    ///
    /// # Errors
    ///
    /// Returns an error if the node index is out of bounds or the node is
    /// already crashed.
    pub fn crash_node(&mut self, id: usize) -> anyhow::Result<()> {
        if id >= self.nodes.len() {
            return Err(anyhow::anyhow!("Node {id} does not exist"));
        }
        if self.nodes[id].crashed {
            return Err(anyhow::anyhow!("Node {id} is already crashed"));
        }
        self.nodes[id].crashed = true;
        tracing::info!(node = id, "Node crashed");
        Ok(())
    }

    /// Simulate a node restart after a crash.
    ///
    /// The node is marked as active again and receives a sync of all
    /// events it missed while crashed.
    ///
    /// # Arguments
    ///
    /// * `id` — Index of the node to restart.
    ///
    /// # Errors
    ///
    /// Returns an error if the node index is out of bounds or the node is
    /// not crashed.
    pub fn restart_node(&mut self, id: usize) -> anyhow::Result<()> {
        if id >= self.nodes.len() {
            return Err(anyhow::anyhow!("Node {id} does not exist"));
        }
        if !self.nodes[id].crashed {
            return Err(anyhow::anyhow!("Node {id} is not crashed"));
        }
        self.nodes[id].crashed = false;
        tracing::info!(node = id, "Node restarted, syncing missed events");

        // Sync missed events from other active nodes
        let n = self.nodes.len();
        for source_idx in 0..n {
            if source_idx == id || self.nodes[source_idx].crashed {
                continue;
            }
            let event_ids: Vec<EventId> = self.nodes[source_idx].graph.event_ids();
            for event_id in event_ids {
                if self.nodes[id].graph.contains(&event_id) {
                    continue;
                }
                if let Ok(events) = self.collect_missing_ancestors(source_idx, event_id, id) {
                    for event in events {
                        if let Err(e) = self.insert_event_to_node(id, event) {
                            tracing::debug!(node = id, "Restart sync insert failed: {}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Set the message drop rate for a specific node.
    ///
    /// The drop rate is the probability that an incoming message to this
    /// node will be silently discarded. A rate of 0.0 means all messages
    /// are delivered; a rate of 1.0 means all messages are dropped.
    ///
    /// Note: `f64` is acceptable here because this parameter is only used
    /// for test injection, not in consensus-critical code paths.
    ///
    /// # Arguments
    ///
    /// * `id` — Index of the node.
    /// * `rate` — Drop probability (0.0 to 1.0).
    pub fn set_drop_rate(&mut self, id: usize, rate: f64) {
        if id < self.nodes.len() {
            self.nodes[id].drop_rate = rate.clamp(0.0, 1.0);
            tracing::info!(node = id, drop_rate = rate, "Message drop rate set");
        }
    }

    // -----------------------------------------------------------------------
    // Event submission
    // -----------------------------------------------------------------------

    /// Submit an event from a specific node and gossip it to the network.
    ///
    /// Creates a properly signed event with the given payload, inserts it
    /// into the node's graph, processes it through consensus, and then
    /// gossips it to all reachable nodes (respecting partitions, crash
    /// status, and drop rates).
    ///
    /// # Arguments
    ///
    /// * `node_id` — Index of the node submitting the event.
    /// * `payload` — Application-specific payload bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the node is crashed or the event cannot be
    /// inserted into the graph.
    pub fn submit_event(&mut self, node_id: usize, payload: Vec<u8>) -> anyhow::Result<()> {
        if node_id >= self.nodes.len() {
            return Err(anyhow::anyhow!("Node {node_id} does not exist"));
        }
        if self.nodes[node_id].crashed {
            return Err(anyhow::anyhow!("Node {node_id} is crashed"));
        }

        // Phase 1: Create the event (immutable reads)
        let event_id;
        let event = {
            let node = &self.nodes[node_id];

            let self_parent = node.latest_events.get(&node.node_id).copied();
            let other_parent = node
                .latest_events
                .iter()
                .find(|(&nid, _)| nid != node.node_id)
                .map(|(_, &eid)| eid);

            let sequence = node.next_sequence;

            let mut vc = node.vector_clock.clone();
            vc.set(node.node_id, sequence.saturating_add(1));

            let mut event = if self_parent.is_none() {
                Event::genesis(node.node_id, payload)
            } else {
                Event::new(node.node_id, sequence, vc, self_parent, other_parent, payload)
            }
            .map_err(|e| anyhow::anyhow!("Event creation failed: {e}"))?;
            event.sign_with_keypair(&node.keypair).expect("signing");

            event_id = event.id;
            event
        };

        // Phase 2: Insert into node's graph and process through consensus
        {
            let node = &mut self.nodes[node_id];
            node.graph.insert(event.clone())?;

            if let Err(e) = node.consensus.process_event(&event, &node.graph) {
                tracing::debug!(node = node_id, "Consensus error on submit: {}", e);
            }

            node.next_sequence = node.next_sequence.saturating_add(1);
            node.latest_events.insert(event.creator, event.id);
            node.vector_clock.merge(&event.vector_clock);
        }

        // Phase 3: Gossip to all reachable nodes
        self.gossip_event(node_id, event_id)?;

        Ok(())
    }

    /// Inject a pre-created event directly into a specific node.
    ///
    /// This bypasses the normal event creation flow and is intended for
    /// testing adversarial scenarios such as equivocation. The event is
    /// inserted into the target node's graph and processed through its
    /// consensus engine, then gossiped to all reachable nodes.
    ///
    /// # Arguments
    ///
    /// * `target_idx` — Index of the node to inject the event into.
    /// * `event` — The pre-created, signed event to inject.
    ///
    /// # Errors
    ///
    /// Returns an error if the target node is crashed or the event cannot
    /// be inserted.
    pub fn inject_event(&mut self, target_idx: usize, event: Event) -> anyhow::Result<()> {
        if target_idx >= self.nodes.len() {
            return Err(anyhow::anyhow!("Node {target_idx} does not exist"));
        }
        if self.nodes[target_idx].crashed {
            return Err(anyhow::anyhow!("Node {target_idx} is crashed"));
        }

        // Propagate any missing parents first
        let self_parent = event.self_parent;
        let other_parent = event.other_parent;
        let event_id = event.id;

        // Try to find parent events from any non-crashed node and propagate them
        for parent_id in [self_parent, other_parent].iter().filter_map(|&p| p) {
            if self.nodes[target_idx].graph.contains(&parent_id) {
                continue;
            }
            // Search for the parent in any active node's graph
            for source_idx in 0..self.nodes.len() {
                if source_idx == target_idx || self.nodes[source_idx].crashed {
                    continue;
                }
                if self.nodes[source_idx].graph.contains(&parent_id) {
                    if let Ok(ancestors) = self.collect_missing_ancestors(source_idx, parent_id, target_idx) {
                        for ancestor in ancestors {
                            if let Err(e) = self.insert_event_to_node(target_idx, ancestor) {
                                tracing::debug!(node = target_idx, "Parent propagation failed: {}", e);
                            }
                        }
                    }
                    break;
                }
            }
        }

        // Insert the event
        self.insert_event_to_node(target_idx, event)?;

        // Gossip from target to all reachable nodes
        self.gossip_event(target_idx, event_id)?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Invariant checks
    // -----------------------------------------------------------------------

    /// Verify safety: no conflicting commits across nodes.
    ///
    /// A safety violation occurs when two different events with the same
    /// `(creator, sequence)` pair are both committed on any node in the
    /// network. This would indicate that the protocol has allowed
    /// conflicting histories to be finalized.
    ///
    /// # Returns
    ///
    /// `true` if no conflicting commits are detected, `false` otherwise.
    pub fn check_safety(&self) -> bool {
        // Collect all committed events from all nodes, grouped by (creator, sequence)
        let mut commits_by_key: HashMap<(NodeId, u64), Vec<(usize, EventId)>> = HashMap::new();

        for (idx, node) in self.nodes.iter().enumerate() {
            for event_id in node.consensus.get_committed() {
                match node.graph.get_checked(&event_id) {
                    Ok(event) => {
                        let key = (event.creator, event.sequence);
                        commits_by_key.entry(key).or_default().push((idx, event_id));
                    }
                    Err(omnia_substrate::causal_graph::CausalGraphError::EventPruned(_)) => {
                        // Pruned events are expected — they were committed but later cleaned up.
                        // Account for them in safety calculations by checking metadata.
                        if let Some(metadata) = node.graph.get_pruned_metadata(&event_id) {
                            let key = (metadata.creator, metadata.sequence);
                            commits_by_key.entry(key).or_default().push((idx, event_id));
                        }
                    }
                    Err(_) => {
                        // Event not found — should not happen for committed events
                        tracing::warn!(
                            "Committed event {} not found in graph during safety check",
                            hex::encode(&event_id[..4])
                        );
                    }
                }
            }
        }

        // Check that no (creator, sequence) pair has conflicting event IDs
        for (key, commits) in &commits_by_key {
            let unique_ids: HashSet<EventId> = commits.iter().map(|(_, id)| *id).collect();
            if unique_ids.len() > 1 {
                tracing::error!(
                    creator = ?&key.0[..4],
                    sequence = key.1,
                    conflicting_count = unique_ids.len(),
                    "SAFETY VIOLATION: Conflicting commits detected"
                );
                return false;
            }
        }

        true
    }

    /// Verify liveness: at least some events have been finalized.
    ///
    /// Checks whether at least one node in the network has at least one
    /// committed event. A network with no committed events is not live.
    ///
    /// # Returns
    ///
    /// `true` if at least one committed event exists, `false` otherwise.
    pub fn check_liveness(&self) -> bool {
        for node in &self.nodes {
            if !node.consensus.get_committed().is_empty() {
                return true;
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Query helpers
    // -----------------------------------------------------------------------

    /// Returns the total number of committed events across all nodes.
    ///
    /// This counts each committed event once per node that has committed it.
    /// For a count of unique committed events, use a different method.
    pub fn committed_count(&self) -> usize {
        self.nodes.iter().map(|n| n.consensus.get_committed().len()).sum()
    }

    /// Returns the number of committed events for a specific node.
    ///
    /// # Arguments
    ///
    /// * `node_idx` — Index of the node to query.
    pub fn node_committed_count(&self, node_idx: usize) -> usize {
        if node_idx < self.nodes.len() {
            self.nodes[node_idx].consensus.get_committed().len()
        } else {
            0
        }
    }

    /// Check if a node has been slashed according to a specific node's consensus engine.
    ///
    /// # Arguments
    ///
    /// * `observer_idx` — Index of the node performing the check.
    /// * `offender_id` — The `NodeId` of the potentially slashed node.
    pub fn is_node_slashed(&self, observer_idx: usize, offender_id: &NodeId) -> bool {
        if observer_idx < self.nodes.len() {
            self.nodes[observer_idx].consensus.is_slashed(offender_id)
        } else {
            false
        }
    }

    /// Check liveness for a specific node using the node's slashing engine.
    ///
    /// Uses the separate [`SlashingEngine`] on the observer node (not the
    /// one embedded in `ConsensusEngine`) to detect inactivity violations.
    /// This is necessary because `ConsensusEngine` does not expose
    /// `check_liveness` directly.
    ///
    /// # Arguments
    ///
    /// * `observer_idx` — Index of the node performing the check.
    /// * `node` — `NodeId` of the node being checked.
    /// * `last_active_round` — Last round where the node participated.
    /// * `current_round` — Current consensus round.
    /// * `threshold` — Number of inactive rounds before a violation is triggered.
    ///
    /// # Returns
    ///
    /// `Some(SlashOutcome)` if a violation was detected, `None` otherwise.
    pub fn check_node_liveness(
        &mut self,
        observer_idx: usize,
        node: NodeId,
        last_active_round: u64,
        current_round: u64,
        threshold: u64,
    ) -> Option<SlashOutcome> {
        if observer_idx >= self.nodes.len() {
            return None;
        }
        self.nodes[observer_idx]
            .slashing
            .check_liveness(node, last_active_round, current_round, threshold)
    }

    // -----------------------------------------------------------------------
    // Network operations
    // -----------------------------------------------------------------------

    /// Create genesis events for all nodes and sync the network.
    ///
    /// Call this after construction to ensure all nodes have each other's
    /// genesis events in their graphs. This is necessary for consensus
    /// to make progress.
    ///
    /// Note: `ChaosNetwork::new()` already creates and syncs genesis events,
    /// so this method is only needed if you want to re-sync.
    pub fn warmup(&mut self) {
        tracing::info!("Running warmup — syncing all nodes");
        self.sync_all();
    }

    /// Submit heartbeat events from all active nodes to advance consensus.
    ///
    /// Each active node creates one event per round. This helps the
    /// consensus engine make progress by providing new witnesses and
    /// cross-references between nodes.
    ///
    /// # Arguments
    ///
    /// * `rounds` — Number of heartbeat rounds to run.
    pub fn advance(&mut self, rounds: usize) {
        for r in 0..rounds {
            for i in 0..self.nodes.len() {
                if !self.nodes[i].crashed {
                    let payload = vec![r as u8];
                    if let Err(e) = self.submit_event(i, payload) {
                        tracing::debug!(node = i, round = r, "Advance submit failed: {}", e);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Internal: network simulation
    // -----------------------------------------------------------------------

    /// Check whether two nodes can communicate (not crashed, not partitioned).
    fn can_communicate(&self, from: usize, to: usize) -> bool {
        if from >= self.nodes.len() || to >= self.nodes.len() {
            return false;
        }
        if self.nodes[from].crashed || self.nodes[to].crashed {
            return false;
        }
        for (group_a, group_b) in &self.partitions {
            let from_in_a = group_a.contains(&from);
            let from_in_b = group_b.contains(&from);
            let to_in_a = group_a.contains(&to);
            let to_in_b = group_b.contains(&to);
            if (from_in_a && to_in_b) || (from_in_b && to_in_a) {
                return false;
            }
        }
        true
    }

    /// Check whether a message should be delivered to a node (considering drop rate).
    fn should_deliver(&self, target_idx: usize) -> bool {
        let drop_rate = self.nodes[target_idx].drop_rate;
        if drop_rate <= 0.0 {
            return true;
        }
        if drop_rate >= 1.0 {
            return false;
        }
        rand::thread_rng().gen::<f64>() >= drop_rate
    }

    /// Gossip an event from a source node to all reachable nodes.
    ///
    /// Respects partitions, crash status, and message drop rates.
    fn gossip_event(&mut self, source_idx: usize, event_id: EventId) -> anyhow::Result<()> {
        let n = self.nodes.len();
        for target_idx in 0..n {
            if target_idx == source_idx {
                continue;
            }
            if !self.can_communicate(source_idx, target_idx) {
                continue;
            }
            if !self.should_deliver(target_idx) {
                tracing::debug!(from = source_idx, to = target_idx, "Message dropped due to drop rate");
                continue;
            }
            if let Err(e) = self.propagate_event(source_idx, event_id, target_idx) {
                tracing::debug!(from = source_idx, to = target_idx, "Gossip propagation failed: {}", e);
            }
        }
        Ok(())
    }

    /// Propagate an event (and its missing ancestors) from source to target.
    fn propagate_event(&mut self, source_idx: usize, event_id: EventId, target_idx: usize) -> anyhow::Result<()> {
        let events = self.collect_missing_ancestors(source_idx, event_id, target_idx)?;
        for event in events {
            self.insert_event_to_node(target_idx, event)?;
        }
        Ok(())
    }

    /// Collect all ancestors of `event_id` that `target` is missing, in
    /// topological order (parents first).
    ///
    /// This is a read-only operation (takes `&self`).
    fn collect_missing_ancestors(
        &self,
        source_idx: usize,
        event_id: EventId,
        target_idx: usize,
    ) -> anyhow::Result<Vec<Event>> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        self.collect_missing_ancestors_inner(source_idx, event_id, target_idx, &mut visited, &mut result)?;
        Ok(result)
    }

    /// Recursive helper for `collect_missing_ancestors`.
    ///
    /// Visits parents before children, so the result is in topological order.
    fn collect_missing_ancestors_inner(
        &self,
        source_idx: usize,
        event_id: EventId,
        target_idx: usize,
        visited: &mut HashSet<EventId>,
        result: &mut Vec<Event>,
    ) -> anyhow::Result<()> {
        if visited.contains(&event_id) {
            return Ok(());
        }
        visited.insert(event_id);

        // Skip if target already has this event
        if self.nodes[target_idx].graph.contains(&event_id) {
            return Ok(());
        }

        // Get the event from the source's graph
        let event = self.nodes[source_idx]
            .graph
            .get(&event_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Event {} not found in source node {}",
                    hex::encode(&event_id[..4]),
                    source_idx
                )
            })?
            .clone();

        // Recursively collect parents first
        if let Some(sp) = event.self_parent {
            self.collect_missing_ancestors_inner(source_idx, sp, target_idx, visited, result)?;
        }
        if let Some(op) = event.other_parent {
            self.collect_missing_ancestors_inner(source_idx, op, target_idx, visited, result)?;
        }

        // Double-check: target might have received it via parent propagation
        if !self.nodes[target_idx].graph.contains(&event_id) {
            result.push(event);
        }

        Ok(())
    }

    /// Insert a single event into a target node's graph and process it
    /// through the node's consensus engine.
    fn insert_event_to_node(&mut self, target_idx: usize, event: Event) -> anyhow::Result<()> {
        // Skip if already present
        if self.nodes[target_idx].graph.contains(&event.id) {
            return Ok(());
        }

        // Insert into graph
        self.nodes[target_idx]
            .graph
            .insert(event.clone())
            .map_err(|e| anyhow::anyhow!("Graph insert failed: {e}"))?;

        // Process through consensus and update tracking
        {
            let node = &mut self.nodes[target_idx];
            if let Err(e) = node.consensus.process_event(&event, &node.graph) {
                // NodeSlashed and other consensus errors are expected in chaos tests
                tracing::debug!(
                    node = target_idx,
                    event = ?&event.id[..4],
                    "Consensus error during propagation: {}",
                    e
                );
            }
            node.latest_events.insert(event.creator, event.id);
            node.vector_clock.merge(&event.vector_clock);
        }

        Ok(())
    }

    /// Synchronize all events between all reachable nodes.
    ///
    /// Runs multiple passes to ensure convergence. This is called after
    /// healing partitions and restarting nodes.
    fn sync_all(&mut self) {
        let n = self.nodes.len();

        for round in 0..10 {
            let mut any_propagated = false;

            for source_idx in 0..n {
                if self.nodes[source_idx].crashed {
                    continue;
                }

                // Collect event IDs from source before we start mutating
                let event_ids: Vec<EventId> = self.nodes[source_idx].graph.event_ids();

                for event_id in event_ids {
                    for target_idx in 0..n {
                        if target_idx == source_idx || self.nodes[target_idx].crashed {
                            continue;
                        }
                        if !self.can_communicate(source_idx, target_idx) {
                            continue;
                        }
                        if self.nodes[target_idx].graph.contains(&event_id) {
                            continue;
                        }

                        if let Ok(events) = self.collect_missing_ancestors(source_idx, event_id, target_idx) {
                            for event in events {
                                if let Ok(()) = self.insert_event_to_node(target_idx, event) {
                                    any_propagated = true;
                                }
                            }
                        }
                    }
                }
            }

            if !any_propagated {
                tracing::debug!(round = round, "Sync converged — no new events propagated");
                break;
            }
            tracing::debug!(round = round, "Sync round completed, more events to propagate");
        }
    }
}
