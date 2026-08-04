//! # Omnia Node Binary
//!
//! The `omnia-node` binary runs a full Omnia Protocol node with:
//!
//! - **Layer 1 Substrate** — causal graph consensus, slashing, and gossip
//! - **Layer 2 Shard Router** — domain-specific state machines with fee enforcement
//! - **Layer 5 Economics** — UBC token, governance, and useful-work rewards
//! - **REST API** — health checks, metrics, and full API surface
//!
//! # Startup Sequence
//!
//! 1. Parse CLI args (with env var overrides via `OMNIA_` prefix)
//! 2. Initialize structured logging (tracing)
//! 3. Create data directory if it doesn't exist
//! 4. Initialize substrate components (consensus, slashing, shard router)
//! 5. Start the HTTP server (axum)
//! 6. Wait for SIGINT/SIGTERM for graceful shutdown
//!
//! # Example
//!
//! ```sh
//! omnia-node --node-id 1 --http-port 8080 --log-level info
//! OMNIA_NODE_ID=2 OMNIA_HTTP_PORT=8081 omnia-node
//! ```

use anyhow::{Context, Result};
use clap::Parser;
use omnia_adapters::SettlementAdapter;
use omnia_economics::EconomicsState;
#[cfg(feature = "network")]
use omnia_network::{Multiaddr, NetworkConfig, OmniaNetwork};
use omnia_node::config::{CliArgs, CliCommand, NodeConfig};
// C-14: PipelineRouter import removed — workers were dead code.
// The pipeline module is retained for future implementation.
use omnia_node::state::AppState;
#[cfg(feature = "metrics")]
use omnia_node::state::NodeMetrics;
use omnia_shards::{
    BiologicalShard, ComputationalShard, EconomicsShard, FeeSchedule, FinancialShard, IdentityShard, MutexShardRouter,
    PhysicalShard, ShardRouter,
};
use omnia_substrate::{Substrate, SubstrateConfig};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Parse CLI arguments and dispatch subcommands
    let cli = CliArgs::parse();

    // Handle subcommands before starting the node
    if let Some(command) = cli.command {
        match command {
            CliCommand::Keygen { output_dir, passphrase } => {
                return run_keygen(&output_dir, passphrase.as_deref());
            }
            #[cfg(feature = "zk")]
            CliCommand::SetupContribute {
                degree,
                min_participants,
                seed,
            } => {
                return run_setup_contribute(degree, min_participants, seed.as_deref());
            }
            #[cfg(feature = "zk")]
            CliCommand::SetupVerify {
                degree,
                num_contributions,
            } => {
                return run_setup_verify(degree, num_contributions);
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::SetupContribute { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::SetupVerify { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            CliCommand::Snapshot { output } => {
                return run_snapshot(&output);
            }
            CliCommand::Restore { input } => {
                return run_restore(&input);
            }
            CliCommand::Run => {}
            #[cfg(feature = "zk")]
            CliCommand::CeremonyServe {
                min_participants,
                max_participants,
                degree,
            } => {
                return run_ceremony_serve(min_participants, max_participants, degree);
            }
            #[cfg(feature = "zk")]
            CliCommand::CeremonyContribute { server_url, seed } => {
                return run_ceremony_contribute(&server_url, seed.as_deref()).await;
            }
            #[cfg(feature = "zk")]
            CliCommand::CeremonyVerify { server_url } => {
                return run_ceremony_verify(&server_url).await;
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::CeremonyServe { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::CeremonyContribute { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::CeremonyVerify { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            CliCommand::GenesisInit { config, output } => {
                return run_genesis_init(&config, &output);
            }
            CliCommand::GenesisValidate { block } => {
                return run_genesis_validate(&block);
            }
        }
    }

    let config = NodeConfig::from_cli();
    config.validate().context("Invalid configuration")?;

    // 2. Initialize tracing with the configured log level
    init_tracing(&config.log_level);

    // 2b. Reject known-weak / placeholder JWT secrets before anything else
    // touches auth (AUDIT-2026-07 C11, #349). An operator who shipped the
    // old compose default would otherwise run with a forgeable, publicly
    // known secret. An unset secret is allowed here (the auth middleware
    // rejects authenticated requests in that case — it is never a bypass).
    if let Err(e) = omnia_node::api::auth::validate_jwt_secret_strength() {
        anyhow::bail!("Refusing to start: {e}");
    }

    // 2c. Initialize the JWT secret cache from the environment.
    // This must happen before any request hits the auth middleware.
    omnia_node::api::auth::init_jwt_secret();

    tracing::info!(
        node_id = config.node_id,
        http_port = config.http_port,
        listen_addr = %config.listen_addr,
        data_dir = %config.data_dir.display(),
        log_level = %config.log_level,
        "Starting Omnia node"
    );

    // 3. Create data directory
    let data_dir = config
        .ensure_data_dir()
        .context("Failed to initialize data directory")?;
    tracing::info!(data_dir = %data_dir.display(), "Data directory ready");

    // 3b. Check for legacy sled database and migrate if present
    let sled_path = data_dir.join("sled");
    if sled_path.exists() {
        tracing::info!(
            sled_path = %sled_path.display(),
            "Detected legacy sled database, migrating to redb..."
        );
        let redb_path = data_dir.join("omnia.redb");
        omnia_substrate::migration::migrate_sled_to_redb(&sled_path, &redb_path)
            .context("Sled-to-redb migration failed")?;
        tracing::info!("Migration complete");
    }

    // 4. Initialize substrate components
    let node_id_bytes = config.node_id_bytes();
    let slashing_dir = config.slashing_dir();

    // Create the substrate runtime with slashing persistence configured.
    //
    // H-12 fix (audit v0.1.68): use `try_new` instead of `new` so that an
    // invalid `OMNIA_CONSENSUS_SEED` env var produces a clean error and
    // exit, rather than silently falling back to a random seed (which
    // would cause the node to fork off the network).
    let mut substrate_config = SubstrateConfig::try_new(node_id_bytes)
        .context("Failed to parse OMNIA_CONSENSUS_SEED — fix the env var or unset it to use a random seed")?;
    substrate_config.slashing_data_dir = Some(slashing_dir.to_path_buf());
    substrate_config.max_payload_size = config.max_payload_size;
    substrate_config.pruning_depth = config.pruning_depth;
    substrate_config.snapshot_interval = config.snapshot_interval;
    substrate_config.nonce_data_dir = Some(config.nonce_dir());
    substrate_config.consensus_data_dir = Some(config.consensus_dir());

    // A2: Populate GossipConfig.bootstrap_peers from CLI/TOML config so that
    // the gossip layer dials the same seed nodes as the network layer.
    // This must be set BEFORE Substrate::new() consumes the config.
    #[cfg(feature = "network")]
    {
        substrate_config.gossip.bootstrap_peers = config.bootstrap_nodes.clone();
    }

    let mut substrate = Substrate::new(substrate_config);

    // P0-1: Initialize the gossip protocol before wrapping substrate in Arc<RwLock>.
    // This creates a GossipProtocol with a shared Arc<RwLock<CausalGraph>> that
    // will later be wired to the P2P network via start_with_network().
    #[cfg(feature = "network")]
    {
        substrate.init_gossip();
        tracing::info!(
            bootstrap_count = config.bootstrap_nodes.len(),
            "Gossip protocol initialized with bootstrap peers"
        );
    }
    tracing::info!(
        path = %slashing_dir.display(),
        "Substrate runtime initialized with persistent slashing engine"
    );

    // P0-2 (audit fix): Register the node as a validator candidate so it
    // can be elected leader and propose blocks. The previous
    // implementation never populated `validator_candidates`, which meant
    // `process_consensus_round()` always skipped the leader check and
    // the node never proposed any blocks — even in a single-node setup.
    //
    // We use the node's persistent keypair as the validator keypair. The
    // stake is set to 1 (the minimum) — real deployments should override
    // this via config or staking operations.
    //
    // P0-4 (audit fix): Load the keypair from disk if OMNIA_NODE_KEY_FILE
    // is set, otherwise generate a fresh one. The previous implementation
    // always generated an ephemeral keypair, which broke identity
    // continuity across restarts AND invalidated the validator
    // registration (the validator pubkey would change every restart,
    // causing the node to be unable to sign blocks it was elected to
    // produce after the restart).
    let node_keypair = load_or_generate_node_keypair(&config.data_dir);
    let node_pubkey_bytes = node_keypair.verifying_key().to_bytes();
    substrate.add_validator(node_id_bytes, node_keypair.clone(), 1);
    tracing::info!(
        node_id = %hex::encode(&node_id_bytes[..4]),
        pubkey = %hex::encode(&node_pubkey_bytes[..8]),
        "Node registered as validator candidate (stake=1) — node can now be elected leader"
    );

    // Lane 0 (ADR-025): enable the consensusless fast path when a static
    // validator set is configured. A malformed spec is a hard startup
    // error — a typo must not silently disable finality.
    if let Ok(spec) = std::env::var("OMNIA_LANE0_VALIDATORS") {
        match omnia_substrate::lane0::ValidatorSet::parse(&spec) {
            Ok(Some(validators)) => {
                let member = validators.contains(&node_pubkey_bytes);
                tracing::info!(
                    validators = validators.len(),
                    total_stake = validators.total_stake(),
                    this_node_is_validator = member,
                    "Lane 0 fast-path finality enabled"
                );
                substrate.init_lane0(validators);
            }
            Ok(None) => {
                tracing::info!("OMNIA_LANE0_VALIDATORS is empty — Lane 0 disabled");
            }
            Err(e) => {
                anyhow::bail!("Invalid OMNIA_LANE0_VALIDATORS: {e}");
            }
        }
    }

    // C4 resolved (single source of truth): the economics state lives
    // ONLY inside the shard router's registered economics shard. Both the
    // consensus/event path (via the substrate's shard processor) and the
    // HTTP API reach that one instance through the same
    // `Arc<Mutex<ShardRouter>>` — there is no separate `AppState.economics`
    // copy to diverge. The API accesses it via `ShardRouter::economics_mut`
    // (see the `with_economics` helper).
    let economics = EconomicsState::new();
    tracing::info!("Economics state initialized (10% decay, 1000 UBC/month)");

    let shard_router = create_shard_router(
        Some(config.nonce_dir().as_path()),
        economics,
        node_pubkey_bytes,
        config.mint_authority,
    )?;
    tracing::info!(shard_count = 6, "Shard router initialized with all shard types");

    // B1: Wrap ShardRouter in Arc<std::sync::Mutex> so it can be shared between
    // the HTTP API (via AppState) and the Substrate consensus loop (via
    // EventProcessor). We use std::sync::Mutex (not tokio) because
    // EventProcessor::process_event is synchronous.
    let shared_shard_router = Arc::new(std::sync::Mutex::new(shard_router));

    // B1: Create a MutexShardRouter wrapper that implements EventProcessor
    // by locking the shared mutex and delegating to the inner ShardRouter.
    let shard_processor = MutexShardRouter::new(Arc::clone(&shared_shard_router));

    // B1: Wire the shard processor into the substrate so committed events
    // from consensus are automatically routed to the appropriate domain shard.
    // Previously, shard_processor was None and events only reached shards
    // via the HTTP API bypass.
    substrate = substrate.with_shard_processor(Box::new(shard_processor));
    tracing::info!("ShardRouter wired as EventProcessor on Substrate — committed events will reach shards");

    // Construct the settlement adapter based on enabled features.
    // By default, uses MockSettlementAdapter (zero alloy, compiles on MSRV 1.88).
    // When ethereum-live is enabled, uses EthereumSettlementAdapter (requires rustc >= 1.91).
    let settlement: Arc<dyn SettlementAdapter> = {
        #[cfg(feature = "ethereum-live")]
        {
            // Try to create a live Ethereum adapter from environment config.
            // Falls back to MockSettlementAdapter if config is missing or invalid.
            match create_ethereum_settlement_adapter() {
                Ok(adapter) => {
                    tracing::info!("Settlement: Ethereum live adapter (alloy-backed, requires rustc >= 1.91)");
                    adapter
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "Settlement: Falling back to MockSettlementAdapter — Ethereum live config invalid or missing"
                    );
                    Arc::new(omnia_adapters::MockSettlementAdapter::new())
                }
            }
        }
        #[cfg(not(feature = "ethereum-live"))]
        {
            tracing::info!("Settlement: Mock adapter (enable --features ethereum-live for live Ethereum)");
            Arc::new(omnia_adapters::MockSettlementAdapter::new())
        }
    };

    // H-10 fix (audit v0.1.68): emit a loud startup warning if the node is
    // configured with a non-live (stub/mock) settlement adapter. Operators
    // running a "production" node with MockSettlementAdapter, or with one
    // of the stub adapters (Bitcoin/Solana/Cosmos/Celestia — all return
    // `NotImplemented`), need to know that `submit_root()` calls will fail
    // and no real settlement will happen.
    //
    // Per the strategy doc:
    //   - Ethereum live adapter: production-ready (or close enough for testnet)
    //   - Bitcoin / Solana / Cosmos / Celestia adapters: STUBS, return NotImplemented
    //   - Mock adapter: testing only
    if !settlement.is_live() {
        let message = "Settlement adapter is NOT a live L1 (is_live() == false). \
             All SettlementAdapter::submit_root() calls will return \
             NotImplemented or be no-ops. Do not use in production. \
             Enable --features ethereum-live and configure EthereumConfig \
             for real settlement.";
        if matches!(std::env::var("OMNIA_ENV").as_deref(), Ok("production")) {
            anyhow::bail!("Refusing production startup with non-live settlement adapter. {message}");
        }
        tracing::warn!(message);
    }

    // Initialize Prometheus metrics
    #[cfg(feature = "metrics")]
    let metrics = NodeMetrics::new().context("Failed to initialize Prometheus metrics")?;
    #[cfg(feature = "metrics")]
    tracing::info!("Prometheus metrics initialized");

    // 5. Build shared application state
    // Clone the slashing engine BEFORE moving substrate into the Arc,
    // so both the substrate and the API share the same Arc<dyn SlashingStore>.
    let slashing_engine = substrate.slashing.clone();

    // Clone the substrate Arc for background tasks BEFORE wrapping in AppState
    let substrate_for_consensus = Arc::new(RwLock::new(substrate));

    // Initialize ceremony server when ZK feature is enabled
    #[cfg(feature = "zk")]
    let ceremony_server = {
        let ceremony_config = omnia_adapters::setup::CeremonyConfig::default();
        let server = omnia_adapters::setup::CeremonyServer::new(ceremony_config);
        if let Err(e) = server.start() {
            tracing::warn!(error = %e, "Failed to start ceremony server");
            None
        } else {
            tracing::info!("Ceremony server initialized");
            Some(Arc::new(RwLock::new(server)))
        }
    };

    // The persistent node keypair was generated earlier (above the
    // Substrate::new call) so it could be used to register the node as
    // a validator candidate. The same keypair is reused here for event
    // signing — events must be verifiable as originating from this
    // node, and the validator registration must match the event signer.
    //
    // TODO: Support loading an existing keypair from the keystore via CLI flag
    // so identity persists across restarts. Until that is implemented, every
    // restart generates a new ephemeral keypair, which breaks identity
    // continuity AND invalidates the validator registration.
    tracing::info!(
        pubkey = %hex::encode(&node_keypair.verifying_key().to_bytes()[..8]),
        "Persistent node keypair ready for event signing (also registered as validator above)"
    );

    let app_state = AppState {
        config: config.clone(),
        substrate: Arc::clone(&substrate_for_consensus),
        slashing: Arc::new(Mutex::new(slashing_engine)),
        shard_router: shared_shard_router,
        event_store: Arc::new(RwLock::new(indexmap::IndexMap::new())),
        transfer_history: Arc::new(RwLock::new(Vec::new())),
        challenges: omnia_node::api::wallet_auth::new_challenge_store(),
        peers: Arc::new(RwLock::new(Vec::new())),
        #[cfg(feature = "metrics")]
        metrics: Arc::new(metrics),
        started_at: Instant::now(),
        is_syncing: Arc::new(AtomicBool::new(false)),
        keypair: Some(node_keypair),
        settlement,
        #[cfg(feature = "zk")]
        ceremony_server,
    };

    // 6. Build and start the HTTP server
    let app = omnia_node::http::build_http_router().with_state(app_state.clone());
    let listen_addr = format!("0.0.0.0:{}", config.http_port);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .with_context(|| format!("Failed to bind HTTP server to {listen_addr}"))?;

    tracing::info!(listen_addr = %listen_addr, "HTTP server starting");

    // 7. Spawn background tasks for consensus loop and pipeline router
    // These are the critical integration pieces that enable the node to
    // participate in the network autonomously.
    let shutdown_tx = spawn_background_tasks(config.clone(), substrate_for_consensus, app_state.clone()).await?;

    // 8. Start the HTTP server with graceful shutdown
    //
    // SECURITY: `into_make_service_with_connect_info::<SocketAddr>()` is
    // REQUIRED for per-client rate limiting. Without it, the axum
    // `ConnectInfo<SocketAddr>` extractor is never injected into request
    // extensions, and `rate_limit_middleware` (api/auth.rs) falls back to
    // `client_key = "unauthenticated"` for every request — meaning all
    // clients share a single rate-limit bucket. The previous
    // implementation called `axum::serve(listener, app)` directly, which
    // silently disabled per-client rate limiting (DoS protection).
    let server_future = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());

    server_future.await.context("HTTP server error")?;

    // Signal background tasks to shut down
    let _ = shutdown_tx.send(());
    tracing::info!("Omnia node shut down gracefully");
    Ok(())
}

/// Spawn background tasks for consensus loop, pipeline router, and P2P networking.
///
/// This function wires the core protocol components that enable the node to
/// run autonomously: the consensus loop (periodic round processing), the
/// pipeline router (hot/warm/cold path workers), and optionally P2P networking.
///
/// Returns a shutdown sender that can be used to signal all tasks to stop.
async fn spawn_background_tasks(
    config: NodeConfig,
    substrate: Arc<RwLock<Substrate>>,
    app_state: AppState,
) -> Result<tokio::sync::broadcast::Sender<()>> {
    let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel(1);

    // C-14 fix (audit v0.1.68): Removed dead pipeline workers.
    // The pipeline router and its hot/warm/cold workers only logged
    // messages — they never validated, inserted, or processed events.
    // All event processing goes through submit_event() + the consensus
    // loop below. The pipeline.rs module is retained for future use.

    // 7b. Spawn the consensus background loop
    // This periodically calls check_round_timeout() and processes consensus
    let mut shutdown_consensus = shutdown_tx.subscribe();
    let substrate_consensus = Arc::clone(&substrate);
    // P0-3 (audit fix): peer-tracking state. The previous implementation
    // never wrote to `AppState.peers`, so `/readyz` always returned 503
    // ("no_peers") and `/api/v1/node/peers` always returned an empty list,
    // even when the node had many active gossip connections. We now
    // poll `Substrate::connected_peer_count()` after each consensus round
    // and update the peers list with a synthetic PeerInfo for the count.
    // A richer per-peer PeerInfo would require extending the gossip layer
    // to expose the PeerId set; this count-based fix is the minimum
    // required to make `/readyz` work.
    let peers_state = Arc::clone(&app_state.peers);
    #[cfg(feature = "metrics")]
    let node_metrics = Arc::clone(&app_state.metrics);
    tokio::spawn(async move {
        tracing::info!("Consensus background loop started");

        // Round timer: fires at the consensus round interval (default 1 second)
        let round_duration = tokio::time::Duration::from_millis(1000);
        let mut round_timer = tokio::time::interval(round_duration);

        // Last-sampled values for delta-based counters. The Sprint-0
        // throughput metrics were registered but never updated, so
        // /metrics reported them flat zero; sampling here once per round
        // makes DAG growth, finality, peers, and RSS observable — the
        // signals multi-node benchmarks converge on (ADR-025 Stage 2).
        #[cfg(feature = "metrics")]
        let mut last_dag_total: u64 = 0;
        #[cfg(feature = "metrics")]
        let mut last_committed: u64 = 0;
        #[cfg(feature = "metrics")]
        let mut last_lane0_finalized: u64 = 0;

        loop {
            tokio::select! {
                _ = shutdown_consensus.recv() => {
                    tracing::info!("Consensus loop shutting down");
                    break;
                }
                _ = round_timer.tick() => {
                    // P0-1: Use process_consensus_round() which drains gossip
                    // events from the network, runs consensus, and feeds
                    // committed events to the shard processor. This replaces
                    // the previous process_consensus() call that skipped the
                    // gossip event drain step.
                    let mut substrate = substrate_consensus.write().await;
                    substrate.process_consensus_round().await;

                    // Sample the round-level metrics (see declaration above).
                    #[cfg(feature = "metrics")]
                    {
                        let dag_total = substrate.graph().await.len() as u64;
                        if dag_total > last_dag_total {
                            node_metrics.dag_events_total.inc_by(dag_total - last_dag_total);
                            last_dag_total = dag_total;
                        }
                        let stats = substrate.consensus_stats();
                        if stats.committed > last_committed {
                            let delta = stats.committed - last_committed;
                            node_metrics.events_finalized.inc_by(delta);
                            node_metrics.consensus_tps.inc_by(delta);
                            last_committed = stats.committed;
                        }
                        // Lane 0 fast-path finality (ADR-025): quorum-acked
                        // events are just as final as Lane 1 commits, so they
                        // feed the same finalized counter. Without this, a
                        // single-writer workload (e.g. the testnet benchmark
                        // submitting through one node) reports zero finality
                        // even with Lane 0 fully operational — Lane 1 rounds
                        // can only advance with events from 2f+1 distinct
                        // creators.
                        if let Some((_, _, lane0_finalized)) = substrate.lane0_stats() {
                            if lane0_finalized > last_lane0_finalized {
                                let delta = lane0_finalized - last_lane0_finalized;
                                node_metrics.events_finalized.inc_by(delta);
                                node_metrics.consensus_tps.inc_by(delta);
                                last_lane0_finalized = lane0_finalized;
                            }
                        }
                        node_metrics.consensus_round.set(substrate.current_round() as i64);
                        node_metrics.sample_memory_rss();
                    }

                    // P0-3: refresh peer count from gossip layer.
                    #[cfg(feature = "network")]
                    {
                        let peer_count = substrate
                            .connected_peer_count()
                            .unwrap_or(0);
                        #[cfg(feature = "metrics")]
                        node_metrics.peers_connected.set(peer_count as i64);
                        let mut peers_guard = peers_state.write().await;
                        // Only mutate the Vec if the count changed —
                        // avoids spurious write-lock contention.
                        if peers_guard.len() != peer_count {
                            peers_guard.clear();
                            for i in 0..peer_count {
                                peers_guard.push(omnia_node::api::node::PeerInfo {
                                    peer_id: format!("peer-{i:04}"),
                                    address: String::new(),
                                    connected_at: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_secs())
                                        .unwrap_or(0),
                                });
                            }
                            tracing::debug!(peer_count, "Updated peers list");
                        }
                    }

                    drop(substrate);
                }
            }
        }
    });

    tracing::info!("Consensus background loop spawned (1s interval)");

    // 7c. Spawn P2P network initialization (when network feature is enabled)
    // P0-1: Wire OmniaNetwork → GossipProtocol → CausalGraph → Consensus
    // Instead of manually draining event_rx and only logging, we now delegate
    // to GossipProtocol::start_with_network() which wires the full pipeline:
    //   network.event_rx → gossip.network_rx → process_pending_events() →
    //   graph.insert() → unprocessed_events → process_consensus()
    #[cfg(feature = "network")]
    {
        let mut shutdown_network = shutdown_tx.subscribe();
        let listen_addr = config.listen_addr.clone();

        // We need a clone of the substrate Arc for wiring the network
        let substrate_for_network = Arc::clone(&substrate);

        tokio::spawn(async move {
            tracing::info!("P2P network initialization started");

            // Parse listen address into a Multiaddr for libp2p.
            // Support both multiaddr format (e.g., "/ip4/0.0.0.0/udp/4001/quic-v1")
            // and plain host:port format (e.g., "0.0.0.0:4001") which is converted
            // to a QUIC multiaddr since the swarm only supports QUIC by default.
            let listen_multiaddr: Multiaddr = if listen_addr.starts_with('/') {
                // Already a multiaddr — parse directly
                match listen_addr.parse() {
                    Ok(addr) => addr,
                    Err(e) => {
                        tracing::error!(error = %e, addr = %listen_addr, "Failed to parse listen address as Multiaddr");
                        return;
                    }
                }
            } else {
                // Plain host:port format — convert to QUIC multiaddr.
                // The default swarm transport is QUIC-only, so we convert
                // "0.0.0.0:4001" to "/ip4/0.0.0.0/udp/4001/quic-v1".
                let quic_addr = format!("/ip4/0.0.0.0/udp/{listen_addr}/quic-v1");
                match quic_addr.parse() {
                    Ok(addr) => {
                        tracing::info!(original = %listen_addr, converted = %addr, "Converted host:port listen address to QUIC multiaddr");
                        addr
                    }
                    Err(e) => {
                        tracing::error!(error = %e, original = %listen_addr, attempted = %quic_addr, "Failed to convert listen address to QUIC multiaddr");
                        return;
                    }
                }
            };

            // A2: Build a NetworkConfig that includes the bootstrap peers
            // from CLI/TOML, so the Kademlia DHT is seeded correctly.
            //
            // The swarm identity is derived from the node's persistent
            // keypair, so the libp2p PeerId survives restarts and pinned
            // `/p2p/<PeerId>` bootstrap addresses stay valid. Previously
            // the identity was regenerated on every start, which broke
            // any pinned address as soon as the node restarted.
            let network_config = NetworkConfig {
                identity: Some(load_or_generate_node_keypair(&config.data_dir).to_bytes()),
                bootstrap_peers: config
                    .bootstrap_nodes
                    .iter()
                    .filter_map(|addr| addr.parse::<Multiaddr>().ok())
                    .collect(),
                ..Default::default()
            };

            // Try to create the network, but don't block if it fails
            match OmniaNetwork::with_config(listen_multiaddr, network_config).await {
                Ok(mut network) => {
                    tracing::info!("P2P network initialized");

                    // Subscribe to the omnia_events gossip topic before
                    // starting the network run loop
                    if let Err(e) = network.subscribe("omnia_events") {
                        tracing::warn!(error = %e, "Failed to subscribe to omnia_events topic");
                    }
                    // Lane 0 (ADR-025): finality acks ride their own topic.
                    // Subscribing is harmless when Lane 0 is disabled —
                    // received acks are simply never folded.
                    if let Err(e) = network.subscribe(omnia_substrate::lane0::LANE0_ACKS_TOPIC) {
                        tracing::warn!(error = %e, "Failed to subscribe to Lane 0 acks topic");
                    }
                    // Keepalive heartbeats (issue #259): keep the idle mesh
                    // alive so partition detection doesn't evict quiet peers.
                    if let Err(e) = network.subscribe(omnia_network::gossip::HEARTBEAT_TOPIC) {
                        tracing::warn!(error = %e, "Failed to subscribe to heartbeat topic");
                    }
                    // Anti-entropy repair (issue #315): digests, event
                    // requests, and repair batches ride this topic.
                    if let Err(e) = network.subscribe(omnia_network::gossip::SYNC_TOPIC) {
                        tracing::warn!(error = %e, "Failed to subscribe to sync topic");
                    }

                    // Wire the network into the substrate's gossip protocol.
                    // This takes ownership of the network, spawns network.run_with_commands()
                    // internally, and stores event_rx in gossip.network_rx so that
                    // process_pending_events() can drain incoming events.
                    let mut substrate = substrate_for_network.write().await;
                    match substrate.wire_network(network).await {
                        Ok(()) => {
                            tracing::info!(
                                "P2P network wired to gossip protocol — events flow: \
                                 network → gossip → graph → consensus"
                            );
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to wire network to gossip protocol");
                        }
                    }
                    drop(substrate);

                    // Wait for shutdown signal — the network run loop is already
                    // spawned inside GossipProtocol::start_with_network()
                    let _ = shutdown_network.recv().await;
                    tracing::info!("P2P network task shutting down");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "P2P network initialization failed — node running without P2P");
                }
            }
        });

        tracing::info!("P2P network task spawned");
    }

    #[cfg(not(feature = "network"))]
    {
        tracing::info!("P2P networking disabled (compile with --features network to enable)");
    }

    // Return shutdown sender so the main function can signal shutdown
    Ok(shutdown_tx)
}

/// Initialize the tracing subscriber with the specified log level.
///
/// Supports structured JSON output when the `RUST_LOG_FORMAT=json`
/// environment variable is set; otherwise uses the standard
/// human-readable format.
fn init_tracing(log_level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    let format = std::env::var("RUST_LOG_FORMAT").unwrap_or_default();
    if format == "json" {
        tracing_subscriber::fmt().with_env_filter(filter).json().init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(false)
            .init();
    }
}

/// Create a shard router with all six shard types registered.
///
/// Uses the standard fee schedule for operation pricing. When `nonce_data_dir`
/// is `Some`, creates a `RedbNonceStore` for persistent replay protection;
/// otherwise falls back to in-memory nonce tracking.
fn create_shard_router(
    nonce_data_dir: Option<&std::path::Path>,
    economics_state: omnia_economics::EconomicsState,
    node_pubkey: [u8; 32],
    mint_authority: Option<[u8; 32]>,
) -> Result<ShardRouter> {
    let fee_schedule = FeeSchedule::standard();
    let quota = omnia_economics::QuotaSystem::default_system();

    let mut router = match nonce_data_dir {
        Some(db_path) => {
            // Ensure the parent directory exists
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create nonce directory: {}", parent.display()))?;
            }
            let nonce_store: Arc<dyn omnia_shards::NonceStore> = Arc::new(
                omnia_shards::RedbNonceStore::open(db_path)
                    .with_context(|| "Failed to open nonce store in redb database")?,
            );
            tracing::info!(path = %db_path.display(), "Shard router using persistent nonce store");
            ShardRouter::with_nonce_store(fee_schedule, quota, nonce_store)
        }
        None => {
            tracing::info!("Shard router using in-memory nonce store (no persistence)");
            ShardRouter::new(fee_schedule, quota)
        }
    };

    // Register all six domain shards.
    //
    // The financial shard's mint authority comes from configuration, not
    // from this node's identity.
    //
    // H10 originally set it to `node_pubkey` so that minting was possible
    // at all (`FinancialShard::new()` leaves it `None`, which disables
    // minting). That was harmless while a single node existed, but it does
    // not survive a real mesh: `FinancialState::apply` accepts a `Mint`
    // only when the event creator matches the configured authority, so
    // three nodes each holding their own key would each accept only their
    // own mints and reject their peers'. They would disagree about total
    // supply and every balance derived from it — a state divergence that
    // consensus cannot repair, because both sides are behaving "correctly"
    // per their own config.
    //
    // The authority is therefore a network-wide genesis parameter. When it
    // is unset we fail closed and disable minting rather than substituting
    // this node's key, because a silent substitution produces exactly the
    // divergence above and looks fine until someone mints.
    let financial_shard = match mint_authority {
        Some(authority) => {
            if authority == node_pubkey {
                tracing::info!(
                    authority = %hex::encode(authority),
                    "Financial shard mint authority is this node's own key — \
                     ensure every peer is configured with the SAME key, not their own"
                );
            } else {
                tracing::info!(
                    authority = %hex::encode(authority),
                    "Financial shard mint authority configured"
                );
            }
            FinancialShard::with_mint_authority(authority)
        }
        None => {
            tracing::warn!(
                "No mint_authority configured (--mint-authority / OMNIA_MINT_AUTHORITY) — \
                 minting on the financial shard is DISABLED. Transfers still work; \
                 accounts simply start at zero until an authority is set network-wide."
            );
            FinancialShard::new()
        }
    };
    router.register(Box::new(financial_shard));
    router.register(Box::new(ComputationalShard::new()));
    router.register(Box::new(PhysicalShard::new()));
    router.register(Box::new(BiologicalShard::new()));
    router.register(Box::new(IdentityShard::new()));
    // C4 fix: use the SAME EconomicsState instance as the HTTP API path.
    // The previous `EconomicsShard::new()` created a second EconomicsState
    // that diverged from AppState.economics — mints via the consensus path
    // were invisible to balance reads via the API, and vice versa.
    router.register(Box::new(EconomicsShard::new_with_state(economics_state)));

    Ok(router)
}

/// Magic bytes identifying an encrypted Omnia key file.
///
/// Format: `OMNIAKEY01` (10 bytes) + nonce (12 bytes) + ciphertext+tag (80 bytes) = 102 bytes total.
const ENCRYPTED_KEY_MAGIC: &[u8; 10] = b"OMNIAKEY01";

/// Domain separation tag for BLAKE3 key derivation from passphrase.
const KEY_DERIVATION_CONTEXT: &str = "omnia-keygen-aes256gcm";

/// Apply key stretching to a passphrase using multiple rounds of BLAKE3 derivation.
///
/// A single BLAKE3 `derive_key` call is too fast for passphrase-based key derivation
/// (an attacker can try billions of guesses per second). This function applies
/// 100,000 rounds of BLAKE3 derivation to slow down brute-force attacks while
/// maintaining deterministic output (same passphrase always produces the same key).
///
/// SECURITY NOTE: BLAKE3-based key derivation with 100K iterations is used for
/// passphrase stretching. This provides moderate protection but is NOT as strong
/// as memory-hard functions like Argon2id. For production deployments with
/// user-supplied passphrases, consider migrating to Argon2id with appropriate
/// parameters (time=3, memory=64MB, parallelism=4).
/// TODO: Replace with argon2 crate for production-grade key derivation.
fn stretch_passphrase(pass: &str, context: &str) -> [u8; 32] {
    let mut key = blake3::derive_key(context, pass.as_bytes());
    for _ in 0..100_000 {
        key = blake3::derive_key(context, &key);
    }
    key
}

/// Load the node's persistent Ed25519 keypair from disk, or generate a
/// fresh one and persist it for future restarts.
///
/// SECURITY FIX (audit): the previous implementation always called
/// `generate_keypair()` at startup, producing a new identity every
/// restart. This broke event-signature continuity (events signed by the
/// old key were unverifiable by the new key) AND invalidated the
/// validator registration (the validator pubkey changed every restart).
///
/// Behavior:
///   - If `OMNIA_NODE_KEY_FILE` is set, load the 32-byte Ed25519 secret
///     key from that file (raw bytes, no encryption — operators are
///     responsible for file permissions). If the file does not exist,
///     generate a fresh keypair and write its secret key there with
///     `0600` permissions.
///   - Otherwise, fall back to the data_dir's `node_key.bin` file using
///     the same load-or-create logic.
///   - If neither path is writable, log a loud warning and generate an
///     ephemeral keypair (the node will run but identity will not
///     persist — operators must fix the filesystem issue).
fn load_or_generate_node_keypair(data_dir: &std::path::Path) -> omnia_substrate::crypto::NodeKeypair {
    use omnia_substrate::crypto::SigningKey;

    let key_path = std::env::var("OMNIA_NODE_KEY_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| data_dir.join("node_key.bin"));

    // Try to load an existing key.
    if key_path.exists() {
        match std::fs::read(&key_path) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut secret = [0u8; 32];
                secret.copy_from_slice(&bytes);
                let signing_key = SigningKey::from_bytes(&secret);
                tracing::info!(
                    path = %key_path.display(),
                    pubkey = %hex::encode(&signing_key.verifying_key().to_bytes()[..8]),
                    "Loaded persistent node keypair from disk"
                );
                return signing_key;
            }
            Ok(bytes) => {
                tracing::warn!(
                    path = %key_path.display(),
                    len = bytes.len(),
                    "Existing key file has wrong length (expected 32 bytes) — generating a new keypair"
                );
            }
            Err(e) => {
                tracing::warn!(
                    path = %key_path.display(),
                    error = %e,
                    "Failed to read existing key file — generating a new keypair"
                );
            }
        }
    }

    // Generate a fresh keypair.
    let keypair = omnia_substrate::crypto::generate_keypair();

    // Try to persist the secret key for future restarts.
    let secret_bytes = keypair.to_bytes();
    match std::fs::write(&key_path, secret_bytes) {
        Ok(()) => {
            // Restrict permissions on Unix.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
            }
            tracing::info!(
                path = %key_path.display(),
                pubkey = %hex::encode(&keypair.verifying_key().to_bytes()[..8]),
                "Generated and persisted new node keypair"
            );
        }
        Err(e) => {
            tracing::warn!(
                path = %key_path.display(),
                error = %e,
                "Failed to persist node keypair — identity will NOT survive restart. \
                 Fix filesystem permissions to enable identity continuity."
            );
        }
    }

    keypair
}

/// Generate a new validator keypair and save it to the specified output directory.
///
/// When a `passphrase` is provided, the private key is encrypted with
/// AES-256-GCM and saved as `validator_key.enc`. The key is derived from
/// the passphrase using BLAKE3 with domain separation.
///
/// When no passphrase is provided, the private key is written as raw bytes
/// to `validator_key.bin` with a prominent warning.
///
/// # Files Created
///
/// - `validator_pubkey.txt` — the hex-encoded public key
/// - `validator_key.enc` (encrypted) or `validator_key.bin` (unencrypted) — the private key
///
/// # Encrypted File Format
///
/// ```text
/// [OMNIAKEY01 (10B)] [nonce (12B)] [ciphertext+tag (80B)]
/// ```
///
/// # Errors
///
/// Returns an error if the output directory cannot be created, if
/// encryption fails, or if file writing fails.
fn run_keygen(output_dir: &str, passphrase: Option<&str>) -> Result<()> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use omnia_substrate::crypto::generate_keypair;

    let dir = std::path::Path::new(output_dir);
    std::fs::create_dir_all(dir).with_context(|| format!("Failed to create output directory: {output_dir}"))?;

    let keypair = generate_keypair();
    let pubkey_bytes = keypair.verifying_key().to_bytes();

    // Write public key as hex
    let pubkey_path = dir.join("validator_pubkey.txt");
    std::fs::write(&pubkey_path, hex::encode(pubkey_bytes))
        .with_context(|| format!("Failed to write public key to {pubkey_path:?}"))?;

    let privkey_bytes = keypair.to_bytes();

    if let Some(pass) = passphrase {
        // ---- Encrypted path ----
        // Derive a 256-bit key from the passphrase using BLAKE3 with domain separation
        // and key stretching (multiple rounds to slow down brute-force attacks)
        let derived_key = stretch_passphrase(pass, KEY_DERIVATION_CONTEXT);
        let cipher_key = aes_gcm::Key::<Aes256Gcm>::from_slice(&derived_key);
        let cipher = Aes256Gcm::new(cipher_key);

        // Generate a random 96-bit (12-byte) nonce
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt: ciphertext includes the 16-byte GCM authentication tag appended
        let ciphertext = cipher
            .encrypt(nonce, privkey_bytes.as_slice())
            .map_err(|e| anyhow::anyhow!("AES-256-GCM encryption failed: {e}"))?;

        // Build the file: magic + nonce + ciphertext+tag
        let mut file_bytes = Vec::with_capacity(10 + 12 + ciphertext.len());
        file_bytes.extend_from_slice(ENCRYPTED_KEY_MAGIC);
        file_bytes.extend_from_slice(&nonce_bytes);
        file_bytes.extend_from_slice(&ciphertext);

        let privkey_path = dir.join("validator_key.enc");
        std::fs::write(&privkey_path, &file_bytes)
            .with_context(|| format!("Failed to write encrypted private key to {privkey_path:?}"))?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&privkey_path, perms)
                .with_context(|| format!("Failed to set permissions on {privkey_path:?}"))?;
        }

        tracing::info!(
            output_dir = %dir.display(),
            pubkey = %hex::encode(&pubkey_bytes[..8]),
            encrypted = true,
            "Validator keypair generated and encrypted successfully"
        );

        println!("Validator keypair generated in {}", dir.display());
        println!("  Public key: {}", hex::encode(pubkey_bytes));
        println!("  Private key: {privkey_path:?} (AES-256-GCM encrypted)");
    } else {
        // ---- Unencrypted path (NOT recommended for production) ----
        let privkey_path = dir.join("validator_key.bin");
        std::fs::write(&privkey_path, privkey_bytes)
            .with_context(|| format!("Failed to write private key to {privkey_path:?}"))?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&privkey_path, perms)
                .with_context(|| format!("Failed to set permissions on {privkey_path:?}"))?;
        }

        tracing::warn!(
            output_dir = %dir.display(),
            "Private key written WITHOUT encryption — provide --passphrase for production use"
        );

        println!("Validator keypair generated in {}", dir.display());
        println!("  Public key: {}", hex::encode(pubkey_bytes));
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║  ⚠  WARNING: Private key is stored UNENCRYPTED!            ║");
        println!("║     Provide --passphrase or set OMNIA_KEYGEN_PASSPHRASE    ║");
        println!("║     to encrypt the key with AES-256-GCM.                  ║");
        println!("║     Unencrypted keys are NOT suitable for production.     ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
        println!("  Private key: {privkey_path:?}");
    }

    Ok(())
}

/// Load and decrypt an encrypted validator private key from disk.
///
/// Reads the file at `path`, validates the `OMNIAKEY01` magic header,
/// extracts the nonce and ciphertext, decrypts with AES-256-GCM using
/// the key derived from `passphrase` via BLAKE3, and returns the raw
/// 64-byte Ed25519 keypair bytes.
///
/// # Expected File Format
///
/// ```text
/// [OMNIAKEY01 (10B)] [nonce (12B)] [ciphertext+tag (80B)]
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be read
/// - The magic header does not match `OMNIAKEY01`
/// - The file is too short to contain the header, nonce, and tag
/// - Decryption fails (wrong passphrase, corrupted data, or tampering)
pub fn load_encrypted_key(path: &std::path::Path, passphrase: &str) -> Result<[u8; 64]> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

    let file_bytes = std::fs::read(path).with_context(|| format!("Failed to read key file: {path:?}"))?;

    // Validate minimum length: magic(10) + nonce(12) + tag(16) = 38 bytes minimum
    // (ciphertext must be at least 0 bytes + 16-byte tag, but Ed25519 keypair is 64 bytes,
    //  so the real minimum is 10 + 12 + 64 + 16 = 102)
    if file_bytes.len() < 38 {
        anyhow::bail!(
            "Key file too short ({} bytes), expected at least 38 bytes",
            file_bytes.len()
        );
    }

    // Validate magic header
    if &file_bytes[..10] != ENCRYPTED_KEY_MAGIC {
        anyhow::bail!(
            "Invalid key file magic: expected {:?}, got {:?}",
            std::str::from_utf8(ENCRYPTED_KEY_MAGIC),
            std::str::from_utf8(&file_bytes[..10]).unwrap_or("<invalid UTF-8>")
        );
    }

    let nonce_bytes = &file_bytes[10..22];
    let ciphertext = &file_bytes[22..];

    if ciphertext.len() < 16 {
        anyhow::bail!(
            "Ciphertext too short ({} bytes), must include 16-byte GCM tag",
            ciphertext.len()
        );
    }

    // Derive the same key from the passphrase (with stretching)
    let derived_key = stretch_passphrase(passphrase, KEY_DERIVATION_CONTEXT);
    let cipher_key = aes_gcm::Key::<Aes256Gcm>::from_slice(&derived_key);
    let cipher = Aes256Gcm::new(cipher_key);
    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt — this also verifies the authentication tag
    let mut plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("Decryption failed: wrong passphrase or corrupted file"))?;

    if plaintext.len() != 64 {
        anyhow::bail!(
            "Decrypted key has unexpected length: expected 64 bytes, got {}",
            plaintext.len()
        );
    }

    let mut key_bytes = [0u8; 64];
    key_bytes.copy_from_slice(&plaintext);
    // Zeroize the plaintext buffer after copying to prevent key material
    // from lingering on the stack or heap after this function returns.
    use zeroize::Zeroize;
    plaintext.zeroize();
    Ok(key_bytes)
}

/// Contribute to the Powers of Tau trusted setup ceremony (Phase 1).
///
/// Runs a local ceremony simulation with the specified parameters,
/// contributing fresh randomness to the SRS. In production, this
/// would coordinate with other participants over the network.
///
/// # Arguments
///
/// * `degree` — The maximum degree for the Powers of Tau SRS
/// * `min_participants` — Minimum participants before the ceremony can finalize
/// * `seed_hex` — Optional hex-encoded seed for deterministic contribution
#[cfg(feature = "zk")]
fn run_setup_contribute(degree: usize, min_participants: usize, seed_hex: Option<&str>) -> Result<()> {
    use omnia_adapters::setup::{contribute, PowersOfTau};

    // Initialize minimal tracing for the ceremony
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let mut srs = PowersOfTau::new(degree).context("Failed to initialize Powers of Tau")?;
    println!("Powers of Tau ceremony initialized (degree={degree})");

    // Parse optional seed
    let seed: Option<[u8; 32]> = seed_hex
        .map(|hex_str| {
            let bytes = hex::decode(hex_str).context("Failed to decode hex seed")?;
            let mut seed = [0u8; 32];
            if bytes.len() != 32 {
                anyhow::bail!(
                    "Seed must be exactly 32 bytes (64 hex chars), got {} bytes",
                    bytes.len()
                );
            }
            seed.copy_from_slice(&bytes);
            Ok(seed)
        })
        .transpose()?;

    // Make the contribution
    let transcript = srs.to_transcript();
    let tau_size = srs.g1_powers.len() + srs.g2_powers.len();
    let contribution =
        contribute(&transcript, tau_size, seed).map_err(|e| anyhow::anyhow!("Contribution failed: {e}"))?;

    srs.apply_contribution(&contribution)
        .map_err(|e| anyhow::anyhow!("Failed to apply contribution: {e}"))?;

    println!("Contribution accepted!");
    println!("  Participant ID: {}", hex::encode(&contribution.participant_id[..4]));
    println!("  Contribution count: {}", srs.contribution_count);
    println!("  Transcript hash: {}", hex::encode(&srs.transcript_hash[..8]));

    if srs.contribution_count >= min_participants {
        println!("  Ceremony has enough participants to proceed to Phase 2.");
    } else {
        println!(
            "  Need {} more contributions to reach minimum of {}.",
            min_participants - srs.contribution_count,
            min_participants
        );
    }

    Ok(())
}

/// Verify a completed Powers of Tau ceremony by replaying contributions.
///
/// Simulates a ceremony with the specified number of contributions
/// and verifies each one.
///
/// # Arguments
///
/// * `degree` — The maximum degree for the Powers of Tau SRS
/// * `num_contributions` — Number of contributions to replay and verify
#[cfg(feature = "zk")]
fn run_setup_verify(degree: usize, num_contributions: usize) -> Result<()> {
    use omnia_adapters::setup::run_ceremony;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Verifying Powers of Tau ceremony (degree={degree}, contributions={num_contributions})...");

    let srs =
        run_ceremony(degree, num_contributions).map_err(|e| anyhow::anyhow!("Ceremony verification failed: {e}"))?;

    println!("Ceremony verification successful!");
    println!("  Total contributions: {}", srs.contribution_count);
    println!("  Transcript hash: {}", hex::encode(&srs.transcript_hash[..8]));
    println!("  G1 powers: {}", srs.g1_powers.len());
    println!("  G2 powers: {}", srs.g2_powers.len());

    Ok(())
}

/// Take a state snapshot and write it to a file.
///
/// Creates a minimal CausalGraph, SlashingState, and nonce map,
/// serializes them into a [`StateSnapshot`], and writes the result
/// to the specified output path.
fn run_snapshot(output_path: &str) -> Result<()> {
    use omnia_substrate::snapshot::StateSnapshot;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let graph = omnia_substrate::CausalGraph::new();
    let slashing = omnia_substrate::SlashingState::default();
    let nonces = std::collections::HashMap::new();

    let snapshot =
        StateSnapshot::take(&graph, &slashing, &nonces, 0).map_err(|e| anyhow::anyhow!("Snapshot failed: {e}"))?;

    snapshot
        .write_to_file(std::path::Path::new(output_path))
        .map_err(|e| anyhow::anyhow!("Failed to write snapshot: {e}"))?;

    println!("Snapshot written to {output_path}");
    println!("  Height: {}", snapshot.height);
    println!("  Event count: {}", snapshot.event_count);
    println!("  State root: {}", hex::encode(&snapshot.state_root[..8]));

    Ok(())
}

/// Restore node state from a snapshot file.
///
/// Reads the snapshot from the specified path, verifies its integrity,
/// and prints summary information.
fn run_restore(input_path: &str) -> Result<()> {
    use omnia_substrate::snapshot::StateSnapshot;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let snapshot = StateSnapshot::read_from_file(std::path::Path::new(input_path))
        .map_err(|e| anyhow::anyhow!("Failed to read snapshot: {e}"))?;

    snapshot
        .verify()
        .map_err(|e| anyhow::anyhow!("Snapshot integrity check failed: {e}"))?;

    println!("Snapshot restored from {input_path}");
    println!("  Version: {}", snapshot.version);
    println!("  Height: {}", snapshot.height);
    println!("  Event count: {}", snapshot.event_count);
    println!("  State root: {}", hex::encode(&snapshot.state_root[..8]));
    println!("  Timestamp: {}", snapshot.timestamp);
    println!("  Integrity: OK");

    Ok(())
}

/// Generate a genesis block from a TOML configuration file.
///
/// Reads the genesis configuration, validates it (minimum 3 validators,
/// unique node IDs, non-zero stakes), generates a deterministic genesis
/// block, and writes it to the output path.
fn run_genesis_init(config_path: &str, output_path: &str) -> Result<()> {
    use omnia_substrate::genesis::{generate_genesis, GenesisConfig};

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Loading genesis configuration from {config_path}");

    let config_content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read genesis config: {config_path}"))?;

    let genesis_config: GenesisConfig =
        toml::from_str(&config_content).with_context(|| "Failed to parse genesis configuration TOML")?;

    println!("  Chain ID: {}", genesis_config.chain_id);
    println!("  Network: {}", genesis_config.network_name);
    println!("  Validators: {}", genesis_config.initial_validators.len());

    let genesis_block =
        generate_genesis(&genesis_config).map_err(|e| anyhow::anyhow!("Genesis generation failed: {e}"))?;

    println!("\nGenesis block generated successfully!");
    println!("  State root: {}", hex::encode(&genesis_block.state_root[..8]));
    println!("  Hash: {}", hex::encode(&genesis_block.hash[..8]));
    println!("  Validators: {}", genesis_block.validators.len());

    // Serialize and write
    let bytes = postcard::to_allocvec(&genesis_block).map_err(|e| anyhow::anyhow!("Serialization failed: {e}"))?;
    std::fs::write(output_path, &bytes).with_context(|| format!("Failed to write genesis block to {output_path}"))?;

    println!("\nGenesis block written to {output_path}");
    println!("  Size: {} bytes", bytes.len());

    Ok(())
}

/// Validate a genesis block file.
///
/// Reads the genesis block, verifies its integrity by re-deriving the
/// expected hash, and prints summary information.
fn run_genesis_validate(block_path: &str) -> Result<()> {
    use omnia_substrate::genesis::{validate_genesis, GenesisBlock};

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Loading genesis block from {block_path}");

    let bytes = std::fs::read(block_path).with_context(|| format!("Failed to read genesis block: {block_path}"))?;

    let genesis_block: GenesisBlock =
        postcard::from_bytes(&bytes).map_err(|e| anyhow::anyhow!("Deserialization failed: {e}"))?;

    println!("  Chain ID: {}", genesis_block.chain_id);
    println!("  Validators: {}", genesis_block.validators.len());
    println!("  Hash: {}", hex::encode(&genesis_block.hash[..8]));
    println!("  State root: {}", hex::encode(&genesis_block.state_root[..8]));

    validate_genesis(&genesis_block).map_err(|e| anyhow::anyhow!("Genesis validation failed: {e}"))?;

    println!("\nGenesis block is VALID");
    Ok(())
}

/// Create a live Ethereum settlement adapter from environment configuration.
///
/// Reads the following environment variables:
/// - `OMNIA_ETH_RPC_URL`: Ethereum JSON-RPC endpoint (required)
/// - `OMNIA_ETH_CONTRACT_ADDRESS`: OmniaRollup contract address (required)
/// - `OMNIA_ETH_OPERATOR_KEY`: Operator private key (required)
/// - `OMNIA_ETH_GAS_LIMIT`: Gas limit for transactions (optional, default 1M)
/// - `OMNIA_ETH_CONFIRMATION_BLOCKS`: Blocks to wait for finality (optional, default 3)
///
/// Returns an error if any required variable is missing or invalid.
#[cfg(feature = "ethereum-live")]
fn create_ethereum_settlement_adapter() -> Result<Arc<dyn SettlementAdapter>> {
    use omnia_adapters::{EthereumConfig, EthereumSettlementAdapter};

    let rpc_url = std::env::var("OMNIA_ETH_RPC_URL").context("OMNIA_ETH_RPC_URL environment variable not set")?;
    let contract_address = std::env::var("OMNIA_ETH_CONTRACT_ADDRESS")
        .context("OMNIA_ETH_CONTRACT_ADDRESS environment variable not set")?;
    let operator_key =
        std::env::var("OMNIA_ETH_OPERATOR_KEY").context("OMNIA_ETH_OPERATOR_KEY environment variable not set")?;

    let gas_limit = std::env::var("OMNIA_ETH_GAS_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);
    let confirmation_blocks = std::env::var("OMNIA_ETH_CONFIRMATION_BLOCKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);

    let config = EthereumConfig {
        rpc_url,
        contract_address,
        operator_private_key: operator_key,
        gas_limit,
        max_fee_per_gas: None,
        confirmation_blocks,
    };

    let adapter = EthereumSettlementAdapter::new(config)
        .map_err(|e| anyhow::anyhow!("Failed to create Ethereum settlement adapter: {}", e))?;

    Ok(Arc::new(adapter))
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM for graceful shutdown.
///
/// This function completes when either signal is received, allowing
/// the axum server to stop accepting new connections and finish
/// serving in-flight requests.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C — shutting down");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM — shutting down");
        }
    }
}

/// Start a multi-party trusted setup ceremony server.
///
/// Creates a [`CeremonyServer`] and runs it with an HTTP endpoint
/// for accepting contributions. The ceremony proceeds through
/// three phases:
/// 1. `NotStarted` → `AcceptingContributions` (on `start()`)
/// 2. Participants contribute via HTTP API
/// 3. `AcceptingContributions` → `Finalized` (on `finalize()`)
///
/// For now, this runs a local ceremony simulation that accepts
/// contributions from the command line. The full network ceremony
/// server with HTTP endpoints will be implemented in a follow-up.
#[cfg(feature = "zk")]
fn run_ceremony_serve(min_participants: usize, max_participants: usize, degree: usize) -> Result<()> {
    use omnia_adapters::setup::{contribute, CeremonyConfig, CeremonyServer};

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let config = CeremonyConfig {
        min_participants,
        max_participants,
        ceremony_id: 1,
        degree,
    };

    let server = CeremonyServer::new(config);
    server.start().context("Failed to start ceremony")?;

    println!("Ceremony server started (degree={degree})");
    println!("  Min participants: {min_participants} / Max participants: {max_participants}");

    // Simulate contributions (in a real server, these would come over HTTP)
    println!("\nSimulating {min_participants} participant contributions...");
    for i in 0..min_participants {
        let (transcript, tau_size) = server.get_srs_state().context("Failed to get SRS state")?;
        let mut seed = [0u8; 32];
        seed[0] = i as u8;
        seed[1] = (i >> 8) as u8;
        let contribution = contribute(&transcript, tau_size, Some(seed))
            .map_err(|e| anyhow::anyhow!("Contribution {i} failed: {e}"))?;
        let receipt = server
            .accept_contribution(contribution)
            .map_err(|e| anyhow::anyhow!("Accept contribution {i} failed: {e}"))?;
        println!("  Contribution {i} accepted (index={})", receipt.contribution_index);
    }

    // Finalize
    let circuit = omnia_adapters::circuit::RollupCircuit::empty();
    let key_pair = server
        .finalize(&circuit)
        .map_err(|e| anyhow::anyhow!("Finalize failed: {e}"))?;

    println!("\nCeremony finalized!");
    println!("  Total contributions: {}", server.contribution_count());
    println!("  Proving key size: {} bytes", key_pair.proving_key.len());
    println!("  Verifying key size: {} bytes", key_pair.verifying_key.len());
    println!("  Transcript hash: {}", hex::encode(&key_pair.tau_hash[..8]));

    // Export and display transcript info
    let transcript = server
        .export_transcript()
        .map_err(|e| anyhow::anyhow!("Export transcript failed: {e}"))?;
    println!("\nTranscript exported:");
    println!("  Contributions: {}", transcript.contribution_count);
    println!("  Final hash: {}", hex::encode(&transcript.final_transcript_hash[..8]));

    Ok(())
}

/// Contribute to a remote ceremony server.
///
/// Connects to the ceremony server at `server_url`, fetches the
/// current SRS state, generates a contribution locally, and
/// submits it to the server.
///
/// # API Contract
///
/// - `GET {server_url}/ceremony/state` → `{ "transcript": [...], "tau_size": N }`
/// - `POST {server_url}/ceremony/contribute` → `ContributionReceipt` (JSON)
#[cfg(feature = "zk")]
async fn run_ceremony_contribute(server_url: &str, seed_hex: Option<&str>) -> Result<()> {
    use omnia_adapters::setup::ceremony_server::ContributionReceipt;
    use omnia_adapters::setup::CeremonyClient;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Connecting to ceremony server at {server_url}...");

    // Parse optional seed
    let seed: Option<[u8; 32]> = seed_hex
        .map(|hex_str| {
            let bytes = hex::decode(hex_str).context("Failed to decode hex seed")?;
            let mut seed = [0u8; 32];
            if bytes.len() != 32 {
                anyhow::bail!(
                    "Seed must be exactly 32 bytes (64 hex chars), got {} bytes",
                    bytes.len()
                );
            }
            seed.copy_from_slice(&bytes);
            Ok(seed)
        })
        .transpose()?;

    // Build HTTP client
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to build HTTP client")?;

    // 1. Fetch current ceremony state
    println!("Fetching current ceremony state...");
    let state_url = format!("{server_url}/ceremony/state");
    let state_resp = client
        .get(&state_url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to ceremony server at {state_url}"))?;

    if !state_resp.status().is_success() {
        let status = state_resp.status();
        let body = state_resp.text().await.unwrap_or_default();
        anyhow::bail!("Ceremony server returned error {status} when fetching state: {body}");
    }

    let state: CeremonyStateResponse = state_resp
        .json()
        .await
        .context("Failed to deserialize ceremony state response")?;

    println!(
        "  Received state: transcript {} bytes, tau_size = {}",
        state.transcript.len(),
        state.tau_size
    );

    // 2. Generate contribution locally
    println!("Generating contribution...");
    let (contribution, _proof) = CeremonyClient::generate_contribution(&state.transcript, state.tau_size, seed)
        .map_err(|e| anyhow::anyhow!("Failed to generate contribution: {e}"))?;

    println!(
        "  Contribution generated: participant_id = {}",
        hex::encode(&contribution.participant_id[..8])
    );

    // 3. Submit contribution to server
    println!("Submitting contribution to server...");
    let contribute_url = format!("{server_url}/ceremony/contribute");
    let contribute_resp = client
        .post(&contribute_url)
        .json(&contribution)
        .send()
        .await
        .with_context(|| format!("Failed to submit contribution to {contribute_url}"))?;

    if !contribute_resp.status().is_success() {
        let status = contribute_resp.status();
        let body = contribute_resp.text().await.unwrap_or_default();
        anyhow::bail!("Ceremony server rejected contribution (HTTP {status}): {body}");
    }

    let receipt: ContributionReceipt = contribute_resp
        .json()
        .await
        .context("Failed to deserialize contribution receipt")?;

    println!("\n✓ Contribution accepted!");
    println!("  Contribution index: {}", receipt.contribution_index);
    println!("  Transcript hash: {}", hex::encode(&receipt.transcript_hash[..8]));
    println!("  Proof commitment: {}", hex::encode(&receipt.proof.commitment[..8]));

    Ok(())
}

/// Verify a ceremony transcript from a remote server.
///
/// Downloads the full transcript and independently verifies each
/// contribution's Proof of Knowledge.
///
/// # API Contract
///
/// - `GET {server_url}/ceremony/transcript` → `CeremonyTranscript` (JSON)
#[cfg(feature = "zk")]
async fn run_ceremony_verify(server_url: &str) -> Result<()> {
    use omnia_adapters::setup::ceremony_server::CeremonyTranscript;
    use omnia_adapters::setup::CeremonyClient;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Fetching ceremony transcript from {server_url}...");

    // Build HTTP client
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("Failed to build HTTP client")?;

    // 1. Fetch the full transcript
    let transcript_url = format!("{server_url}/ceremony/transcript");
    let resp = client
        .get(&transcript_url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to ceremony server at {transcript_url}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Ceremony server returned error {status} when fetching transcript: {body}");
    }

    let transcript: CeremonyTranscript = resp.json().await.context("Failed to deserialize ceremony transcript")?;

    println!("  Received transcript:");
    println!("    Contributions: {}", transcript.contribution_count);
    println!("    Ceremony ID: {}", transcript.config.ceremony_id);
    println!("    Degree: {}", transcript.config.degree);
    println!(
        "    Final hash: {}",
        hex::encode(&transcript.final_transcript_hash[..8])
    );

    // 2. Verify the transcript independently
    println!("\nVerifying transcript...");
    let degree = transcript.config.degree;
    let contribution_count = transcript.contribution_count;

    match CeremonyClient::verify_transcript(&transcript, degree) {
        Ok(true) => {
            println!("\n✓ Transcript verification succeeded!");
            println!("  All {contribution_count} contributions verified");
            println!("  Final transcript hash matches");
        }
        Ok(false) => {
            anyhow::bail!("Transcript verification failed: final hash mismatch");
        }
        Err(e) => {
            anyhow::bail!("Transcript verification failed: {e}");
        }
    }

    Ok(())
}

/// Response body for `GET /ceremony/state`.
///
/// Contains the current SRS transcript bytes and the number of G1 powers
/// needed to generate a contribution.
#[cfg(feature = "zk")]
#[derive(serde::Deserialize)]
struct CeremonyStateResponse {
    /// Current SRS transcript bytes.
    transcript: Vec<u8>,
    /// Number of G1 powers in the ceremony.
    tau_size: usize,
}
