# Changelog

> 🎯 Audience: Developers, Operators
> 🔗 Context: Version history and migration notes for all releases
> 📅 Last Updated: 2026-06-24

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.90](https://github.com/Willow7737/omnia-protocol/compare/v0.1.89...v0.1.90) (2026-08-06)


### Bug Fixes

* **docker:** create stub example file for cargo fetch in builder stage ([b85e530](https://github.com/Willow7737/omnia-protocol/commit/b85e530994e1e4e75fc19371e33a71357ec06e89))
* **docker:** create stub example file for cargo fetch in builder stage ([93b9306](https://github.com/Willow7737/omnia-protocol/commit/93b93064c2bfe0799a52c177f4cf11de4ea5845d))

## [0.1.89](https://github.com/Willow7737/omnia-protocol/compare/v0.1.88...v0.1.89) (2026-08-06)


### Features

* **adapters:** add live Bitcoin settlement adapter (bitcoin-live feature) ([47d96d8](https://github.com/Willow7737/omnia-protocol/commit/47d96d8e36159d09d8668dbbb9b8273fa52f8ed8))
* **adapters:** add live Bitcoin settlement adapter (bitcoin-live feature) ([01df4b2](https://github.com/Willow7737/omnia-protocol/commit/01df4b2f275fc6b2cd5ae6780011732580c35144))


### Bug Fixes

* **ci:** gate bitcoin_regtest_e2e example behind bitcoin-live feature ([3a5a22c](https://github.com/Willow7737/omnia-protocol/commit/3a5a22cc01a9b9dc36d4ac14b1b94cfa262b5e9a))
* **doc:** escape feature-gated doc links in bitcoin module ([eb0b6cb](https://github.com/Willow7737/omnia-protocol/commit/eb0b6cbe98c3adeda7fcd52ee2d54028b510b075))
* **fmt:** apply cargo fmt to bitcoin e2e example ([96a821e](https://github.com/Willow7737/omnia-protocol/commit/96a821efb5a3c731cfe050c2ee1240fd7141ae47))


### Tests

* **adapters:** add bitcoin regtest e2e example ([bd42782](https://github.com/Willow7737/omnia-protocol/commit/bd427828e95e201409ca69323f6296af65c1ed9b))

## [0.1.88](https://github.com/Willow7737/omnia-protocol/compare/v0.1.87...v0.1.88) (2026-08-05)


### Documentation

* update repository banner image ([acd9a48](https://github.com/Willow7737/omnia-protocol/commit/acd9a48a1af1921f56ccb5c8fda2c969b8c6cd55))

## [0.1.87](https://github.com/Willow7737/omnia-protocol/compare/v0.1.86...v0.1.87) (2026-08-04)


### Bug Fixes

* **auth:** create_token falls back to HS256 when RSA not configured ([8358e6d](https://github.com/Willow7737/omnia-protocol/commit/8358e6de735814d430a543813de7afdd5795949f))
* **ci:** resolve clippy errors — RfFingerprint::stub, unused mut, deprecated SubstrateConfig ([05d4147](https://github.com/Willow7737/omnia-protocol/commit/05d41475baf0eb16ac26654156556b99f33e2c12))
* **fmt:** add trailing comma in encode call for rustfmt ([4e866ca](https://github.com/Willow7737/omnia-protocol/commit/4e866cac86e8c95fdb17a9ef2fede644a4d33d5b))
* **fmt:** apply cargo fmt to auth.rs method chain formatting ([41a95df](https://github.com/Willow7737/omnia-protocol/commit/41a95df9d91d9326e3e1e73b0b75845be012c625))
* **monitoring:** add omnia_node_cpu_usage_ratio metric to replace missing process_cpu_seconds_total ([6bce45c](https://github.com/Willow7737/omnia-protocol/commit/6bce45cb259ef9d4c6411611a53d9d5c8932d993))
* **tests:** adapt api_integration to RS256 auth migration ([8bb86d5](https://github.com/Willow7737/omnia-protocol/commit/8bb86d5918d10fc41bb3e280cdd29ed096c92edf))
* **tests:** allow legacy HS256 JWTs in docker e2e tests ([29436e0](https://github.com/Willow7737/omnia-protocol/commit/29436e0927de6ac973469445e55b77b352b726ac))
* **tests:** allow legacy HS256 JWTs in integration tests ([4156be8](https://github.com/Willow7737/omnia-protocol/commit/4156be8272c3b9068445817955af25c5f8608d54))
* **tests:** reset JWT secret cache in all api_integration test helpers ([47f562e](https://github.com/Willow7737/omnia-protocol/commit/47f562e3708e7b9692d8f1c515aa0de32295323a))

## [0.1.86](https://github.com/Willow7737/omnia-protocol/compare/v0.1.85...v0.1.86) (2026-08-03)


### Bug Fixes

* **tests:** use saturating_sub to prevent Instant overflow on short-lived CI runners ([a230f58](https://github.com/Willow7737/omnia-protocol/commit/a230f581d1e9401c644377aff9a7e5c6ac5a558d))

## [0.1.85](https://github.com/Willow7737/omnia-protocol/compare/v0.1.84...v0.1.85) (2026-08-01)


### Features

* configurable mint authority (fixes per-node divergence) + standing-mesh docs ([4bd58b4](https://github.com/Willow7737/omnia-protocol/commit/4bd58b436fe6f7ba2a815f8df468a5e657810da6))
* **consensus:** real EC-VRF leader election + unpredictable beacon (AUDIT-2026-07 C1, ADR-026 Phase 1) ([c6bd1d0](https://github.com/Willow7737/omnia-protocol/commit/c6bd1d007e6fc0e706d86d53111713fdf497782e))
* **consensus:** real EC-VRF leader election + unpredictable beacon (C1, [#339](https://github.com/Willow7737/omnia-protocol/issues/339), ADR-026) ([3aa16db](https://github.com/Willow7737/omnia-protocol/commit/3aa16dba9725ada43b6059dadca5200cf4358953))
* **crypto:** true t-of-n threshold BLS via Lagrange combination (AUDIT-2026-07 C2) ([57c8a87](https://github.com/Willow7737/omnia-protocol/commit/57c8a8798b567c6945c5b26893f47163ff94f7bf))
* **crypto:** true t-of-n threshold BLS via Lagrange combination (C2, [#340](https://github.com/Willow7737/omnia-protocol/issues/340)) ([b2d59a3](https://github.com/Willow7737/omnia-protocol/commit/b2d59a32f271c669b6cfe1ef0e23457f081a69ef))
* **node:** configurable mint authority, and fix the per-node divergence it caused ([0bc50d2](https://github.com/Willow7737/omnia-protocol/commit/0bc50d255795ba6035c515ea930686d8c8d56016))
* **node:** expose the financial ledger so wallets can actually pay people ([d2f7a1f](https://github.com/Willow7737/omnia-protocol/commit/d2f7a1f3d138e9e8d41fd2967e0c4b12d7f22a74))
* **shards:** add SignedTransfer so a wallet can move its own funds ([0ddf7c8](https://github.com/Willow7737/omnia-protocol/commit/0ddf7c8337d808c1466c05e138f6af34deec93ab))
* **substrate:** explicit finality lifecycle for Lane 0/Lane 1 (H5, [#355](https://github.com/Willow7737/omnia-protocol/issues/355)) ([44171f2](https://github.com/Willow7737/omnia-protocol/commit/44171f2e5d23bb9a7ea37f940ece322d49b0939a))
* **substrate:** explicit finality lifecycle for Lane 0/Lane 1 (H5, [#355](https://github.com/Willow7737/omnia-protocol/issues/355)) ([45cf2ec](https://github.com/Willow7737/omnia-protocol/commit/45cf2ec8edf1ba10edc250634009f9b0416a5333))


### Bug Fixes

* **adapters:** ZK rollup proves non-empty batches; real settlement ABI (AUDIT-2026-07 C3) ([1e0f978](https://github.com/Willow7737/omnia-protocol/commit/1e0f9783aba6f7b9fd1c5afe04737258b297c6b9))
* **adapters:** ZK rollup proves non-empty batches; real settlement ABI (C3, [#341](https://github.com/Willow7737/omnia-protocol/issues/341)) ([270584a](https://github.com/Willow7737/omnia-protocol/commit/270584a1c985f8e9703ea0566013c88ab9586f1c))
* **consensus:** compute BFT threshold over the active validator set (H2, [#352](https://github.com/Willow7737/omnia-protocol/issues/352)) ([b1e0610](https://github.com/Willow7737/omnia-protocol/commit/b1e0610579084bf171621bdb2cdc877678c52bb6))
* **consensus:** compute BFT threshold over the active validator set (H2, [#352](https://github.com/Willow7737/omnia-protocol/issues/352)) ([33ee8e8](https://github.com/Willow7737/omnia-protocol/commit/33ee8e892cad928a17c4d9279a4412b8453ca70f))
* **consensus:** enforce leader eligibility on the propose path (H3, [#353](https://github.com/Willow7737/omnia-protocol/issues/353)) ([#399](https://github.com/Willow7737/omnia-protocol/issues/399)) ([b5123d0](https://github.com/Willow7737/omnia-protocol/commit/b5123d0362d776c26973249b162abd85ac1f5e79))
* **consensus:** governance-authorized, persistent slashing undo (AUDIT-2026-07 C6) ([5d400d8](https://github.com/Willow7737/omnia-protocol/commit/5d400d8ced75598e61579d4bebc0b40d66fb8c09))
* **consensus:** governance-authorized, persistent slashing undo (C6, [#344](https://github.com/Willow7737/omnia-protocol/issues/344)) ([99f17c0](https://github.com/Willow7737/omnia-protocol/commit/99f17c06ca6843cbf7fc5818a041333ab3f44463))
* **consensus:** pruning-invariant finalized state-root accumulator (H1, [#351](https://github.com/Willow7737/omnia-protocol/issues/351)) ([c84d67a](https://github.com/Willow7737/omnia-protocol/commit/c84d67a8045215dc8319285aa74762c5efa3e2cd))
* **consensus:** pruning-invariant finalized state-root accumulator (H1, [#351](https://github.com/Willow7737/omnia-protocol/issues/351)) ([18f0c14](https://github.com/Willow7737/omnia-protocol/commit/18f0c148400073d3b58a5c9fd43eb446b9e4cb02))
* **deps:** bump ruint to 1.20.0 for RUSTSEC-2026-0220 ([0a7a1ac](https://github.com/Willow7737/omnia-protocol/commit/0a7a1acfb0c5ef675e41cda0ba084a41567dc924))
* **financial:** transfer is atomic — no sender debit on recipient overflow ([c597cc6](https://github.com/Willow7737/omnia-protocol/commit/c597cc6cd959e4634f5c36bc6461628afdf2b5d1))
* **financial:** transfer is atomic — no sender debit on recipient overflow ([#343](https://github.com/Willow7737/omnia-protocol/issues/343)) ([bb7342c](https://github.com/Willow7737/omnia-protocol/commit/bb7342ccc65f7c5f89042d3c22fd0f407865c9bd))
* **gossip:** break deferral-queue priority-inversion deadlock ([5788fb2](https://github.com/Willow7737/omnia-protocol/commit/5788fb24762837c58156160129a858f41c9909d1))
* **gossip:** break deferral-queue priority-inversion deadlock ([658b249](https://github.com/Willow7737/omnia-protocol/commit/658b249d1922abe3bfc8145cb0e8aba0720229f3))
* **gossip:** break deferral-queue priority-inversion deadlock ([#326](https://github.com/Willow7737/omnia-protocol/issues/326)) ([2453288](https://github.com/Willow7737/omnia-protocol/commit/2453288821cc1f0ede43025247ea133b5ac27a5c))
* **gossip:** solicited repair events bypass the per-peer rate limiter ([d3a4bad](https://github.com/Willow7737/omnia-protocol/commit/d3a4bad3ce14f856a1a7f9dbaa4925ff70d4bf62))
* **gossip:** solicited repair events bypass the per-peer rate limiter ([bda4027](https://github.com/Willow7737/omnia-protocol/commit/bda40276b7b3fb23c2365eb1cec5ff1d0887806c))
* **network:** bind fast-sync snapshot to the supermajority attestation (C10, [#348](https://github.com/Willow7737/omnia-protocol/issues/348)) ([f73264a](https://github.com/Willow7737/omnia-protocol/commit/f73264a36efaa0af2f9a8e86f5e55127972b5cb7))
* **network:** bind fast-sync snapshot to the supermajority attestation (C10, [#348](https://github.com/Willow7737/omnia-protocol/issues/348)) ([99968a1](https://github.com/Willow7737/omnia-protocol/commit/99968a126197057a234ef1eeac2128d1b8d7bf25))
* **network:** raise gossipsub max_transmit_size — repair batches never fit in 64 KiB ([a2751ef](https://github.com/Willow7737/omnia-protocol/commit/a2751ef15ab253c7ddd0effe101b2bdf0149618e))
* **network:** raise gossipsub max_transmit_size — repair batches never fit in 64 KiB ([a9421da](https://github.com/Willow7737/omnia-protocol/commit/a9421da391f8eaa14f56374a613d7bbe69542a9d))
* **node:** reject known-weak JWT secret at startup + require it in compose (C11, [#349](https://github.com/Willow7737/omnia-protocol/issues/349)) ([b8fd944](https://github.com/Willow7737/omnia-protocol/commit/b8fd944780eb6610c4479e70cede825ba360c396))
* **node:** reject known-weak JWT secret at startup + require it in compose (C11, [#349](https://github.com/Willow7737/omnia-protocol/issues/349)) ([442161f](https://github.com/Willow7737/omnia-protocol/commit/442161f6334cdd6717109e7f51a92990293ae97e))
* **node:** restore protocol_version's doc comment ([7554e18](https://github.com/Willow7737/omnia-protocol/commit/7554e18a33d8daf5c70dad25139d69ab9ba2cee4))
* **shards:** cross-shard messages require source-shard attestation (AUDIT-2026-07 C4) ([cd0b38d](https://github.com/Willow7737/omnia-protocol/commit/cd0b38d9ec36d98ebb2ecf9b28ece06e266ad7e0))
* **shards:** cross-shard messages require source-shard attestation (C4, [#342](https://github.com/Willow7737/omnia-protocol/issues/342)) ([33538a4](https://github.com/Willow7737/omnia-protocol/commit/33538a402a42fe6d4d6be71105acf81a6d83c7c2))
* **shards:** persist nonce before acknowledging it in memory (AUDIT-2026-07 C8, [#346](https://github.com/Willow7737/omnia-protocol/issues/346)) ([#381](https://github.com/Willow7737/omnia-protocol/issues/381)) ([03c5140](https://github.com/Willow7737/omnia-protocol/commit/03c5140b72122ca991019e4e8c678fe328e22b1b))
* **shards:** persist nonce before acknowledging it in memory (C8, [#346](https://github.com/Willow7737/omnia-protocol/issues/346)) ([e8abef5](https://github.com/Willow7737/omnia-protocol/commit/e8abef5cccc95c6ce38b9515dca9d5ae165097bc))
* **shards:** persist nonce before acknowledging it in memory (C8, [#346](https://github.com/Willow7737/omnia-protocol/issues/346)) ([5ae82a9](https://github.com/Willow7737/omnia-protocol/commit/5ae82a94df8ebdbce622a945f8500b70672da137))
* **shards:** ZK verifying keys come from a VK registry, never the caller (AUDIT-2026-07 C9, [#347](https://github.com/Willow7737/omnia-protocol/issues/347)) ([#382](https://github.com/Willow7737/omnia-protocol/issues/382)) ([93a4ca2](https://github.com/Willow7737/omnia-protocol/commit/93a4ca28e8ece9ab7b50da4b1470f8a4974fadc3))
* **shards:** ZK verifying keys come from a VK registry, never the caller (C9, [#347](https://github.com/Willow7737/omnia-protocol/issues/347)) ([e505cd6](https://github.com/Willow7737/omnia-protocol/commit/e505cd6f7e4e0564fe88080fb2c35d6a324d153c))
* **shards:** ZK verifying keys come from a VK registry, never the caller (C9, [#347](https://github.com/Willow7737/omnia-protocol/issues/347)) ([ee57466](https://github.com/Willow7737/omnia-protocol/commit/ee5746619ceaaec08ffe83f6f3d6bfd7d90cd4d2))
* **substrate:** bind Lane 0 acks to the post-apply state root (H4, [#354](https://github.com/Willow7737/omnia-protocol/issues/354)) ([37e9b01](https://github.com/Willow7737/omnia-protocol/commit/37e9b0132868d441465eee5f8f1b1e32ae68fb04))
* **substrate:** bind Lane 0 acks to the post-apply state root (H4, [#354](https://github.com/Willow7737/omnia-protocol/issues/354)) ([f550d48](https://github.com/Willow7737/omnia-protocol/commit/f550d4877443477c43bea33e16a402907780bec3))
* **substrate:** persist Lane 0 finality certificates across restart (C7, [#345](https://github.com/Willow7737/omnia-protocol/issues/345)) ([#394](https://github.com/Willow7737/omnia-protocol/issues/394)) ([57af4bc](https://github.com/Willow7737/omnia-protocol/commit/57af4bc61a5b5c50be0378474b1dc0193a38cec1))


### Performance

* **gossip:** has_more fast drain + live benchmark progress ([3d5edd1](https://github.com/Willow7737/omnia-protocol/commit/3d5edd18757ced504b4616437f578af475e5d31f))
* **gossip:** has_more fast drain + live benchmark progress ([fa38591](https://github.com/Willow7737/omnia-protocol/commit/fa385913add10b86e2210676744ecf48876c7286))
* **gossip:** raise anti-entropy repair throughput ~order of magnitude ([6facbf8](https://github.com/Willow7737/omnia-protocol/commit/6facbf80d76638e1f24ca87eb46bf1272082a2a3))
* **gossip:** raise anti-entropy repair throughput ~order of magnitude ([f1693e3](https://github.com/Willow7737/omnia-protocol/commit/f1693e3b81827e6711df6fc2e7de0d49f3438e5d))


### Documentation

* bootstrap the GitHub wiki — 8 pages, repo-versioned, auto-published ([88903fd](https://github.com/Willow7737/omnia-protocol/commit/88903fdd281a0e59cff735841387323c9d18acb7))
* bootstrap the GitHub wiki — 8 pages, repo-versioned, auto-published ([4f5b441](https://github.com/Willow7737/omnia-protocol/commit/4f5b441a39b2ae602cffa289ee0211a30a67fc64))
* **consensus:** fix rustdoc intra-doc links in vrf_election (C1) ([bff6534](https://github.com/Willow7737/omnia-protocol/commit/bff6534fe7e9ea183ec24940520b01ab5ce57e16))
* describe the two economies and the new financial endpoints ([a21028f](https://github.com/Willow7737/omnia-protocol/commit/a21028fc3b2f35afd50473130729edfe2be11cb8))
* record the 10k lossless-convergence milestone (post-[#330](https://github.com/Willow7737/omnia-protocol/issues/330)) ([923a11c](https://github.com/Willow7737/omnia-protocol/commit/923a11c2ed79fbeae8d5ab58ebf0ad0faa9a32c8))
* record the 10k lossless-convergence milestone (post-[#330](https://github.com/Willow7737/omnia-protocol/issues/330)) ([1ca9db6](https://github.com/Willow7737/omnia-protocol/commit/1ca9db676d2ee94833afdb8e961d913d685c5d3d))
* record the 5-node 10k headline run + measured fast-drain numbers ([f936ae8](https://github.com/Willow7737/omnia-protocol/commit/f936ae8e0f6cd96fa3ef3367997ae5bc618d21d1))
* record the 5-node 10k headline run + measured fast-drain numbers ([f634b2f](https://github.com/Willow7737/omnia-protocol/commit/f634b2f459b62abd5c36ef058ba622246e3e2b3c))
* record the geo-distributed WAN campaign — the asterisk is gone ([8ccd050](https://github.com/Willow7737/omnia-protocol/commit/8ccd050284bb04b207bad50929024cd0e99fed78))
* record the geo-distributed WAN campaign — the asterisk is gone ([55bde20](https://github.com/Willow7737/omnia-protocol/commit/55bde20f910834a7e4033c145389206b91f0abc6))
* record the geo-distributed WAN campaign — the asterisk is gone ([#338](https://github.com/Willow7737/omnia-protocol/issues/338)) ([a3f87a6](https://github.com/Willow7737/omnia-protocol/commit/a3f87a67468c44ae01cb61a49e22a690db35247c))
* staleness audit — multi-node testnet is live, honest 10k status ([7aa5e3c](https://github.com/Willow7737/omnia-protocol/commit/7aa5e3c6a49febc3b61dc8fc95c052e327076839))
* staleness audit — multi-node testnet is live, honest 10k status ([24983d9](https://github.com/Willow7737/omnia-protocol/commit/24983d90e6a0bc735377c9c808d6f70d5966494e))
* state the network's actual condition instead of implying a standing mesh ([9bf9f11](https://github.com/Willow7737/omnia-protocol/commit/9bf9f11555d3629ed05cf98f5e50ad718eebc9e8))
* state the network's actual condition instead of implying a standing mesh ([04b896a](https://github.com/Willow7737/omnia-protocol/commit/04b896a1e9aa747c0c5963b4c7b736f83a507b4a))
* the validator network is standing now — say so, and say what it isn't ([b87e09a](https://github.com/Willow7737/omnia-protocol/commit/b87e09a7f542ec6656cffb21fe5f077588132462))


### Tests

* **e2e:** consensus test can no longer silently pass on failure ([45ee1e4](https://github.com/Willow7737/omnia-protocol/commit/45ee1e4497c761988153ac0698b65018550423ad))
* **e2e:** consensus test can no longer silently pass on failure ([#350](https://github.com/Willow7737/omnia-protocol/issues/350)) ([fb29211](https://github.com/Willow7737/omnia-protocol/commit/fb292113da7f048c9bdc0c298911369534ed4b07))
* **node:** pin the wallet's exact wire payload against the HTTP endpoint ([e9f300d](https://github.com/Willow7737/omnia-protocol/commit/e9f300d6dede26211ffdaf6b9a87eea18c14d7d4))


### CI

* re-baseline binary size gate 16 -&gt; 17 MiB for release overflow-checks ([3c1166a](https://github.com/Willow7737/omnia-protocol/commit/3c1166a346b11d627156c56e38e0cb64168c0967))

## [0.1.84](https://github.com/Willow7737/omnia-protocol/compare/v0.1.83...v0.1.84) (2026-07-18)


### Features

* **gossip:** anti-entropy repair — periodic frontier digests + missing-event recovery ([d0cd2f5](https://github.com/Willow7737/omnia-protocol/commit/d0cd2f59ccf19ea3224e670f0cc4e84ae813686d))
* **gossip:** anti-entropy repair — periodic frontier digests + missing-event recovery ([4cce4d4](https://github.com/Willow7737/omnia-protocol/commit/4cce4d49b3157ed5a329b9a497eda6dda2e829a4))
* **gossip:** anti-entropy repair — periodic frontier digests + missing-event recovery ([#320](https://github.com/Willow7737/omnia-protocol/issues/320)) ([66a05f5](https://github.com/Willow7737/omnia-protocol/commit/66a05f5551af65d7c472507e968d64436a423225))
* **lane0:** diagnosable OMNIA_LANE0_VALIDATORS parse errors ([2b8f9ff](https://github.com/Willow7737/omnia-protocol/commit/2b8f9ff4343e192ae9655ec13f44a5e195bc2e76))
* **lane0:** diagnosable OMNIA_LANE0_VALIDATORS parse errors ([#302](https://github.com/Willow7737/omnia-protocol/issues/302)) ([8a6af15](https://github.com/Willow7737/omnia-protocol/commit/8a6af1539ed3a101aae0a58be3881e432096e950))
* **testnet:** worker mesh topology, Lane 0 finality metric, deferral observability ([869f6dc](https://github.com/Willow7737/omnia-protocol/commit/869f6dc17b7e2456008dbf8c4e997b8e97a71c3a))
* **testnet:** worker mesh topology, Lane 0 finality metric, deferral observability ([a648777](https://github.com/Willow7737/omnia-protocol/commit/a6487775fb12434c3bd28e920f4c23410f8e9241))


### Bug Fixes

* **gossip:** defer out-of-window events instead of losing them to gap rejects ([2207e69](https://github.com/Willow7737/omnia-protocol/commit/2207e69afd3c6b58cbdfc2b0a847cad8c6a721be))
* **gossip:** defer out-of-window events instead of losing them to gap rejects ([e1da764](https://github.com/Willow7737/omnia-protocol/commit/e1da76488421189dff33b7d3a4c61f2422971ed2))
* **lane0:** stable rustfmt formatting for validator parse errors ([9cfd4b4](https://github.com/Willow7737/omnia-protocol/commit/9cfd4b4ef58224eead4b471a3017ae27f5b7418a))
* **lane0:** stable rustfmt formatting for validator parse errors ([3d0f115](https://github.com/Willow7737/omnia-protocol/commit/3d0f115a1fc05360edc0fa90898b93bc5c8d5260))
* **network:** add identify so Kademlia populates and peers discover each other ([8a005b3](https://github.com/Willow7737/omnia-protocol/commit/8a005b3f0d4355e758e4417e3529293f9401c64a))
* **network:** add identify so Kademlia populates and peers discover each other ([7d15ac2](https://github.com/Willow7737/omnia-protocol/commit/7d15ac2463ce3f424d836ca743f09cb53860146d))
* **network:** gossipsub mesh scoring + rate-limit deferral — 100% testnet propagation ([#314](https://github.com/Willow7737/omnia-protocol/issues/314)) ([968f7a6](https://github.com/Willow7737/omnia-protocol/commit/968f7a6ddc3a1cae76d711b361c4b0a0968fe766))
* **network:** resolve /dns4 bootstrap addresses via DNS transport ([86db207](https://github.com/Willow7737/omnia-protocol/commit/86db20712650f383d8449daf07880ee748dd76ed))
* **network:** resolve /dns4 bootstrap addresses via DNS transport ([88845dd](https://github.com/Willow7737/omnia-protocol/commit/88845dd363d0f07cb00913087b92e4f92465b06d))
* **network:** resolve /dns4 bootstrap addresses via DNS transport ([#303](https://github.com/Willow7737/omnia-protocol/issues/303)) ([8f6b0fe](https://github.com/Willow7737/omnia-protocol/commit/8f6b0feb3f5cb9310f6449278de27ca874daffe8))


### Documentation

* **benchmarks:** record full Stage 2 multi-node load matrix ([d6331af](https://github.com/Willow7737/omnia-protocol/commit/d6331afaf79eb7e905d6f00ca78279e4564b1a5b))
* **benchmarks:** record full Stage 2 multi-node load matrix ([323de5a](https://github.com/Willow7737/omnia-protocol/commit/323de5ab3dbc2f96c79fbc6cf006ab0e48de1044))
* refresh all status-bearing markdown for the live meshed testnet ([b279f15](https://github.com/Willow7737/omnia-protocol/commit/b279f152a737eeb8a4f575525a2384bfa9cfd19d))
* refresh all status-bearing markdown for the live meshed testnet ([05ceb8c](https://github.com/Willow7737/omnia-protocol/commit/05ceb8c4f840941221c4a950213adc55315b4fd6))
* refresh all status-bearing markdown for the live meshed testnet ([#321](https://github.com/Willow7737/omnia-protocol/issues/321)) ([b870de8](https://github.com/Willow7737/omnia-protocol/commit/b870de83382b0526c402de397dbd06c37eef690a))

## [0.1.83](https://github.com/Willow7737/omnia-protocol/compare/v0.1.82...v0.1.83) (2026-07-15)


### Features

* **consensus:** epoch-fenced Lane 0 validator rotation (ADR-025 Stage 4) ([#290](https://github.com/Willow7737/omnia-protocol/issues/290)) ([1c3d40d](https://github.com/Willow7737/omnia-protocol/commit/1c3d40d130d414aed75420f2b56fbd25cffcb9f4))
* **consensus:** Lane 1-committed validator-set-change trigger (ADR-025) ([6731b93](https://github.com/Willow7737/omnia-protocol/commit/6731b9335fc34926fc4dd83ca1b2ce970d957d19))
* **consensus:** Lane 1-committed validator-set-change trigger (ADR-025) ([71e4b14](https://github.com/Willow7737/omnia-protocol/commit/71e4b1441df1201c5786bb6b7ce6d2698dd14cf2))
* **economics:** transfers become on-chain events (ADR-025 Lane 0, Step 1a) ([f150b34](https://github.com/Willow7737/omnia-protocol/commit/f150b34cc94382db0b86d4c7730b8d3deda9a34c))
* **economics:** transfers become on-chain events (ADR-025 Lane 0, Step 1a) ([feb91e5](https://github.com/Willow7737/omnia-protocol/commit/feb91e59fe3531c63ec9c3880f637913043a92fc))
* **node:** wallet-signed self-sovereign spend authorization (Step 2) ([3316c41](https://github.com/Willow7737/omnia-protocol/commit/3316c41931876e733bda34d465e797b27d143269))
* **node:** wallet-signed self-sovereign spend authorization (Step 2) ([a17f3b4](https://github.com/Willow7737/omnia-protocol/commit/a17f3b48bb3d83ba41fb39bf031f0ecd3f561ec7))
* **node:** wallet-signed self-sovereign spend authorization (Step 2) ([bfd50a3](https://github.com/Willow7737/omnia-protocol/commit/bfd50a3e29f6c7428deaace1c70214e2f69d98de))


### Bug Fixes

* **consensus:** bound the out-of-order buffer's creator map (H-4) ([ca2748e](https://github.com/Willow7737/omnia-protocol/commit/ca2748ef29d79d1c736e44ddacd3379572ff930b))
* **consensus:** bound the out-of-order buffer's creator map (H-4) ([d137244](https://github.com/Willow7737/omnia-protocol/commit/d13724481144d5a4b787755dbd0f14ffe5332428))
* restart-safe finalized_height + gossip keepalive ([#287](https://github.com/Willow7737/omnia-protocol/issues/287)) ([388a7e9](https://github.com/Willow7737/omnia-protocol/commit/388a7e909d475465de5bf97abbb2049231b64479))
* restart-safe finalized_height + gossip keepalive ([#287](https://github.com/Willow7737/omnia-protocol/issues/287)) ([eccc69b](https://github.com/Willow7737/omnia-protocol/commit/eccc69b292068b8e6ffbbb3a5f3e367e7ee8b388))
* **tla:** rewrite OmniaCRDT to be TLC-runnable; rejoin the CI matrix ([7dc0e51](https://github.com/Willow7737/omnia-protocol/commit/7dc0e51aa1fdaf709ab3a1e6e8d5cd16b0deb8e0))
* **tla:** rewrite OmniaCRDT to be TLC-runnable; rejoin the CI matrix ([2e23131](https://github.com/Willow7737/omnia-protocol/commit/2e23131cabf184d3060e1b3861518e9f492d536d))
* **tla:** rewrite OmniaCRDT to be TLC-runnable; rejoin the CI matrix ([51823c3](https://github.com/Willow7737/omnia-protocol/commit/51823c35d0beb003eb8ac5ec9bb6afc5da9dda9f))
* **tla:** rewrite OmniaCRDT to be TLC-runnable; rejoin the CI matrix ([#296](https://github.com/Willow7737/omnia-protocol/issues/296)) ([1e2641e](https://github.com/Willow7737/omnia-protocol/commit/1e2641e30c7d0d1727caedb8252002889319ac55))


### Refactoring

* **economics:** single source of truth (Step 1b, resolves C4) ([3803ac4](https://github.com/Willow7737/omnia-protocol/commit/3803ac4e24d4a29265e220912db135e5eaf9c618))
* **economics:** single source of truth (Step 1b, resolves C4) ([b328e2c](https://github.com/Willow7737/omnia-protocol/commit/b328e2c41ee5c0941fcd6563054501a1b076368b))


### Documentation

* **shards:** fix broken intra-doc links to EconomicsState ([c3b9983](https://github.com/Willow7737/omnia-protocol/commit/c3b9983bcc0bf5ebac83285582f369ae429ae442))


### Tests

* **consensus:** Lane 0 adversarial arena — property-based CI gate (ADR-025 Stage 5) ([8ae5f33](https://github.com/Willow7737/omnia-protocol/commit/8ae5f33fc691f28c9e7f2c5dc91224c19c4677f4))
* **consensus:** Lane 0 adversarial arena — property-based CI gate (ADR-025 Stage 5) ([7aab14a](https://github.com/Willow7737/omnia-protocol/commit/7aab14a391e52e4cef53e44d29c118575a2d2e4d))
* **substrate:** fix OMNIA_CONSENSUS_SEED test-isolation race in config construction ([#300](https://github.com/Willow7737/omnia-protocol/issues/300)) ([87cd3b8](https://github.com/Willow7737/omnia-protocol/commit/87cd3b8f282fe656213cc1308bf0c240925522fe))
* **substrate:** route config construction through locked test_config helper ([537d89e](https://github.com/Willow7737/omnia-protocol/commit/537d89eec912e62b0b0153af823c568ef5a11d7b))
* **substrate:** route config construction through locked test_config helper ([3e5ab42](https://github.com/Willow7737/omnia-protocol/commit/3e5ab42b75b7d76ffa652580c62a419fc167d852))


### CI

* TLC model-check gate for the TLA+ specs ([#295](https://github.com/Willow7737/omnia-protocol/issues/295)) ([ba0f908](https://github.com/Willow7737/omnia-protocol/commit/ba0f9082340517e1d1f2c9344338f35767293786))

## [0.1.82](https://github.com/Willow7737/omnia-protocol/compare/v0.1.81...v0.1.82) (2026-07-11)


### Features

* **consensus:** Lane 0 consensusless fast-path finality (ADR-025 Stage 3, v1) ([f9b350a](https://github.com/Willow7737/omnia-protocol/commit/f9b350ac2ce4f0b624ee078a0a99b9b398117155))
* **consensus:** Lane 0 consensusless fast-path finality (ADR-025 Stage 3, v1) ([934aed7](https://github.com/Willow7737/omnia-protocol/commit/934aed7e49c4485fd4d60e7b5c7e8a7efb93ebfe))
* **consensus:** Lane 0 consensusless fast-path finality (ADR-025 Stage 3, v1) ([#278](https://github.com/Willow7737/omnia-protocol/issues/278)) ([14a90d9](https://github.com/Willow7737/omnia-protocol/commit/14a90d9090cc0a523d158296b066288eee57f879))
* **node:** add wallet challenge/signature auth endpoints ([395bf02](https://github.com/Willow7737/omnia-protocol/commit/395bf022158059db111ff4e71f283569143fcd8b))
* **node:** POST /api/v1/auth/register — register the authenticated caller's DID ([#271](https://github.com/Willow7737/omnia-protocol/issues/271)) ([b2bb717](https://github.com/Willow7737/omnia-protocol/commit/b2bb717ab5ecb43dc013cf507feb6a6582c5ea73))
* **node:** POST /api/v1/auth/register — register the authenticated caller's DID ([#271](https://github.com/Willow7737/omnia-protocol/issues/271)) ([8b22332](https://github.com/Willow7737/omnia-protocol/commit/8b223327e7ba036be2310fb4dcd5f4dfe69fe5a0))
* **ops:** ADR-025 Stage 2 tooling — testnet benchmark + live node metrics ([446bcba](https://github.com/Willow7737/omnia-protocol/commit/446bcbaf307086a628b16a4860875613391fc8d5))
* **ops:** ADR-025 Stage 2 tooling — testnet benchmark + live node metrics ([3947df2](https://github.com/Willow7737/omnia-protocol/commit/3947df27eac1da4c7bfb2edff0f3fc7ef4f3320f))
* **ops:** ADR-025 Stage 2 tooling — testnet benchmark + live node metrics ([e3debb0](https://github.com/Willow7737/omnia-protocol/commit/e3debb0c1b3a17d3a15daa2546129b4efab4530f))
* **ops:** ADR-025 Stage 2 tooling — testnet benchmark + live node metrics ([#277](https://github.com/Willow7737/omnia-protocol/issues/277)) ([7ba2c2a](https://github.com/Willow7737/omnia-protocol/commit/7ba2c2a70ae4dfa28acc322657edb4cbbe0a4724))


### Performance

* **network:** integrate idle gossip components (AUDIT-14, ADR-025 Stage 1) ([#276](https://github.com/Willow7737/omnia-protocol/issues/276)) ([2ffdfec](https://github.com/Willow7737/omnia-protocol/commit/2ffdfecfb2ef3612ef9dcfa6b3637ac22b23324e))


### Refactoring

* **node:** derive wallet DID with SHA-256 for cross-client parity ([45a62fe](https://github.com/Willow7737/omnia-protocol/commit/45a62feec94556b8e02662f752de87978e5387df))
* **node:** derive wallet DID with SHA-256 for cross-client parity ([e1892ac](https://github.com/Willow7737/omnia-protocol/commit/e1892ac3518e26d127089d304b2572a3ef9d168e))


### Documentation

* **adr:** ADR-025 two-lane consensus + catch adr-index up to 025 ([ba3940c](https://github.com/Willow7737/omnia-protocol/commit/ba3940c61a42d9f5aa62193f594dbbd673ba51f3))
* **adr:** ADR-025 Two-Lane Consensus + catch adr-index up to 025 ([e94e25b](https://github.com/Willow7737/omnia-protocol/commit/e94e25bcc9c1357ac87b5fbd663108542bce5cf2))
* **bench:** record 2026-07-09 local reference run (v0.1.76+/dev) ([1e36a7b](https://github.com/Willow7737/omnia-protocol/commit/1e36a7b2d4cf48cb9f05a7b3be9a6b212bce79b9))
* reflect live testnet + shipped wallet ecosystem ([7b21ec5](https://github.com/Willow7737/omnia-protocol/commit/7b21ec50a3d732d887fe70dfdaa711e781e5e92f))
* reflect live testnet + shipped wallet; record fresh benchmark run ([66338d9](https://github.com/Willow7737/omnia-protocol/commit/66338d9989a372b62628330b34f6a14da2b8e0ea))

## [0.1.81](https://github.com/Willow7737/omnia-protocol/compare/v0.1.80...v0.1.81) (2026-07-03)


### Bug Fixes

* **docker:** add bootstrap peer ID to OMNIA_BOOTSTRAP_NODES ([f615b46](https://github.com/Willow7737/omnia-protocol/commit/f615b46fd671a32b13a1c61625d1951608e3b945))
* **docker:** add bootstrap peer ID to OMNIA_BOOTSTRAP_NODES ([18da0ba](https://github.com/Willow7737/omnia-protocol/commit/18da0ba6d43e395e3fc6e6067dd8594c73a162d3))
* **test:** reset JWT secret cache + use try_new in integration tests ([5d84153](https://github.com/Willow7737/omnia-protocol/commit/5d841532259d57729ee4297f852e68b5dd911d0d))

## [0.1.80](https://github.com/Willow7737/omnia-protocol/compare/v0.1.79...v0.1.80) (2026-06-30)


### Bug Fixes

* **audit:** resolve 6 findings from third-pass audit (NEW-C1 through NEW-L1) ([62855b5](https://github.com/Willow7737/omnia-protocol/commit/62855b502c031bd5618c0767daf88083582dde77))
* **audit:** resolve 6 findings from third-pass audit (NEW-C1 through NEW-L1) ([25ac0ec](https://github.com/Willow7737/omnia-protocol/commit/25ac0ec8548a576427aa658bfc0e46567d561924))
* **ci:** handle to_bytes() Result + fmt + dead_code warning ([dbe6237](https://github.com/Willow7737/omnia-protocol/commit/dbe62379342da02866b6e7487f3b4957ef9aee2e))
* **ci:** remove unused import + clear env var in tests + use try_new ([1e663bb](https://github.com/Willow7737/omnia-protocol/commit/1e663bbe831c33a1ca1ddbd16cac801f7824cb3a))
* **ci:** use InvalidTotalNodes variant + fmt fixes ([03033cf](https://github.com/Willow7737/omnia-protocol/commit/03033cf778bcecc1f256910ad085d697d3ee1e19))
* **test:** generate real Ed25519 signature for PoUW test proof ([44687e8](https://github.com/Willow7737/omnia-protocol/commit/44687e8e0df052a8921b70f71ef59bb8d4e11691))
* **test:** sign cross-shard messages in fee_enforcement tests ([7c68400](https://github.com/Willow7737/omnia-protocol/commit/7c6840054810847f6b2c0c38edb9f3d575d81625))
* **test:** use same verifier keypair for state + proof signing ([4bedc3e](https://github.com/Willow7737/omnia-protocol/commit/4bedc3ecb62bd8c85ef59dab4e5aa32ebc157bcd))

## [0.1.79](https://github.com/Willow7737/omnia-protocol/compare/v0.1.78...v0.1.79) (2026-06-29)


### Bug Fixes

* **audit:** resolve 9 findings from second architecture audit (F-1 to F-27) ([3f841f8](https://github.com/Willow7737/omnia-protocol/commit/3f841f87598aa320ae0998ceb5fbf1f2de2a3362))
* **audit:** resolve 9 findings from second architecture audit (F-1 to F-27) ([5e222f3](https://github.com/Willow7737/omnia-protocol/commit/5e222f3b3f70c853657f986d271a7c43d72d1197))
* **ci:** add production feature to shards crate for F-13 cfg gate ([10a3c10](https://github.com/Willow7737/omnia-protocol/commit/10a3c10622087c1026b8389b741e8991b748bd4d))
* **ci:** bump anyhow to 1.0.103 + clean up deny.toml ([8fc27e2](https://github.com/Willow7737/omnia-protocol/commit/8fc27e2262b62ef2a4b8f31f666cd82ec16c967b))
* **ci:** fix f64 test calls + fmt for F-20 burn_percentage_bps migration ([5fb2196](https://github.com/Willow7737/omnia-protocol/commit/5fb2196ad0c937681bf6ec3bad0c8ea99df411ba))
* **ci:** revert anyhow to 1.0.102 + ignore RUSTSEC-2026-0190 ([0ded742](https://github.com/Willow7737/omnia-protocol/commit/0ded7429772a66b686553900c3fecf0792c7f6f6))
* **ci:** set cargo-deny multiple-versions to allow ([f645d7c](https://github.com/Willow7737/omnia-protocol/commit/f645d7ca80a2f047811d94d37119c7ef602643ee))

## [0.1.78](https://github.com/Willow7737/omnia-protocol/compare/v0.1.77...v0.1.78) (2026-06-29)


### Bug Fixes

* **audit:** resolve 10 findings from architecture design flaw audit ([14e8753](https://github.com/Willow7737/omnia-protocol/commit/14e875389ec53f598b02ccf20ddc779ca404a391))
* **audit:** resolve 10 findings from architecture design flaw audit ([fcd7107](https://github.com/Willow7737/omnia-protocol/commit/fcd7107df90da63423e94b1e1044bee194173c81))
* **ci:** re-add cargo-vet continue-on-error with honest tracking doc ([dbc73bb](https://github.com/Willow7737/omnia-protocol/commit/dbc73bb1c19b36842eab845c3beada1622180cb2))
* **ci:** remove omnia-binding from omnia-node deps in Cargo.lock ([181f97d](https://github.com/Willow7737/omnia-protocol/commit/181f97d7152bd1adfd872049cbb74f31223fbf9e))
* **ci:** update Cargo.lock for 0.1.76 + fmt the create_shard_router call ([3c3cf08](https://github.com/Willow7737/omnia-protocol/commit/3c3cf0868ffc0d1c8ea0336e7b2414c372df5ea2))

## [0.1.77](https://github.com/Willow7737/omnia-protocol/compare/v0.1.76...v0.1.77) (2026-06-28)


### Features

* **api:** add list endpoints for events, proposals, transfers, validators ([cf7fe12](https://github.com/Willow7737/omnia-protocol/commit/cf7fe125b90701d2063bc2fe6f982cbdb13be666))
* **api:** add list endpoints for events, proposals, transfers, validators ([72d40f3](https://github.com/Willow7737/omnia-protocol/commit/72d40f36cd31da5288c35a253ddd55d4abd7d409))


### Bug Fixes

* **api:** use .values().rev() on IndexMap to get &StoredEvent, not (&K, &V) ([cbb26d2](https://github.com/Willow7737/omnia-protocol/commit/cbb26d2e47f71911c38c71392aece2a6f957af72))

## [0.1.76](https://github.com/Willow7737/omnia-protocol/compare/v0.1.75...v0.1.76) (2026-06-25)


### Features

* **bench:** ZK discovery fix, slashing IAI, network-sim, scaling analysis ([38513ae](https://github.com/Willow7737/omnia-protocol/commit/38513aee136c6636026ad8c55c31b522a39dbdc9))
* **bench:** ZK discovery fix, slashing IAI, network-sim, scaling analysis ([fb8231a](https://github.com/Willow7737/omnia-protocol/commit/fb8231aa83067e939f659a1048bc7a99d731ca96))


### Bug Fixes

* **bench:** resolve 3 CI failures from first real run ([784797f](https://github.com/Willow7737/omnia-protocol/commit/784797f0c221e7c4121c86c8d04df88e0b9be07c))
* **bench:** resolve 3 CI failures from first real run ([a751b27](https://github.com/Willow7737/omnia-protocol/commit/a751b2779b888799b7f5286737d49e229550e2ba))
* **bench:** update baselines for P0-1 signature verification cost ([9877df7](https://github.com/Willow7737/omnia-protocol/commit/9877df7cfb17cec77e8327c3e2337bb66e962aa6))
* **ci:** rustdoc HTML tag + sign test events for P0-1 signature verification ([ee0e76f](https://github.com/Willow7737/omnia-protocol/commit/ee0e76f17854c4bc8bbf21d50f39b1d76d505a4e))
* **iai:** capture stderr — iai-callgrind 0.13 writes to stderr, not stdout ([5492596](https://github.com/Willow7737/omnia-protocol/commit/5492596c569b9b7985c83c5129ba998b51b989d4))
* **iai:** strip ANSI escape codes — root cause of 9/9 missing ([c3eb212](https://github.com/Willow7737/omnia-protocol/commit/c3eb2127df55b96228d04cd464145a251edd031b))
* **iai:** strip ANSI escape codes — root cause of 9/9 missing ([d632285](https://github.com/Willow7737/omnia-protocol/commit/d632285c623a2c6f2dc8b7972a2669798f6c0c24))
* **security:** close 11 P0 critical findings from new audit ([d5928e8](https://github.com/Willow7737/omnia-protocol/commit/d5928e809327274bb7e663cbc7f4f9de3ecd9cb1))
* **security:** close 11 P0 critical findings from new audit ([cee569b](https://github.com/Willow7737/omnia-protocol/commit/cee569be892db4406bddc739f5a5e5a9395808f2))
* **tests:** sign events in chaos-tests load_test for P0-1 signature verification ([028eef5](https://github.com/Willow7737/omnia-protocol/commit/028eef57fe469340ca7d24edaaf9e73dcec0b7f6))


### Documentation

* **benchmarks:** rewrite benchmark-gates.md for 3-layer gate + update BASELINE.md ([b51ddef](https://github.com/Willow7737/omnia-protocol/commit/b51ddefa4e11f22e21ce0534f58671492e1ae23e))
* comprehensive update of 25 architecture, operations, and reference docs ([cf96377](https://github.com/Willow7737/omnia-protocol/commit/cf963779d0ecd17bcf6eb41ba7ca07f54698cde9))
* final consistency sweep — fix all remaining stale references across 91 files ([e0c53b4](https://github.com/Willow7737/omnia-protocol/commit/e0c53b4c0ea73af8255e5cefc39413c7334d63a6))
* final consistency sweep — fix all remaining stale references across 91 files ([3bc4be1](https://github.com/Willow7737/omnia-protocol/commit/3bc4be1d5c812d5eb4a03f53fd0b5952f9a61f5d))
* **security:** document 16 critical v0.1.69 fixes in SECURITY.md + AUDIT_FIX_NOTES.md ([65de793](https://github.com/Willow7737/omnia-protocol/commit/65de793d3e385fe7dafd809667e78d4d419beb29))
* update 20 phase summaries, security docs, reference docs, formal verification ([f0a6605](https://github.com/Willow7737/omnia-protocol/commit/f0a66055f9c43aa5b8f62e0b63187db967d24c52))
* update 20 phase summaries, security docs, reference docs, formal verification ([5567ac7](https://github.com/Willow7737/omnia-protocol/commit/5567ac7b2895d132c97f88a1d1f2bfd7573c5cb7))

## [0.1.75](https://github.com/Willow7737/omnia-protocol/compare/v0.1.74...v0.1.75) (2026-06-23)


### Features

* **bench:** 3-layer benchmark gate — IAI + multi-sample + self-hosted runner ([a4de84a](https://github.com/Willow7737/omnia-protocol/commit/a4de84ae82f2f681c88d635e8f2a8c0bdf8c2d22))
* **bench:** 3-layer benchmark gate — IAI + multi-sample + self-hosted runner ([519c370](https://github.com/Willow7737/omnia-protocol/commit/519c370272bb389560852c534eeecde6f005674e))


### Bug Fixes

* **ci+mentor:** address benchmark gate critique and compilation error ([0b66636](https://github.com/Willow7737/omnia-protocol/commit/0b666365e83984c835146cb8b6438fae7e7fde3d))

## [0.1.74](https://github.com/Willow7737/omnia-protocol/compare/v0.1.73...v0.1.74) (2026-06-22)


### Bug Fixes

* **ci:** resolve cargo audit, cargo fmt, and compilation errors ([3823e09](https://github.com/Willow7737/omnia-protocol/commit/3823e091db14f42bec36092334cba44c6743cadf))
* **clippy:** resolve bind_instead_of_map and needless_borrows warnings ([6f15cf9](https://github.com/Willow7737/omnia-protocol/commit/6f15cf9e81cd12dee5c7fe1e18b67b25ea1af49e))
* **rustdoc:** resolve broken intra-doc links and HTML tags ([0c69fee](https://github.com/Willow7737/omnia-protocol/commit/0c69feed7b78865eb7af777223fca91cd2604504))
* **security+node:** close 16 critical audit findings ([5d3d776](https://github.com/Willow7737/omnia-protocol/commit/5d3d7760edb99d9ab3cb4a056051969dfbf413fc))

## [0.1.73](https://github.com/Willow7737/omnia-protocol/compare/v0.1.72...v0.1.73) (2026-06-22)


### Bug Fixes

* **bench:** address 4 mentor review issues — governor verify, anomaly, ZK root-cause, gate output ([9ec7c31](https://github.com/Willow7737/omnia-protocol/commit/9ec7c3125c0f15a8d98b6c02c5d52391dfeb0d45))
* **bench:** address 4 mentor review issues — governor verify, anomaly, ZK root-cause, gate output ([df53ec8](https://github.com/Willow7737/omnia-protocol/commit/df53ec878ddb60a735121409adb54452e297c768))
* **bench:** fix baseline panic + false regression from partial-match ([63dfecb](https://github.com/Willow7737/omnia-protocol/commit/63dfecbd6e1e9769be9201831fa44fa5ab6051d3))
* **chaos:** handle graph insert errors gracefully under message loss ([8ebaa71](https://github.com/Willow7737/omnia-protocol/commit/8ebaa7176754f366c7a623f49878e6061c7a8b82))

## [0.1.72](https://github.com/Willow7737/omnia-protocol/compare/v0.1.71...v0.1.72) (2026-06-21)


### Bug Fixes

* **bench:** address 6 mentor review issues — hang, ungating, ZK drift ([7815de8](https://github.com/Willow7737/omnia-protocol/commit/7815de8116072824ff943eef07d9d1cf739eaba1))
* **bench:** address 6 mentor review issues — hang, ungating, ZK drift ([5e79051](https://github.com/Willow7737/omnia-protocol/commit/5e79051f55239a0647e315a99393cd1194b2e134))
* **bench:** address 6 mentor review issues — script bug, CI split, baselines ([b19b8ac](https://github.com/Willow7737/omnia-protocol/commit/b19b8ac1b6eda09e26d91c62a2ba0a648b8046d0))
* **bench:** address 6 mentor review issues — script bug, CI split, baselines ([76c83ef](https://github.com/Willow7737/omnia-protocol/commit/76c83efdc76dd94ab32ac75d60c746b6ab8aa297))

## [0.1.71](https://github.com/Willow7737/omnia-protocol/compare/v0.1.70...v0.1.71) (2026-06-21)


### Bug Fixes

* accept 'expected layout' error in ZK proof rejection tests ([550bf62](https://github.com/Willow7737/omnia-protocol/commit/550bf62f230a6a26b9ebbf95aca9b265b0620fe7))
* add ENV_LOCK to node/tests/integration.rs for env var race ([4551b97](https://github.com/Willow7737/omnia-protocol/commit/4551b979507c27a65970fbf9abfa359432497fcb))
* add hex dependency to omnia-economics for C-7/C-8 caller binding ([d5da3f4](https://github.com/Willow7737/omnia-protocol/commit/d5da3f45d7e080c2e3f3799853ce362ea71fb0b5))
* apply audit report security fixes (C-6, C-7, C-8, C-12, C-15, H-4, H-13) ([a0a2b02](https://github.com/Willow7737/omnia-protocol/commit/a0a2b0224e13edf47d2cfd91c414d800fd1d9c3c))
* apply final audit fixes (C-1, C-2, C-3/C-4, C-9) ([0725fc7](https://github.com/Willow7737/omnia-protocol/commit/0725fc78982b915e0dc9852c2e6d290d82f1fb94))
* apply remaining audit fixes (C-10, C-11, C-14, H-6, H-7, H-8, H-10) ([1bf6676](https://github.com/Willow7737/omnia-protocol/commit/1bf66765789a3b98a7b63c4dfd1e10ceca93aa17))
* **bench+coverage:** address mentor review — 5 critical issues ([ad76223](https://github.com/Willow7737/omnia-protocol/commit/ad76223470e189f5c3ab0bc1672e26252bcc3f83))
* **bench+coverage:** address mentor review — 5 critical issues ([#219](https://github.com/Willow7737/omnia-protocol/issues/219)) ([86e2070](https://github.com/Willow7737/omnia-protocol/commit/86e20704364ab2c70175f3e7c3654eab143c2b3b))
* **ci:** increase fuzz smoke test timeout from 10s to 120s ([24a58b4](https://github.com/Willow7737/omnia-protocol/commit/24a58b48e41a64612ff223161a061088886ac110))
* **ci:** increase fuzz smoke timeout from 120s to 180s for libp2p ([3cb4f8d](https://github.com/Willow7737/omnia-protocol/commit/3cb4f8dc435404518b6cda0cc95756484454501c))
* **ci:** increase fuzz smoke timeout from 180s to 300s for gossip ([e497bfe](https://github.com/Willow7737/omnia-protocol/commit/e497bfef952539b5adb5fd25d2d38a14752e1ea0))
* **ci:** update cargo audit in nightly-fuzz.yml and release.yml ([4de45ec](https://github.com/Willow7737/omnia-protocol/commit/4de45ec95229b1dac2d84860b46715ae761d1618))
* **ci:** update cargo audit in nightly-fuzz.yml and release.yml ([7b16712](https://github.com/Willow7737/omnia-protocol/commit/7b16712ead6305e42ad05a5db8ee7563e7006108))
* **ci:** use --bench flags to avoid --save-baseline on lib tests ([f15ed77](https://github.com/Willow7737/omnia-protocol/commit/f15ed777cfabe5f162a72c42a39106607d124b00))
* **ci:** use cargo-msrv 0.19.1 compatible syntax ([5ae7f25](https://github.com/Willow7737/omnia-protocol/commit/5ae7f256519f52ba60ff6077d92e5fa38d8f620e))
* docker compose e2e path + skip in all-features CI ([2c832cf](https://github.com/Willow7737/omnia-protocol/commit/2c832cf3c8fc71ee571ba31a54b4421f1ebdc44f))
* gate celestia mock tests with #[cfg(not(feature = "celestia"))] ([78f35d8](https://github.com/Willow7737/omnia-protocol/commit/78f35d8cd7fbc92ce3adea73dfc745717d93b52d))
* gate test_economics_state_full_lifecycle with #[cfg(not(feature = "production"))] ([db5a4c2](https://github.com/Willow7737/omnia-protocol/commit/db5a4c2242e55cbf7dfccadf1f2389c9254940ad))
* resolve CI failures — fmt + fast_sync/circuit compile errors ([5f54171](https://github.com/Willow7737/omnia-protocol/commit/5f5417177f207829452876e1b8867515856d48cb))
* resolve dead_code + clippy warnings under -D warnings ([1bacc7d](https://github.com/Willow7737/omnia-protocol/commit/1bacc7d52cde719b18d7d2b3ed173f6fe3c32e8a))
* rustfmt integration.rs + pin cargo-msrv to 0.19.1 ([72cff8a](https://github.com/Willow7737/omnia-protocol/commit/72cff8a0016cef764a6a2777268affab98a157f4))


### Documentation

* comprehensive markdown audit — 36 files updated, 2 removed ([60c0a1a](https://github.com/Willow7737/omnia-protocol/commit/60c0a1ae41632dea72d06f53085b63bac4af1583))
* comprehensive markdown audit — 36 files updated, 2 removed ([e28696c](https://github.com/Willow7737/omnia-protocol/commit/e28696c1ccc267a9bb75fd664683f0ae13388eda))


### Tests

* mark deprecated-dkg test_dkg_phase_transitions as #[ignore] ([0feb0c3](https://github.com/Willow7737/omnia-protocol/commit/0feb0c3396219bab5026445425f6cad986958029))


### CI

* bump actions/checkout from v4 to v5 (Node.js 20 deprecation) ([6d37603](https://github.com/Willow7737/omnia-protocol/commit/6d37603cb6553af509079ab26977dff4aadd028f))
* bump actions/upload-artifact and download-artifact from v4 to v5 ([3033b84](https://github.com/Willow7737/omnia-protocol/commit/3033b841639d2595ea882d4a1f1a3bdccd8dfa1f))

## [0.1.70](https://github.com/Willow7737/omnia-protocol/compare/v0.1.69...v0.1.70) (2026-06-19)


### Bug Fixes

* 2 test failures from authorization checks + unused mut warning ([86d7427](https://github.com/Willow7737/omnia-protocol/commit/86d74277737e6545021222735b24c41900a5660b))
* 4 test failures — error serialization + DID registration ([c9f02a5](https://github.com/Willow7737/omnia-protocol/commit/c9f02a5f0ce7bd401b2fc510cd0d1ae0f452d0ac))
* apply rustfmt formatting to test modules ([6a6e733](https://github.com/Willow7737/omnia-protocol/commit/6a6e733ca576e2b1b6bdd66ec9a04998cf9bfd0b))
* apply rustfmt to start_test_server_with_economics signature + calls ([3d7b27c](https://github.com/Willow7737/omnia-protocol/commit/3d7b27c5f689328fcec61daa669b63ba052d1f39))
* **ci:** pin iai-callgrind-runner to 0.13.4 to match library version ([d2bf8ec](https://github.com/Willow7737/omnia-protocol/commit/d2bf8ec1278ed10855bc4b807e5fe40edf1037d8))
* **ci:** pin iai-callgrind-runner to 0.13.4 to match library version ([3eab61b](https://github.com/Willow7737/omnia-protocol/commit/3eab61b760158cc9fbe49a5815a970f844aed131))
* env var race causing test_mint_ubc_admin_ok 403 failure ([e154a18](https://github.com/Willow7737/omnia-protocol/commit/e154a1804d865460e7cfb06c16695c1d0613574e))
* missing type imports in validator test modules + unused HashMap ([fa5043a](https://github.com/Willow7737/omnia-protocol/commit/fa5043abb7ac3fb70d84e7c97b5b0a5afda06261))
* pre-register DIDs directly on AppState.economics for handler tests ([0c2d071](https://github.com/Willow7737/omnia-protocol/commit/0c2d071e8b77168a9070137ca70db4e9a6586ac4))
* rustfmt — collapse ENV_LOCK to single line, wrap format! call ([14990d4](https://github.com/Willow7737/omnia-protocol/commit/14990d455472ec13d774f141244dcd265143c07e))
* rustfmt — wrap long set_var string literal ([689a513](https://github.com/Willow7737/omnia-protocol/commit/689a513041f0ac0c8e5a74184ebf981e308ded6b))
* rustfmt + compile errors in new test modules ([ec5247c](https://github.com/Willow7737/omnia-protocol/commit/ec5247c2c7e34348e2a3921dbae1b5fe1cd0ecc9))
* serialize env-var-dependent tests with std::sync::Mutex ([45d9869](https://github.com/Willow7737/omnia-protocol/commit/45d9869a3064523a212268c72a2216a895c7ef7b))
* trim vote choice whitespace + add handler branch coverage ([8799f38](https://github.com/Willow7737/omnia-protocol/commit/8799f38df2f99a08fcdbf91c872a310a5cfc820b))
* use 64-char invalid-hex string for InvalidHex test ([d3883c2](https://github.com/Willow7737/omnia-protocol/commit/d3883c270f4fbd1491431c96cf48abc79d2da379))
* use std::sync::Mutex for ENV_LOCK + rustfmt import order ([b4725d5](https://github.com/Willow7737/omnia-protocol/commit/b4725d541c67cff54387bc8d7372f35a67921bbd))
* wrap long assert! in crdt/mod.rs test ([3142426](https://github.com/Willow7737/omnia-protocol/commit/3142426f3bd388ae5d815e53c016057e98e391d7))


### Documentation

* honest C-4 status + coverage measurement caveat ([4b66ffa](https://github.com/Willow7737/omnia-protocol/commit/4b66ffa61f8de1750f604a20a6f32eac8c630646))


### Tests

* add coverage for 0% modules — stub adapters + shard validators ([94fed9c](https://github.com/Willow7737/omnia-protocol/commit/94fed9c40433f70b205ad6ca5f1f46a9b79a2331))
* add coverage for 0% modules — stub adapters + shard validators ([9aa04fa](https://github.com/Willow7737/omnia-protocol/commit/9aa04fa7b9899d5f21e69263dc94047f52e93c1f))
* add real coverage for substrate/lib.rs, crdt/mod.rs, domain_state.rs ([0f8d6ad](https://github.com/Willow7737/omnia-protocol/commit/0f8d6adcc1334c7992e5141a3ae7b2b8554e8577))
* add real coverage for substrate/lib.rs, crdt/mod.rs, domain_state.rs ([d2b8fdf](https://github.com/Willow7737/omnia-protocol/commit/d2b8fdf2e0d8cf3df32d5135d4aacf785003f2fd))
* remove stub adapter tests, add real coverage for identity + API ([ec10de0](https://github.com/Willow7737/omnia-protocol/commit/ec10de01b612f6c134a8e6af5b24e5589ea30a2b))

## [0.1.69](https://github.com/Willow7737/omnia-protocol/compare/v0.1.68...v0.1.69) (2026-06-19)


### Features

* add local testnet startup script ([d5d0d4e](https://github.com/Willow7737/omnia-protocol/commit/d5d0d4e3d1d0e1acce267490b3dc9fb4e8d6f87d))
* **docker:** add server-side env vars for web dashboard proxy ([dcd420b](https://github.com/Willow7737/omnia-protocol/commit/dcd420bef9f2c6e7377f5f681e10ec184ab44510))
* **docker:** add server-side env vars for web dashboard proxy ([6898423](https://github.com/Willow7737/omnia-protocol/commit/6898423174960f4fc1435dea6087800a69ee55b6))
* **docker:** web depends on all nodes for reliable live data ([04fdce4](https://github.com/Willow7737/omnia-protocol/commit/04fdce42be72855af9f0307bef7c65d5d6b32789))


### Bug Fixes

* **audit-v0.1.68:** apply Phase 1-5 fixes from OMNIA_FIX_STRATEGY ([51ee76f](https://github.com/Willow7737/omnia-protocol/commit/51ee76fa43ec99ff4ec036e260380830ff46ab2c))
* **bench:** exclude consensus_throughput from regression gate (variance &gt; threshold) ([f2a2046](https://github.com/Willow7737/omnia-protocol/commit/f2a204606c0714e3129e8c52adc55700e1cdac08))
* **bench:** exclude consensus_throughput from regression gate (variance &gt; threshold) ([8e55417](https://github.com/Willow7737/omnia-protocol/commit/8e5541730927bebdc6caf3fd26bb1acab9333bb6))
* capture payload.operation debug before move in route_event ([2ea298b](https://github.com/Willow7737/omnia-protocol/commit/2ea298b4243b339fefc289039aee3503263c0de3))
* resolve CI --locked failure, align versions, correct documentation ([0518b37](https://github.com/Willow7737/omnia-protocol/commit/0518b374719e0754ac08aa8eace9eda65804e66e))
* resolve CI failures — blake3 borrow, fmt, Cargo.lock deps ([193b57c](https://github.com/Willow7737/omnia-protocol/commit/193b57c8ebe70d5c87243e2c623f1a116eaa8e4e))
* resolve remaining CI failures — shadow var, fmt, audit, lock ([0c3920d](https://github.com/Willow7737/omnia-protocol/commit/0c3920d1d05200e128d3449709b90f89d225779a))
* resolve Result type alias conflict and invalid cargo audit flag ([4f28c17](https://github.com/Willow7737/omnia-protocol/commit/4f28c17ed9a42440e3ce1f4a0b9bd4e541e12150))
* rustdoc broken intra-doc link to remove_peer ([6f12652](https://github.com/Willow7737/omnia-protocol/commit/6f126525c5345b98496039078f4e8fc61a54fd3d))
* test compilation errors from H-5 IndexMap migration and C-6 test ([70421e9](https://github.com/Willow7737/omnia-protocol/commit/70421e9acb50b582d0c5b343ee266d03fd7b8d93))


### Performance

* **bench:** reduce ZK expanded_circuit sample size to avoid CI timeout ([a0672c0](https://github.com/Willow7737/omnia-protocol/commit/a0672c0b424cb48889340a26e62aff318a4aacf9))
* **bench:** reduce ZK expanded_circuit sample size to avoid CI timeout ([5afd408](https://github.com/Willow7737/omnia-protocol/commit/5afd408b33d0e6884618b092b2d8115bba7a588a))

## [0.1.68](https://github.com/Willow7737/omnia-protocol/compare/v0.1.67...v0.1.68) (2026-06-02)

### Features

- make node info/peers API public + add web dashboard to Docker Compose ([01c2ffe](https://github.com/Willow7737/omnia-protocol/commit/01c2ffe52fa6d0190398a65ffe5042bf69ad6671))

### Bug Fixes

- **fmt:** collapse method chains to single lines in test ([383c0ac](https://github.com/Willow7737/omnia-protocol/commit/383c0ac71f5280fb78abdc1eef2287b187331fcd))
- **fmt:** collapse Router::new() chain to single line ([1b67d23](https://github.com/Willow7737/omnia-protocol/commit/1b67d2351eac74ed6ef14b13cdc68b7ba2646b93))
- P2P network QUIC transport — Docker multiaddr mismatch + listen address defaults ([acf97b5](https://github.com/Willow7737/omnia-protocol/commit/acf97b5206d4b4dfc54da8e2843bfdfa617bed0d))
- P2P network QUIC transport — Docker multiaddr mismatch + listen address defaults ([4ccc159](https://github.com/Willow7737/omnia-protocol/commit/4ccc1596449f83e59ae0a61c36b2ec97059e0561))
- **test:** use authenticated endpoint in 401 error format test ([4953cea](https://github.com/Willow7737/omnia-protocol/commit/4953cea39a93601bacdc8f649abebe1ed3bfed45))
- update auth integration tests for public node info/peers endpoints ([a960240](https://github.com/Willow7737/omnia-protocol/commit/a960240e03b4bfa871b0ab19b29c8a64363f093b))
- update auth integration tests for public node info/peers endpoints ([7d064e8](https://github.com/Willow7737/omnia-protocol/commit/7d064e87ca26175cbc574057da332a2a53819c21))

## [0.1.67](https://github.com/Willow7737/omnia-protocol/compare/v0.1.66...v0.1.67) (2026-06-02)

### Bug Fixes

- P2P network QUIC transport — Docker multiaddr mismatch + listen address defaults ([acf97b5](https://github.com/Willow7737/omnia-protocol/commit/acf97b5206d4b4dfc54da8e2843bfdfa617bed0d))
- P2P network QUIC transport — Docker multiaddr mismatch + listen address defaults ([4ccc159](https://github.com/Willow7737/omnia-protocol/commit/4ccc1596449f83e59ae0a61c36b2ec97059e0561))

## [0.1.66](https://github.com/Willow7737/omnia-protocol/compare/v0.1.65...v0.1.66) (2026-06-02)

### Bug Fixes

- chaos test message loss flaky assert + Docker missing ENTRYPOINT ([a60f9c0](https://github.com/Willow7737/omnia-protocol/commit/a60f9c048411d5286b8572a97b990773b34a79e2))

### Documentation

- benchmark accuracy audit — fix non-existent commands, add accuracy qualifiers ([79186fa](https://github.com/Willow7737/omnia-protocol/commit/79186fa83738f4adf405c14bf0712ac08d03e04e))
- benchmark accuracy audit — fix non-existent commands, add accuracy qualifiers ([31d330b](https://github.com/Willow7737/omnia-protocol/commit/31d330b0aaed791c68ed3294d7c63c0c77586c55))
- overhaul all markdowns — true stats, fix Phase/Sprint confusion, remove agent-ctx ([71f7ab7](https://github.com/Willow7737/omnia-protocol/commit/71f7ab7a2694e0f045cce0957874c00919d77955))
- overhaul all markdowns — true stats, fix Phase/Sprint confusion, remove agent-ctx ([dc6ee7b](https://github.com/Willow7737/omnia-protocol/commit/dc6ee7be100d7acf242e2a7d27cab5eb9b065f26))

## [0.1.65](https://github.com/Willow7737/omnia-protocol/compare/v0.1.64...v0.1.65) (2026-06-01)

### Bug Fixes

- apply cargo fmt to threshold.rs, provide keypair in test AppState constructors ([f0a5776](https://github.com/Willow7737/omnia-protocol/commit/f0a577624febec84b55a0634072a03174b578e32))
- clippy ok_or_else → ok_or in economics/src/ubc.rs ([1995699](https://github.com/Willow7737/omnia-protocol/commit/19956999ff7132c3a1fffad4c0d9a9a9ea95f944))
- clippy redundant_pattern_matching in chaos-tests ([e9e6399](https://github.com/Willow7737/omnia-protocol/commit/e9e63997aa679d67411eef00bedf523580d8c516))
- comprehensive security and performance audit — 110 issues resolved ([eb6b87d](https://github.com/Willow7737/omnia-protocol/commit/eb6b87dc2a53accb80c1f28843f3becab92e5c9b))
- comprehensive security and performance audit — 110 issues resolved ([5b2ca8e](https://github.com/Willow7737/omnia-protocol/commit/5b2ca8ee463167212f36cd6db5bef0071a26a752))
- resolve all remaining CI failures across workspace ([c8e6067](https://github.com/Willow7737/omnia-protocol/commit/c8e6067fba4625176a648aac2c79f0cd9768ef33))
- resolve CI failures — add ReadableTable imports, fix unicode escapes, apply cargo fmt ([4173d04](https://github.com/Willow7737/omnia-protocol/commit/4173d04b5441f0be9edc50b26f9c196278bcb09c))
- resolve E0277 From&lt;ProvenanceError&gt; impl, clippy ok_or/unwrap errors, remove duplicate stub() ([bb19ab6](https://github.com/Willow7737/omnia-protocol/commit/bb19ab6b4856cc014c59a7c528f8384a80c286c2))
- resolve E0597 lifetime issue, remove unused import, fix remaining fmt issues ([1b52498](https://github.com/Willow7737/omnia-protocol/commit/1b524983a1496c98342e2247d48a9810c16ca00c))

## [0.1.64](https://github.com/Willow7737/omnia-protocol/compare/v0.1.63...v0.1.64) (2026-05-31)

### Bug Fixes

- comprehensive security and performance audit fixes across all crates ([66897c2](https://github.com/Willow7737/omnia-protocol/commit/66897c2dc145d3171a1100423d298b546ad2fb29))
- comprehensive security and performance audit fixes across all crates ([9d4ac7b](https://github.com/Willow7737/omnia-protocol/commit/9d4ac7b3baafdcbbebdf03c674b11b065bfabd4d))
- resolve 7 omnia-crypto test failures (keystore + threshold DKG) ([f1b4ce5](https://github.com/Willow7737/omnia-protocol/commit/f1b4ce5532d046435a24f917c647f7edea169a3b))
- resolve all CI compilation, clippy, and formatting errors ([59cd4bd](https://github.com/Willow7737/omnia-protocol/commit/59cd4bdf2c6130c208dd52cda62921000024b7b6))
- resolve all remaining CI compilation, clippy, and warning errors (round 2) ([da9d912](https://github.com/Willow7737/omnia-protocol/commit/da9d912e248e6a3309ec4a48d2a1a5eae8454401))
- resolve CI test failures and rustdoc errors (round 3) ([6cc6a08](https://github.com/Willow7737/omnia-protocol/commit/6cc6a08a5cb0f86bb85bb00cbead02c6f3f4e045))
- update version assertion in consensus state persistence test ([f94e6cd](https://github.com/Willow7737/omnia-protocol/commit/f94e6cdb81e9e12a3d2a3d88f40407bb407f8c1d))

## [0.1.63](https://github.com/Willow7737/omnia-protocol/compare/v0.1.62...v0.1.63) (2026-05-31)

### Bug Fixes

- E0509 cannot move out of EthereumConfig with Drop trait ([88c4e4a](https://github.com/Willow7737/omnia-protocol/commit/88c4e4a9e2382f994ac1c5154f51bbd8642cef87))
- E0509 cannot move out of EthereumConfig with Drop trait ([5eb702d](https://github.com/Willow7737/omnia-protocol/commit/5eb702db16e1e8b1dfc63d14a003a0e78f90474c))

## [0.1.62](https://github.com/Willow7737/omnia-protocol/compare/v0.1.61...v0.1.62) (2026-05-31)

### Features

- comprehensive security & performance audit fixes - production hardening ([e88aadd](https://github.com/Willow7737/omnia-protocol/commit/e88aadd46544c26f08f6ddfae65ee26ab141e4e7))

### Bug Fixes

- identity_hardening tests and rustdoc broken link ([0c69bec](https://github.com/Willow7737/omnia-protocol/commit/0c69beca28c3b9c468405ac68a76c80e1e0312e5))
- resolve all CI failures — compilation errors, test failures, fmt and warnings ([f3def05](https://github.com/Willow7737/omnia-protocol/commit/f3def057e4337279a3700eb037073ecb0af7fa60))
- resolve remaining CI failures - zk_benchmarks, clippy, crypto tests ([74c1aa0](https://github.com/Willow7737/omnia-protocol/commit/74c1aa037312db96712b23eb8b5ac42f5e7d0218))
- **security:** comprehensive security, performance, and correctness audit fixes ([b05c520](https://github.com/Willow7737/omnia-protocol/commit/b05c52023f168952aa063697d028170d5cf773a6))
- update slashing tests to expect EquivocationDetected error ([0fd09c0](https://github.com/Willow7737/omnia-protocol/commit/0fd09c03d02c97705477034abf1e2a89feb8fed8))

## [0.1.61](https://github.com/Willow7737/omnia-protocol/compare/v0.1.60...v0.1.61) (2026-05-30)

### Bug Fixes

- **benches:** handle Result&lt;Event, EventValidationError&gt; from Event::new/genesis ([d0eaaa2](https://github.com/Willow7737/omnia-protocol/commit/d0eaaa2fa7300be1b489a402ca3ec8ad21fdface))
- cargo fmt in layer2_integration, cross-shard source verification in fee test ([0cef080](https://github.com/Willow7737/omnia-protocol/commit/0cef0806e092efdebd2c09f571991e4ad4c79959))
- clippy bind_instead_of_map in substrate, fix financial shard test assertions ([694f37e](https://github.com/Willow7737/omnia-protocol/commit/694f37e57a2b68f08e8c936fdd6e3045bb3b8ad1))
- clippy needless_range_loop in merkle.rs, integration test JWT auth ([d821c9d](https://github.com/Willow7737/omnia-protocol/commit/d821c9d6929c1a8be5d2a549a7695aaa179c7b71))
- cross-shard test failures due to mint authority and identity auth ([9e1e925](https://github.com/Willow7737/omnia-protocol/commit/9e1e9254358cf28f02fdcfebdc88c1af132d3fdf))
- remaining Event::new()/genesis() Result unwrapping + clippy threshold.rs ([b306738](https://github.com/Willow7737/omnia-protocol/commit/b30673811c9423053ecb52fb736bad3092b5f53d))
- remove needless return in biological/computational ZK proof error paths ([a652ab1](https://github.com/Willow7737/omnia-protocol/commit/a652ab117fd6ac89b081f37e614bc95cc2825544))
- resolve all CI failures — Result unwrapping, fmt, clippy, auth tests ([fb25658](https://github.com/Willow7737/omnia-protocol/commit/fb256580286d2ec542a19482d1f818c832bf7620))
- resolve all remaining CI failures — keystore Result assertions, clippy redundant closures, chaos-tests warnings, deprecated annotations ([1fcd23f](https://github.com/Willow7737/omnia-protocol/commit/1fcd23ff2507788f20b751fb80a017c4c250d12c))
- resolve rustdoc broken intra-doc link for MAX_REWARD_PER_PROOF ([37ecaf2](https://github.com/Willow7737/omnia-protocol/commit/37ecaf279a635daf1f2c28953bc0fd22b34ab49d))
- **security:** comprehensive security, performance, and correctness audit fixes ([98cf43a](https://github.com/Willow7737/omnia-protocol/commit/98cf43a42ee4d8d118a615bb518f53308c327f86))
- **security:** comprehensive security, performance, and correctness audit fixes ([a6d8720](https://github.com/Willow7737/omnia-protocol/commit/a6d8720c1e23e91b0432d44365d69579af6613b4))

## [0.1.60](https://github.com/Willow7737/omnia-protocol/compare/v0.1.59...v0.1.60) (2026-05-27)

### Bug Fixes

- **docker:** create stub source files before cargo generate-lockfile ([cb65ff0](https://github.com/Willow7737/omnia-protocol/commit/cb65ff0f67eaddd0bf09f9b9e751386010024f52))
- **docker:** simplify Dockerfile — delete lockfile instead of generate-lockfile ([aefb24b](https://github.com/Willow7737/omnia-protocol/commit/aefb24bb80a95c0cec3d88161c786d18d3f4801f))
- **docker:** simplify Dockerfile — delete lockfile instead of generate-lockfile ([5342f87](https://github.com/Willow7737/omnia-protocol/commit/5342f87605810dbabe79ed0c0ad024cd2bbfde48))

## [0.1.59](https://github.com/Willow7737/omnia-protocol/compare/v0.1.58...v0.1.59) (2026-05-27)

### Bug Fixes

- **docker:** resolve build failure from CI-only crates excluded by .dockerignore ([b25226c](https://github.com/Willow7737/omnia-protocol/commit/b25226c925eb00e633452f7097cae708cfbb3339))

## [0.1.58](https://github.com/Willow7737/omnia-protocol/compare/v0.1.57...v0.1.58) (2026-05-27)

### Features

- add GitHub Codespace config for 5-node testnet ([7a601ab](https://github.com/Willow7737/omnia-protocol/commit/7a601ab2382df14af4cb311b331f59936058d146))

## [0.1.57](https://github.com/Willow7737/omnia-protocol/compare/v0.1.56...v0.1.57) (2026-05-27)

### Features

- add limit verification test suite (39 tests), LIMITS.md reference ([7e1cdab](https://github.com/Willow7737/omnia-protocol/commit/7e1cdab5cc541ad13b53288aec53537710a9dd0a))
- add limit verification test suite (39 tests), LIMITS.md reference ([01ba013](https://github.com/Willow7737/omnia-protocol/commit/01ba013213e8d67cf14e88964378b282538eea81))
- add testnet launch script with monitoring support ([84b2a45](https://github.com/Willow7737/omnia-protocol/commit/84b2a45ac572c2c3e826e7073315db6463060bdb))
- add testnet launch script with monitoring support ([10daa78](https://github.com/Willow7737/omnia-protocol/commit/10daa7865e217060fa973c5fc63af7e4b186b33a))

### Bug Fixes

- **ci:** handle forge create non-zero exit in ethereum-settlement.yml ([fb40575](https://github.com/Willow7737/omnia-protocol/commit/fb405753f943945ec940ced0045172f1674de641))
- **ci:** handle forge create non-zero exit in ethereum-settlement.yml ([1eafa5a](https://github.com/Willow7737/omnia-protocol/commit/1eafa5aad493a1fb7634fe3044401f89a1d23dce))
- **ci:** resolve 25 CI workflow issues — action versions, Dockerfile, timeouts, shell safety ([87a6529](https://github.com/Willow7737/omnia-protocol/commit/87a65296d53589e90962c24f82c98ca85ed948d5))
- **ci:** resolve Docker lowercase repo name + shard mutation testing to prevent timeout ([5c6d8b8](https://github.com/Willow7737/omnia-protocol/commit/5c6d8b81b33cef61f10a1dac93a0b0bde0b32047))
- **ci:** resolve Docker lowercase repo name + shard mutation testing to prevent timeout ([a002c20](https://github.com/Willow7737/omnia-protocol/commit/a002c2083403add99dd29eadd115b4d7c05e47a1))
- **clippy:** replace unwrap() with expect() in keystore_bridge.rs rotate() ([4551edb](https://github.com/Willow7737/omnia-protocol/commit/4551edbdd1366c060bbcb088f9aebaec906b0544))
- **consensus:** allow equivocation at current sequence in monotonicity check ([0679e8f](https://github.com/Willow7737/omnia-protocol/commit/0679e8f484d03fa5d6cc0c641e096df630106252))
- **consensus:** resolve rustdoc private-intra-doc-links errors ([131eb3d](https://github.com/Willow7737/omnia-protocol/commit/131eb3d233a626acdaedf0a422e6ef35ecfd554d))
- **fmt:** apply cargo fmt to keystore_bridge.rs and g_counter.rs ([55fd867](https://github.com/Willow7737/omnia-protocol/commit/55fd867efd67c7784882e1b7aef0ce36aa1db4d3))
- regenerate Cargo.lock and apply cargo fmt to tests/src/lib.rs ([e97c4df](https://github.com/Willow7737/omnia-protocol/commit/e97c4df979ae08b7e06388a0680a70bce54bce24))
- resolve 7 high-priority audit bugs + CI/Docker infrastructure fixes ([b5d0e55](https://github.com/Willow7737/omnia-protocol/commit/b5d0e55caf3d1f602e06d26f32fc56b1080b7055))
- resolve 9 medium-priority audit findings + verify 4 as by-design ([c8a41ad](https://github.com/Willow7737/omnia-protocol/commit/c8a41adb5504f971658820f059c658fdf2ff1f1d))
- resolve 9 medium-priority audit findings + verify 4 as by-design ([bd1105e](https://github.com/Willow7737/omnia-protocol/commit/bd1105e6bc120ad39e26f9f7dbd3d656685c000d))
- resolve all CI failures for testnet readiness ([24c4c3b](https://github.com/Willow7737/omnia-protocol/commit/24c4c3b93c251c41b9f65306b80d194521ac9728))
- **rustdoc:** remove private intra-doc link in current_signing_key() ([f0868d6](https://github.com/Willow7737/omnia-protocol/commit/f0868d609e4e6540b0e879e8ddb851eebadecb0a))
- **security:** enforce sequence monotonicity at CausalGraph::insert() boundary ([25198a0](https://github.com/Willow7737/omnia-protocol/commit/25198a07f349f44fa269f1028eff8919929536d7))
- update test_duplicate_signers_detected for dedup (AUDIT-12), fix governance.rs doc link ([4da24da](https://github.com/Willow7737/omnia-protocol/commit/4da24da0a8c80aa563ff51ab2065ca8804e6a13c))

### Performance

- eliminate O(n) bottlenecks in CausalGraph::insert — 37× throughput gain ([7bb452a](https://github.com/Willow7737/omnia-protocol/commit/7bb452a768668569166b4c6548cd94cdaa2a989c))

### Documentation

- add verified protocol limits, update audit status to 8/23 remediated ([9429e9a](https://github.com/Willow7737/omnia-protocol/commit/9429e9a436f79202e946be7430a13b3e810b383b))
- add verified protocol limits, update audit status to 8/23 remediated ([3330f25](https://github.com/Willow7737/omnia-protocol/commit/3330f2579b62a10d8fd626fbb53b3cb6f2eda568))
- update markdowns for v0.1.56 — full codebase audit, test counts, roadmap findings ([1e69ee7](https://github.com/Willow7737/omnia-protocol/commit/1e69ee7bb2d39850165b13f4429360d3d54a9fde))
- update markdowns for v0.1.56 — full codebase audit, test counts, roadmap findings ([dc01129](https://github.com/Willow7737/omnia-protocol/commit/dc01129778177e75d7691a64691fcf8a884f84a7))
- update project status to 93% — 7 high-priority audit findings remediated ([dbf46e8](https://github.com/Willow7737/omnia-protocol/commit/dbf46e837f95831f48d57f76ddb7d5867db301fe))

## [0.1.56](https://github.com/Willow7737/omnia-protocol/compare/v0.1.55...v0.1.56) (2026-05-25)

### Bug Fixes

- **ci:** resolve 3 CI failures — benchmark baselines, iai-callgrind runner, OpenSSL cross-compile ([597824d](https://github.com/Willow7737/omnia-protocol/commit/597824d87ead2bcce08d3b04029690a91f012998))
- **ci:** resolve 3 CI failures — benchmark baselines, iai-callgrind runner, OpenSSL cross-compile ([a38b637](https://github.com/Willow7737/omnia-protocol/commit/a38b637f208c39da98f5663982fc1606ccc78b58))

## [0.1.55](https://github.com/Willow7737/omnia-protocol/compare/v0.1.54...v0.1.55) (2026-05-24)

### Bug Fixes

- **ci:** resolve 4 CI failures — benchmarks, OpenSSL, Solidity, binary size ([4c241a2](https://github.com/Willow7737/omnia-protocol/commit/4c241a273c0f24b07b644ad0b9983b07e4ee9c9d))
- **ci:** revert reqwest rustls migration + add OpenSSL ARM64 cross-compile from source ([153703d](https://github.com/Willow7737/omnia-protocol/commit/153703d765b27d6dad46e13e60ebb7a1310a1bfc))
- **ci:** revert reqwest rustls migration + add OpenSSL ARM64 cross-compile from source ([b90cf48](https://github.com/Willow7737/omnia-protocol/commit/b90cf488e71560ed85e6d513307909058b832383))

## [0.1.54](https://github.com/Willow7737/omnia-protocol/compare/v0.1.53...v0.1.54) (2026-05-24)

### Bug Fixes

- **ci:** resolve nightly mutation testing timeout and fuzz linker errors ([0901de9](https://github.com/Willow7737/omnia-protocol/commit/0901de905b89d8b3d4020f54f5b47586285509a2))
- **ci:** resolve nightly mutation testing timeout and fuzz linker errors ([717961e](https://github.com/Willow7737/omnia-protocol/commit/717961e7c5802d662597367a56764bc660f1447a))
- **e2e:** late-join test cross-ref events + update docs for v0.1.53 review ([44f5287](https://github.com/Willow7737/omnia-protocol/commit/44f5287ef539adab68d11009f5b40f83747a1668))
- **e2e:** late-join test cross-ref events + update docs for v0.1.53 review ([1b15de6](https://github.com/Willow7737/omnia-protocol/commit/1b15de63290b2f450e275e3ce95189ad001ec57c))
- **fmt:** collapse eprintln! to single line in e2e_multi_node_consensus.rs ([c4a7566](https://github.com/Willow7737/omnia-protocol/commit/c4a75663f55e2ab040f073c5b9070df9ffc0a928))
- **fmt:** collapse eprintln! to single line in e2e_multi_node_consensus.rs ([9c4d15a](https://github.com/Willow7737/omnia-protocol/commit/9c4d15a556eb688c618eddfef574ddb822ec3784))

## [0.1.53](https://github.com/Willow7737/omnia-protocol/compare/v0.1.52...v0.1.53) (2026-05-24)

### Bug Fixes

- **ci:** sync release-please manifest + auto-fix version in Release workflow ([ce55c13](https://github.com/Willow7737/omnia-protocol/commit/ce55c13addf240d8410d84e417b0505269019afd))
- **ci:** sync release-please manifest + auto-fix version in Release workflow ([11fdfd4](https://github.com/Willow7737/omnia-protocol/commit/11fdfd40632a903bb5335c5d404f36cbca82400f))

## [0.1.52](https://github.com/Willow7737/omnia-protocol/compare/v0.1.51...v0.1.52) (2026-05-24)

### Features

- **dkg:** fix three critical DKG bugs + wire bootstrap peers (Group A) ([9067163](https://github.com/Willow7737/omnia-protocol/commit/90671637864dcbd0a7cdcdacb7db8428d7f0264a))
- **dkg:** fix three critical DKG bugs + wire bootstrap peers (Group A) ([423a7bf](https://github.com/Willow7737/omnia-protocol/commit/423a7bf1a32f5723f188f21e8c214005b5511cd7))
- Docker Compose E2E test + benchmark regression gates (Group C) ([c1653f3](https://github.com/Willow7737/omnia-protocol/commit/c1653f3f7b0828536026c03311b252e0e21ca428))
- wire ShardRouter + E2E multi-node consensus test (Group B) ([b0e709e](https://github.com/Willow7737/omnia-protocol/commit/b0e709e85a4e58076ce0abeba4357bbf752a7321))

### Bug Fixes

- borrow nodes in e2e test loops to avoid move-after-use ([131a47f](https://github.com/Willow7737/omnia-protocol/commit/131a47ff402c58065d64e8a98b8ba7c8d6416256))
- **crypto:** correct blst API type mismatches + LE/BE scalar encoding ([1656553](https://github.com/Willow7737/omnia-protocol/commit/16565536bf86993ac38f432c0085a0d3279787ff))
- HashSet::contains borrow check + clippy unwrap_used denial ([14b5477](https://github.com/Willow7737/omnia-protocol/commit/14b54771dd216ffe98e4b5b230b1f5f8adf0ad69))
- **shards:** resolve broken rustdoc intra-doc links ([2354958](https://github.com/Willow7737/omnia-protocol/commit/23549581d7aff424246872cf1bf679ab9aff3fb2))

## [0.1.51](https://github.com/Willow7737/omnia-protocol/compare/v0.1.50...v0.1.51) (2026-05-23)

### Bug Fixes

- **ci:** bump workspace version to 0.1.50 and inherit in all crates ([e6849ba](https://github.com/Willow7737/omnia-protocol/commit/e6849bab824a40b74d89dc7da59afca6396abd30))
- **ci:** bump workspace version to 0.1.50 and inherit in all crates ([f0f491d](https://github.com/Willow7737/omnia-protocol/commit/f0f491dcc507687d766cc134931ba00f6491a26e))
- **ci:** update Cargo.lock to reflect workspace version 0.1.50 ([129174d](https://github.com/Willow7737/omnia-protocol/commit/129174d6d8b02d699deab98c94ade359e7a9b85f))

## [0.1.50](https://github.com/Willow7737/omnia-protocol/compare/v0.1.49...v0.1.50) (2026-05-23)

### Bug Fixes

- allow release-please to use PAT for credentials ([c1acefc](https://github.com/Willow7737/omnia-protocol/commit/c1acefc4bdd51f3119a328d2f6eec81ef29a8f4f))

## [0.1.49](https://github.com/Willow7737/omnia-protocol/compare/v0.1.48...v0.1.49) (2026-05-23)

### Bug Fixes

- apply rustfmt formatting and increase binary size gate to 16 MB ([552d450](https://github.com/Willow7737/omnia-protocol/commit/552d450a33e6fee5584b162fae88947080cad702))
- **P0-1:** Wire GossipProtocol into node binary ([a4df6c1](https://github.com/Willow7737/omnia-protocol/commit/a4df6c1aee2c61b31dd2390334e46aa13aec6109))
- **P0-1:** Wire GossipProtocol into node binary ([807f093](https://github.com/Willow7737/omnia-protocol/commit/807f093ed4256e9f74d3210cb8b21629fc6f8ee8))
- **P0-2:** Integrate bls12_381_scalar module into FeldmanVssSession ([e3f50f8](https://github.com/Willow7737/omnia-protocol/commit/e3f50f8f173bb93e6e6025755de25cfc5a0cfd3e))
- **P1-1,P2-1,P3-1,P3-2:** BatchProofCircuit type safety, malformed proof rejection, auth tests, ceremony client ([f727241](https://github.com/Willow7737/omnia-protocol/commit/f72724163a6b06200a05d71108fde8f487e654b8))
- pin Foundry toolchain to stable instead of nightly ([f934196](https://github.com/Willow7737/omnia-protocol/commit/f934196221e698c5a707e516eab9e2c7b170ec02))
- resolve broken intra-doc links in substrate/src/lib.rs ([4ea63ff](https://github.com/Willow7737/omnia-protocol/commit/4ea63ff5f3397e7ef4944bf88979ce56790493a5))
- resolve clippy doc comment warnings in shard modules ([428f89c](https://github.com/Willow7737/omnia-protocol/commit/428f89c1cd23a725dd5c83143e9efea9ab94a510))

## [0.1.48](https://github.com/Willow7737/omnia-protocol/compare/v0.1.47...v0.1.48) (2026-05-22)

### Bug Fixes

- Add BLS12-381 scalar field arithmetic for DKG (P1) ([c0b758f](https://github.com/Willow7737/omnia-protocol/commit/c0b758fc907ad600d75dd482c35acb46596aa0c6))
- Address P0-P3 issues from code review v0.1.47 ([3243ed8](https://github.com/Willow7737/omnia-protocol/commit/3243ed87cc459629cd70726e756fd089380ce18e))
- correct bls12_381_scalar arithmetic and merkle doctest ([78f30c8](https://github.com/Willow7737/omnia-protocol/commit/78f30c8c819fb7159de18d4f9fdd04cfdc7478e4))
- resolve compilation errors and CI failures on dev branch ([28d44ab](https://github.com/Willow7737/omnia-protocol/commit/28d44abedf5f5ab1361b9ca6606f2502192fc169))
- resolve remaining compilation errors and CI failures ([345f082](https://github.com/Willow7737/omnia-protocol/commit/345f08284680f22e0491ba15b2b08935845a7825))

### Documentation

- Update README and PROJECT_DASHBOARD for v0.1.47 ([e47b7fa](https://github.com/Willow7737/omnia-protocol/commit/e47b7fa5edbe8e46c55920a39a73bb0dcc63d754))

### CI

- Add dev branch triggers to CI workflows ([fec7b68](https://github.com/Willow7737/omnia-protocol/commit/fec7b68f75bf2b97ff2a4bad64afa453694c7aa3))
- Add dev branch triggers to CI workflows ([3ab72be](https://github.com/Willow7737/omnia-protocol/commit/3ab72be6919c5226e720f2eac9856ea958c7bcfe))

## [0.1.47](https://github.com/Willow7737/omnia-protocol/compare/v0.1.46...v0.1.47) (2026-05-22)

### Features

- sprint fixes — deterministic key derivation, poseidon merkle, real_verification, owner pre-flight ([f6e41bb](https://github.com/Willow7737/omnia-protocol/commit/f6e41bbaaa8a7457f4c17ded6480137efd2969eb))
- sprint fixes — deterministic key derivation, poseidon merkle, real_verification, owner pre-flight ([26d9999](https://github.com/Willow7737/omnia-protocol/commit/26d9999fe357515989b5121170c6e42f8b5a8ac9))

### Bug Fixes

- resolve rustdoc intra-doc link for validate_with_caller ([be770b8](https://github.com/Willow7737/omnia-protocol/commit/be770b8d9df093a987a775d8aef02985b1417880))

## [0.1.46](https://github.com/Willow7737/omnia-protocol/compare/v0.1.45...v0.1.46) (2026-05-22)

### Features

- Phase 0 Critical Remediation — 11 code review fixes across 3 phases ([57ba4dd](https://github.com/Willow7737/omnia-protocol/commit/57ba4dd6665526789b25498ac3ac2d2fdb39fc03))
- Phase 0 Critical Remediation — 11 code review fixes across 3 phases ([5645c55](https://github.com/Willow7737/omnia-protocol/commit/5645c5565441f8b828665c04eeb058d906c908f6))

### Bug Fixes

- **ci:** add CircuitSpecificSetupSNARK trait import + cargo fmt ([ab9d4f8](https://github.com/Willow7737/omnia-protocol/commit/ab9d4f8499447d7c62630d81d513775c89f7535b))
- **ci:** add Clone derive to RollupCircuit, deref-then-clone for Groth16::setup ([f8b2a12](https://github.com/Willow7737/omnia-protocol/commit/f8b2a12f85cc46a8bb309b20dc0b56a702ff6654))
- **ci:** clippy overly_complex_bool_expr, SRS test, network integration tests ([c62b17a](https://github.com/Willow7737/omnia-protocol/commit/c62b17af20394acbfcf9905847a1150c4fbf7869))
- **ci:** declare rng as mutable for Groth16::setup ([fc73e15](https://github.com/Willow7737/omnia-protocol/commit/fc73e15afb2ff5acb3c97a1733bb772f34ecffe8))
- **ci:** Groth16::setup signature + missing max_sequence_entries ([6924083](https://github.com/Willow7737/omnia-protocol/commit/6924083e9729dec7307be47b647995f5ee430ea1))
- **ci:** network test missing field, clippy bool_comparison, SRS test determinism ([029eeb7](https://github.com/Willow7737/omnia-protocol/commit/029eeb7067c60e1b75dccc1b4a4a198151be45c0))
- **ci:** use turbofish Groth16::&lt;Bn254&gt;::setup to resolve type inference ([b4dd099](https://github.com/Willow7737/omnia-protocol/commit/b4dd09900c3427a96f9fcae312a9da9e4ba1851d))
- sign burn event with account owner keypair in double-spend test ([2d561a7](https://github.com/Willow7737/omnia-protocol/commit/2d561a7a1496d6b4d09b8acf32b428812b8f7a4f))
- use real keypair for account owner in cross_shard burn test ([c5a327f](https://github.com/Willow7737/omnia-protocol/commit/c5a327f21ada5de0bd2674dae8689711792ab54e))

## [0.1.45](https://github.com/Willow7737/omnia-protocol/compare/v0.1.44...v0.1.45) (2026-05-22)

### Bug Fixes

- **ci:** allow deprecated DkgSession in re-exports ([18a8518](https://github.com/Willow7737/omnia-protocol/commit/18a8518d9b32c6984575cb686071440fc7f91f3f))
- **ci:** replace redundant closure with function reference in merkle.rs ([aacf953](https://github.com/Willow7737/omnia-protocol/commit/aacf953bf70c0bd85da2782dd06f7ad49206d899))
- resolve all 14 code review issues + multi-node consensus test ([631dd31](https://github.com/Willow7737/omnia-protocol/commit/631dd311cb10e2b0e7077136aea35931ffb2ff02))
- resolve all 14 code review issues + multi-node consensus test ([f467ebe](https://github.com/Willow7737/omnia-protocol/commit/f467ebe14680ab6bbf43d2d6a9ee3ddd4597b870))
- **test:** bloom filter race in test_three_node_with_optimized_gossip_components ([e0ad142](https://github.com/Willow7737/omnia-protocol/commit/e0ad14216c53787d5a224f5b9b5f94cd8124d19f))

## [0.1.44](https://github.com/Willow7737/omnia-protocol/compare/v0.1.43...v0.1.44) (2026-05-21)

### Features

- **sprint-0:** foundation & baselines ([bf99745](https://github.com/Willow7737/omnia-protocol/commit/bf997451097ba5cd7a0fe1a586844238534b0503))
- **sprint-1:** sharded consensus state with parallel event validation ([c09173e](https://github.com/Willow7737/omnia-protocol/commit/c09173eec2870999a7470aea5f222a074f0a0fc7))
- **sprint-1:** sharded consensus state with parallel event validation ([ae41956](https://github.com/Willow7737/omnia-protocol/commit/ae419569eced76937009c9927ccd91e5f35fca77))
- **sprint-2:** batch event submission & processing ([4814ea5](https://github.com/Willow7737/omnia-protocol/commit/4814ea5692d180f2d0387026390b3ed30c410dad))
- **sprint-3:** optimized graph insertion with pre-allocated data structures ([32852fd](https://github.com/Willow7737/omnia-protocol/commit/32852fddc0e652b08b37cc8c3b755736eb2fe60a))
- **sprint-4:** network-optimized gossip protocol ([80a790d](https://github.com/Willow7737/omnia-protocol/commit/80a790d160d7048f38e225449b74a15cc9582a63))
- **sprint-5:** Phase 0 integration, stability & sign-off documentation ([c4f8b12](https://github.com/Willow7737/omnia-protocol/commit/c4f8b129f22c4fd0a9490c74edfefaa7c3f6a919))
- **sprint-5:** stability test framework and full chaos test suite ([1d952dc](https://github.com/Willow7737/omnia-protocol/commit/1d952dce2ed57fddaff9e3164c6db644196d382e))

### Bug Fixes

- **ci:** resolve clippy errors, formatting issues, and test failure ([ac782b9](https://github.com/Willow7737/omnia-protocol/commit/ac782b902c450e0186c85a41e0f3f57cc8ce8538))
- **ci:** resolve clippy unwrap_used errors and warnings ([f1176e6](https://github.com/Willow7737/omnia-protocol/commit/f1176e6fc62d45bad22ec6ba1266e8c895556cc2))
- **ci:** resolve rustdoc broken links and invalid HTML tags ([fccdf59](https://github.com/Willow7737/omnia-protocol/commit/fccdf59d624512acafc76961324a037e1a6451d9))

## [0.1.43](https://github.com/Willow7737/omnia-protocol/compare/v0.1.42...v0.1.43) (2026-05-21)

### Bug Fixes

- **ci:** comprehensive Clippy fixes, udeps nightly, and deprecated warnings ([f4c5dd6](https://github.com/Willow7737/omnia-protocol/commit/f4c5dd6374061e91b2e6e4fbc2e32bdda9901104))
- **ci:** comprehensive Clippy fixes, udeps nightly, and deprecated warnings ([ca9aaad](https://github.com/Willow7737/omnia-protocol/commit/ca9aaad659467d41285c5364040fbf495f622c8c))
- **ci:** resolve all 7 CI failures ([47ff783](https://github.com/Willow7737/omnia-protocol/commit/47ff7837f2d1610195dc889382e1eeaff1a6ed89))
- **ci:** resolve fuzz ASAN linker errors, benchmark deps, fmt, udeps toolchain, badge ([c80b325](https://github.com/Willow7737/omnia-protocol/commit/c80b3251e30f1e548507ad686f32a3cd2c1f3e21))
- **ci:** resolve fuzz ASAN linker errors, benchmark deps, fmt, udeps toolchain, badge ([a9db9f9](https://github.com/Willow7737/omnia-protocol/commit/a9db9f96da785d5fa1c2647e8e9e51aa3cc989cd))
- **ci:** resolve remaining Clippy, fmt, and feature-gate CI failures ([0f47fed](https://github.com/Willow7737/omnia-protocol/commit/0f47fedbb8983ff326d04a0db4b89ae58fd00739))
- **ci:** resolve remaining Clippy, fmt, and feature-gate CI failures ([cd07277](https://github.com/Willow7737/omnia-protocol/commit/cd072778aa8ca2a703421a2046042b7fd1854147))
- **ci:** resolve unsafe_code forbid conflict and gate pqc test ([e993d07](https://github.com/Willow7737/omnia-protocol/commit/e993d07f2d5c3554bfb69c84306143148da415b4))
- **ci:** resolve unused imports, missing ApiDoc import, and broken doc links ([a4936df](https://github.com/Willow7737/omnia-protocol/commit/a4936df66465a65aa34e8653be200bc7e3350b3a))
- **ci:** resolve unused imports, missing ApiDoc import, and broken doc links ([73ba66a](https://github.com/Willow7737/omnia-protocol/commit/73ba66a38cdd082fc5ff2e4df79b72a491fed7c7))
- **doc:** replace broken intra-doc links for feature-gated types ([307452c](https://github.com/Willow7737/omnia-protocol/commit/307452ca47b70a671550f8e80eba71b7c1d6ae6c))
- **doc:** resolve remaining broken intra-doc links across workspace ([669be6b](https://github.com/Willow7737/omnia-protocol/commit/669be6b1940b755184a4e5eda23910fef50a2cb5))
- resolve 6 CI-breaking bugs + 5 minor issues ([2ed119e](https://github.com/Willow7737/omnia-protocol/commit/2ed119e4d38880f5fdec1d1bba76faff97537a2e))

### Documentation

- comprehensive documentation audit — align all docs with current project state ([5960027](https://github.com/Willow7737/omnia-protocol/commit/596002736e4913c9097be0254235d1ecce76d490))

## [0.1.42](https://github.com/Willow7737/omnia-protocol/compare/v0.1.41...v0.1.42) (2026-05-20)

### Bug Fixes

- **ci:** resolve all CI workflow failures and broken README badge ([80ff258](https://github.com/Willow7737/omnia-protocol/commit/80ff2585fb59e4a0feed8792722e61fe636a2dfd))
- **ci:** resolve all CI workflow failures and broken README badge ([a68d2c8](https://github.com/Willow7737/omnia-protocol/commit/a68d2c8549dfa4dbff819a68ac61636a2c070f56))

## [0.1.41](https://github.com/Willow7737/omnia-protocol/compare/v0.1.40...v0.1.41) (2026-05-20)

### CI

- stabilize nightly fuzz & security audits ([9d53380](https://github.com/Willow7737/omnia-protocol/commit/9d5338051b87739ad4295d93b2134613f7c2943b))
- stabilize nightly fuzz & security audits ([76783f3](https://github.com/Willow7737/omnia-protocol/commit/76783f38605a2068137a9e803fb1b8fafbb589c2))

## [0.1.40](https://github.com/Willow7737/omnia-protocol/compare/v0.1.39...v0.1.40) (2026-05-20)

### Documentation

- consolidate & eliminate duplication ([63beef3](https://github.com/Willow7737/omnia-protocol/commit/63beef33fa739ef4fec7d6dc28d1d2eefcae31fc))
- consolidate & eliminate duplication ([8c6a2b5](https://github.com/Willow7737/omnia-protocol/commit/8c6a2b5e758d95ab9ee65832ab969ed3e7fd7af8))

## [0.1.39](compare/v0.1.38...v0.1.39) (2026-05-20)

### Bug Fixes

- decouple alloy from MSRV via Hybrid Settlement pattern ([aded2bb](commit/aded2bbd2632aeef471b23e870bd9e92aa53d907))

## [0.1.38](compare/v0.1.37...v0.1.38) (2026-05-20)

### Documentation

- restructure documentation into organized hierarchy ([7850778](commit/7850778f5e98f86d3d37c2be8135893a785d7618))
- restructure documentation into organized hierarchy ([f6be244](commit/f6be2449e659b81c4dc799f6973b1d932613514a))

## [0.1.37](compare/v0.1.36...v0.1.37) (2026-05-20)

### Features

- implement Hybrid Settlement Architecture ([67ce911](commit/67ce911a9919fc39bef513bfc1c1ff1f1f6d78cb))
- implement Hybrid Settlement Architecture ([c260daf](commit/c260dafaa1d72098961617d92cb2bfccdbb2a9e5))

## [0.1.36](compare/v0.1.35...v0.1.36) (2026-05-20)

### Bug Fixes

- **ci:** resolve CI failures — alloy MSRV exemption, ethereum-live toolchain, test skip, baseline fallback ([93c0d29](commit/93c0d2963e966752e1c0a7f24adba5600c419048))
- **ci:** resolve CI failures — alloy MSRV exemption, ethereum-live toolchain, test skip, baseline fallback ([0edae16](commit/0edae16326405b0296f219afcc8cf8effdb8175f))

## [0.1.35](compare/v0.1.34...v0.1.35) (2026-05-19)

### CI

- rectify workflows post-restructuring (Step 6) ([857ada3](commit/857ada355497d14f4d6ce3d8665778d58e3cd150))
- rectify workflows post-restructuring (Step 6) ([f3273b7](commit/f3273b7f4d4112b650010c72e073b1f8a426ad62))

## [0.1.34](compare/v0.1.33...v0.1.34) (2026-05-19)

### CI

- rectify workflows post-restructuring (Steps 1–5.5) ([07e668d](commit/07e668d903875f27fe048c61c56685ead2462ba8))
- rectify workflows post-restructuring (Steps 1–5.5) ([b68e832](commit/b68e8326704a5f0a59e40f4f22d58fff85e6905c))

## [0.1.33](compare/v0.1.32...v0.1.33) (2026-05-19)

### Bug Fixes

- escape square brackets in rustdoc comments to fix CI doc build ([0a3036e](commit/0a3036ecf007da9fdcc4a470cfb872a35fd35053))

## [0.1.32](compare/v0.1.31...v0.1.32) (2026-05-19)

### Bug Fixes

- **phase-5:** populate Poseidon reference constants, implement dual-hash, fix formatting ([3bd9bd7](commit/3bd9bd7dcab4604de31a22f433134a1dec603d8b))

## [0.1.31](compare/v0.1.30...v0.1.31) (2026-05-19)

### Features

- **phase-5:** real benchmarks, fix ECVRF, fix BFT tests, populate BASELINE.md ([d400042](commit/d400042350d5e3f7b02c692206055d4d33fadaab))

### Documentation

- **phase-5:** update summary with real benchmark data and bug fixes ([00f1ea8](commit/00f1ea8c023775b211b5e659604e8d18b88d9030))

## [0.1.30](compare/v0.1.29...v0.1.30) (2026-05-19)

### Features

- **phase-5:** testnet launch, performance validation & audit preparation ([c57f281](commit/c57f28160cbb1b73b91de41efbc409523996393e))

## [0.1.29](compare/v0.1.28...v0.1.29) (2026-05-19)

### Bug Fixes

- **ci:** fix cargo fmt and cargo-vet remaining issues ([759a675](commit/759a675c1908a9a79cc8ecb2bf5897655356305f))
- **ci:** resolve broken doc links and cargo-vet unvetted deps ([bf32351](commit/bf32351c595734b4b6abc175a10971564d814be5))

## [0.1.28](compare/v0.1.27...v0.1.28) (2026-05-19)

### Bug Fixes

- **ci:** resolve all failing CI workflow jobs ([7092a68](commit/7092a68372875ae61b5e90dfafb841bc50c2351d))

## [0.1.27](compare/v0.1.26...v0.1.27) (2026-05-19)

### Bug Fixes

- **ci:** resolve all failing CI workflow jobs ([1e0eab7](commit/1e0eab7fdeb73fe0ac037c3155717b6ba2950f18))

## [0.1.26](compare/v0.1.25...v0.1.26) (2026-05-19)

### Features

- Phase 4 — Mainnet readiness, settlement integration & architectural closure ([97c2e3c](commit/97c2e3cd7b70f53dcc873aab3834b2f1df3e260b))

## [0.1.25](compare/v0.1.24...v0.1.25) (2026-05-18)

### Bug Fixes

- **ci:** resolve all failing CI workflow jobs ([dc4b19f](commit/dc4b19f2ecdde425a7646adec2a7eea465af375c))

## [0.1.24](compare/v0.1.23...v0.1.24) (2026-05-18)

### Features

- Phase 3 — Critical security closure, network production readiness, and cryptographic completion ([f8285ab](commit/f8285ab6641fe385f17a81b3ffa205212b4fe2eb))

## [0.1.23](compare/v0.1.22...v0.1.23) (2026-05-18)

### Bug Fixes

- **zk:** resolve rustdoc broken link to apply_contribution_ec ([dcc9119](commit/dcc911991af0b88f4578f438402506951c5a34ac))

## [0.1.22](compare/v0.1.21...v0.1.22) (2026-05-18)

### Bug Fixes

- **ci:** resolve all failing GitHub Actions workflows ([74e8ba5](commit/74e8ba502e3b4bce91cf25c048b0739bfb006a19))

## [0.1.21](compare/v0.1.20...v0.1.21) (2026-05-18)

### Features

- **binding:** integrate PQC key rotation with encrypted keystore (H-4) ([ba6f0f8](commit/ba6f0f8ba007d9a0c8cfea346a79c0f6056efe0f))
- **shards:** fix SSS recovery flow with encrypted shares and key derivation (C-1) ([bead81a](commit/bead81af45cb72b6862281c2ddcf90a00edca0dd))
- **substrate:** add BIP-39 mnemonic support to keystore (M-1) ([068f32b](commit/068f32bb3a4c954b328a10eb78ca7919a88e9ad5))
- **substrate:** add gradual slashing with jail/suspension and events (H-5) ([17e08f2](commit/17e08f206d25a273432a68ec3b16da296d1fc10e))
- **substrate:** implement DKG for threshold signatures (M-2) ([71ba10a](commit/71ba10a6cdc2177da079a84c25ccc4e956cc98b7))
- **zk:** add Groth16 batch verification (H-3) ([8ae958f](commit/8ae958ff2d0aa8622ce7b02fcae619821b3f1307))
- **zk:** add ZK-SNARK benchmark suite (H-2) ([ad795d5](commit/ad795d50c4bc6390d0a8ce88f447293d2e4f3dd8))
- **zk:** fix trusted setup ceremony with real EC scalar multiplication (C-2) ([65e00be](commit/65e00beb9bdce7090ed3beb368cfcee03b18d2d4))
- **zk:** populate circuit dummy fields with event semantics constraints (H-1) ([8738e05](commit/8738e059a8e8192e259d7738d4f1c91dec80c4d6))

### Documentation

- add ADRs 010-014 and update project dashboard (M-4, M-5) ([ef52634](commit/ef5263448820b272a12c3399104dd9d9e87f34db))
- add PHASE_2_SUMMARY.md documenting all Phase 2 deliverables ([a269a49](commit/a269a4994bacfa9d6394ed51649da337588c87b9))

## [0.1.20](compare/v0.1.19...v0.1.20) (2026-05-17)

### Bug Fixes

- **ci:** resolve cargo fmt, doc link errors, and codecov token failure ([2fbfe5f](commit/2fbfe5f3f4c55f19ab0b7b47fd063d11a5209993))

## [0.1.19](compare/v0.1.18...v0.1.19) (2026-05-17)

### Bug Fixes

- resolve compilation errors, clippy warnings, and test failures ([87d52bd](commit/87d52bdee0721d9cd858f8e4a7e3ccfbbfa8ad0d))

## [0.1.18](compare/v0.1.17...v0.1.18) (2026-05-17)

### Bug Fixes

- **ci:** fix CI coverage enforcement and remove continue-on-error on security jobs ([1716c4c](commit/1716c4c8bfa475cbae719cc402829cd59466f8fe))
- **ci:** fix release workflow and nightly fuzz error handling ([9ce12a5](commit/9ce12a5858d1d10502e3cf63874f67ea4ed9c45a))
- **deps:** set unknown-git to deny in deny.toml ([dc7bd12](commit/dc7bd126a332c4fd2532a7b4e55df4551ec9c3cb))
- **economics:** use saturating_add for vote accumulation in governance ([a579608](commit/a5796084907b1dc374271f8a14b6a4587175db94))
- **fuzz:** update OSS-Fuzz Dockerfile to Rust 1.85 ([37cddcf](commit/37cddcfedf32067e1a1228b09f8167993d293c97))
- **security:** add constant-time comparisons in ZK and binding crates ([be29809](commit/be298099f2d1c89b01538bd033fe07d642a05392))
- **security:** Phase 1 - Critical smart contract fixes for OmniaRollup.sol ([7d52c57](commit/7d52c5753153b0eb35253587f54a419d7542dd9d))
- **shards:** change Shard state_snapshot() trait to return Result ([b34cc66](commit/b34cc66d260221103b60f2910e6c89228566b276))
- **shards:** return Result from ShamirRecovery::gf_inverse() instead of panicking ([46b2e73](commit/46b2e732c78159b8a164ae9cdb0f4002870fc3b7))
- **shards:** use saturating_add for EconomicsShardState balance operations ([abb4183](commit/abb418369d294d6d1dfd7919bcaad8e32290a7c7))
- **substrate:** add auto-upgrade path for legacy XOR keystores ([ceb5ca6](commit/ceb5ca61e32c9b7f4a0a16f6b2b7033042941265))
- **substrate:** add bounded caches and pruning for unbounded collections ([743c9c4](commit/743c9c45c2688eab14d51f928912faec92ed9521))
- **substrate:** deduplicate committed events in check_commitments with HashSet ([af33da1](commit/af33da1fa3d597d4c19b06d2c52053e465b32235))
- **substrate:** fix can_strongly_see() EventPruned handling - propagate error instead of false default ([c562a94](commit/c562a945081bb6b843f15e913a5b15e24d9d1964))
- **substrate:** fix finalized_order() O(n²) performance with HashSet ([470f82b](commit/470f82b8cb9ccf9cdb73ea8c7caaf432909bb473))
- **substrate:** fix GCounter overflow and add crate lints to chaos-tests ([51e677b](commit/51e677bdf8616f04fe8c927a38f1bc9b10845f96))
- **substrate:** fix SlashingEngine Clone divergent state with Arc&lt;RwLock&lt;SlashingState&gt;&gt; ([774a597](commit/774a5973bb8853dcfc0ef4964d3b1df089e42840))
- **substrate:** fix verify_integrity() for pruned graphs by checking pruned_events ([890be85](commit/890be85474b26905133354ab334a02bf043ade36))
- **substrate:** propagate Result in KeyStore AES-256-GCM functions ([fc58998](commit/fc589985fb3d2a61c6a80998ce80988015078b9e))
- **substrate:** return Result from ConsensusConfig::with_random_seed() and BlsKeypair::generate_random() ([cae0963](commit/cae096342b43eec505f42c122320e425ec1e8caa))
- **zk:** make Ethereum settlement stub return error instead of true ([9a97686](commit/9a9768623a9915029719a67db671d525e1e37c4f))
- **zk:** return Result from PowersOfTau::new() instead of using expect() ([92fdcbb](commit/92fdcbb63e86683d7075431ca23904cef9b8bdb6))

### Documentation

- replace all sled references with redb in documentation ([f52f769](commit/f52f769140882aeafaeb95cb8d4831a5635a6032))
- replace bincode references with postcard in documentation ([93ad377](commit/93ad3774dc8fecb8f04b8c137c1077232f726544))

## [0.1.17](compare/v0.1.16...v0.1.17) (2026-05-17)

### Bug Fixes

- **ci:** add RUSTSEC-2025-0141 (bincode v1 unmaintained) to ignore lists ([1f67a22](commit/1f67a22c3d6b925a01455cc32dae03451d6f3dc3))

## [0.1.16](compare/v0.1.15...v0.1.16) (2026-05-17)

### Bug Fixes

- **ci:** resolve all failing GitHub Actions workflows ([7203862](commit/72038620e2f3bf70daadf78be0b30e26ae9193db))

## [0.1.15](compare/v0.1.14...v0.1.15) (2026-05-17)

### Features

- **substrate:** add bincode v0 fallback deserialization in wire format ([b6b66da](commit/b6b66da6d6599efea5a36727d102dadc3d190f09))
- **substrate:** add sled-to-redb migration utility ([7b28dc3](commit/7b28dc32be21e3540d97f4f13d06296854c8a8a2))
- **tooling:** add cargo-mutants configuration ([d8fba17](commit/d8fba172c5730af6d912381ef2ede69c1abdbe49))

### Bug Fixes

- **api:** remove false rate-limiting claim and unused ErrorCode::RateLimited variant ([58f7639](commit/58f7639badceefd5e7515ed4f2794bfda18793d7))
- **causal-graph:** return EventPruned instead of MissingParent for pruned parents in insert() ([377da82](commit/377da826469c580de9a65dceb2d83392f75abc48))
- **causal-graph:** return Result from topological_order() to detect pruned ancestry ([82ab609](commit/82ab6099e34fd754e612acbaba122b4f5415222c))
- **ci:** add clippy::unwrap_used checks to CI pipeline ([37b038b](commit/37b038bd2c2647f08cd7ef6a1017d1bdd70e5573))
- **consensus:** detect equivocation even when first event is pruned ([b58d092](commit/b58d092badfbd4af7237ad667cb1032f216e8fb0))
- **helm:** replace {{ .Service }} with {{ .Release.Service }} in all templates ([8c7995b](commit/8c7995bbd17578d7639e0369e4d9759d188cae62))
- **lints:** add #[allow(clippy::unwrap_used)] to all test modules and integration tests ([11aafc7](commit/11aafc7dd452e4918926c7740bc9033b8b6c7cc1))
- resolve compilation errors in migration.rs, poseidon.rs, and Cargo.toml ([5530fe2](commit/5530fe216b33d63d3ea740b5aef50d1d4f6d6e9e))
- **rollup:** add withdrawal access control, escrow pattern, and reentrancy guard ([7c85e20](commit/7c85e20dfae3126f48bb1d725099091cfa3bd570))
- **rollup:** bind state root to proof public inputs in submitBatch() ([02ba4a6](commit/02ba4a6ae9f50272947e203eb7fe59c2825ec89d))
- **shards,economics:** convert to_bytes() from panicking expect() to Result return type ([206e8a2](commit/206e8a2ceb3a42992314304c6c7b7d0992c15650))
- **substrate,chaos-tests:** replace graph.get() with graph.get_checked() for pruned event handling ([ae9927d](commit/ae9927dffd8215205ff841ce0ebd24cf8d649b10))
- **zk:** replace expect() with Result return type in poseidon.rs and contribution.rs ([2c2896d](commit/2c2896dcb986e7d33920f2a1d90546c8a232d5f2))

### Documentation

- **self-assessment:** update section 3.2 to reflect Poseidon hash implementation ([5bdb993](commit/5bdb9930ff87bba5d9e522976d443977e2f19b85))

## [0.1.14](compare/v0.1.13...v0.1.14) (2026-05-17)

### Bug Fixes

- **ci:** ignore transitive vulnerability advisories in cargo-deny and cargo-audit ([56841f2](commit/56841f27d2bfe24a23bdadae445b03d7c6bac80b))

## [0.1.13](compare/v0.1.12...v0.1.13) (2026-05-17)

### Bug Fixes

- **ci:** fix cargo-deny license and wildcard errors ([4720794](commit/472079453dab9acd8555166cf512a57db691bf58))

## [0.1.12](compare/v0.1.11...v0.1.12) (2026-05-17)

### Bug Fixes

- **ci:** remove deprecated deny.toml keys (unlicensed, copyleft, default) for cargo-deny 0.19.x ([fa044f2](commit/fa044f2e57ab86d7482c3b8fcd235c9215af5df5))

## [0.1.11](compare/v0.1.10...v0.1.11) (2026-05-17)

### Bug Fixes

- **ci:** fix deny.toml yanked field value — accepts allow/warn/deny not workspace ([52bb45e](commit/52bb45ef38cf852ba8cba8fe288f36dfdd0899b3))

## [0.1.10](compare/v0.1.9...v0.1.10) (2026-05-17)

### Bug Fixes

- **ci:** resolve all failing GitHub Actions workflows ([5c6d570](commit/5c6d570cccd00deb86eb2a158ef94539944234f1))

## [0.1.9](compare/v0.1.8...v0.1.9) (2026-05-17)

### Bug Fixes

- make Prettier check non-blocking, exclude generated files ([ebb3c0f](commit/ebb3c0f7458c857932d02211cfc992323c26dd2c))

## [0.1.8](compare/v0.1.7...v0.1.8) (2026-05-17)

### Bug Fixes

- remaining rustfmt formatting and deny.toml config ([b75c4fa](commit/b75c4fac0bf4caf946493333db9045b9cda710bd))

## [0.1.7](compare/v0.1.6...v0.1.7) (2026-05-17)

### Bug Fixes

- rustfmt formatting in errors.rs function signature ([41750fb](commit/41750fb56707ca61e43f03a4957cbf63a52ed768))

## [0.1.6](compare/v0.1.5...v0.1.6) (2026-05-17)

### Bug Fixes

- unwrap→expect replacements and utoipa schema derivations ([cbacbfe](commit/cbacbfe9b8b3f531a8492199ee77cae46d7dc9a7))

## [0.1.5](compare/v0.1.4...v0.1.5) (2026-05-17)

### Bug Fixes

- redb v2 API — use range() for iteration, remove drain() call ([3625438](commit/36254387f6a1f58a5d4d957868f0d9194eec53ce))

## [0.1.4](compare/v0.1.3...v0.1.4) (2026-05-17)

### Bug Fixes

- resolve remaining CI failures — compilation and audit ([1eb2193](commit/1eb2193f11ef584e4a54a399a604c8da0f1e0a9a))

## [0.1.3](compare/v0.1.2...v0.1.3) (2026-05-17)

### Bug Fixes

- resolve all CI workflow failures ([d1b2d2b](commit/d1b2d2be751b4ce9ff4be0d4ec8d3e6b0ea79173))

## [0.1.2](compare/v0.1.1...v0.1.2) (2026-05-16)

### Features

- A-grade quality improvement plan — all 23 tasks across 4 sprints ([d231c10](commit/d231c1023be94e6b07e796cf4707c8ac9f3ced6d))

## [0.1.1](compare/v0.1.0...v0.1.1) (2026-05-16)

### Features

- **ci:** add CD pipeline with release-please and automated binary/contract/Docker publishing ([8f9d108](commit/8f9d10872d0f408af99a509aebfd4ca07193731d))
- Initial commit - Omnia Protocol universal coordination layer ([f5d035a](commit/f5d035a35d1579a970d0b0c5022076c02461c930))
- integrate Layer 1 Substrate implementation and enhance documentation structure ([e1212c4](commit/e1212c43dff244565616db3882a85e60201bb357))
- **integration:** wire Layer 2 shards into Layer 1 substrate — EventProcessor trait + committed event routing ([f49b72f](commit/f49b72f358a63696dbc069f83930625769d6577e))
- **layer3:** binding layer — provenance log + RF stub + quantum commitment stub ([62ca8d8](commit/62ca8d8b95432ddbb9a3b7fd67398ac4466f212f))
- **layer4:** identity hardening — Shamir recovery + biometric anchors + AI agent identity ([b0ea4df](commit/b0ea4dfecc928e26ce3a2c69ea69b59ffdf69879))
- **layer5:** economics — UBC token + proof-of-useful-work + quadratic voting with decay ([d33de8f](commit/d33de8f3b3733d4f01924e4d2c135f55fa7a14b7))
- **phase0:** settlement-agnostic ZK-rollup — Ethereum adapter + Bitcoin/Solana/Celestia stubs ([c703dd4](commit/c703dd43debb5598dfbe99ada48fdc3913011d12))
- Sprint 3 — Testnet Readiness ([7800d26](commit/7800d26b090e5f064c14f46886548d28d56d9eba))
- Sprint 4 — security hardening, formal verification, cryptographic maturity, and operational readiness ([aac338c](commit/aac338c67df2df7350da235e0b428e835681b601))
- Sprint 4 final push — nonce wiring, VRF stake weighting, Grafana alerts, ceremony PoK ([4ff1deb](commit/4ff1deb416c63c96b27d671094fda201a5dfd68e))
- Sprint 5 — fuzzing, proptests, supply chain hardening, reproducible builds ([0c1cb52](commit/0c1cb528bac5b536dcf6cd7aa84ac1a28fad8fc0))
- Sprint 6 — Complete the Foundation ([0f6ceff](commit/0f6ceffc2ab08cf24eae28c9070ae2ae61dd68c4))
- **sprint-1:** foundation hardening — consensus docs, event validati… ([df14119](commit/df141199c574aee35c989afa7120c18d4565855e))
- **sprint-1:** foundation hardening — consensus docs, event validation, adversarial tests, ProofBundle, CI/CD, ADRs ([0d51070](commit/0d5107006628714faf329ef2131e6aefd8af0192))
- **sprint-6:** Complete the Foundation — all 8 phases ([9a79f63](commit/9a79f6394a404b89af640b323b28cebf1211ba2f))
- **sprint-7:** Remediate ARGUS-PANOPTES audit findings ([51241d7](commit/51241d7af57640ed80b31596cb5090a792018e83))
- **sprint-7:** remediate ARGUS-PANOPTES audit findings — 2 critical, 3 high, 5 medium, 2 low/info ([56dbcfa](commit/56dbcfad09baefae1e2e8da851600de09a17d356))

### Bug Fixes

- add missing nonce_data_dir field in integration test NodeConfig ([0da370e](commit/0da370e8cae726be17ce600bfe8926665b6be946))
- add required toolchain input to dtolnay/rust-toolchain@v1 ([2cf1285](commit/2cf128550996a470443b71edef933ddf57269248))
- **ci:** cargo fmt + ignore RUSTSEC-2024-0384 (instant via sled) ([33e3542](commit/33e3542eb73ed0838ffc898f886bd19ee01d038b))
- **ci:** exclude fuzz crate from workspace test/clippy/doc/coverage, fix cargo-vet command ([5ad0b3d](commit/5ad0b3d495ba1c723f54dcc84fa65602156025be))
- **ci:** fix cargo-vet config format, make supply-chain job non-blocking ([cc571a9](commit/cc571a939df6f511f62d6d11d1ac9e0670fa8a03))
- **ci:** fresh rustup install on macOS to bypass broken Homebrew rustup ([03ea642](commit/03ea642f6abdf5c67b20b8c2e26bf630f8c9f38d))
- **ci:** ignore RUSTSEC-2025-0055 audit, fix macOS cargo PATH ([fc28e66](commit/fc28e66474dfaff73c46957606126ca72d86e6fb))
- **ci:** move with_random_seed out of impl Default, fix remaining fmt issues ([e76a3d6](commit/e76a3d6c762485d11c663ef1f412ebbd8f8a5400))
- **ci:** properly initialize cargo-vet supply-chain with exemptions and imports ([8766ebf](commit/8766ebf0c2fa93a83ca2440e052dcb91c083a465))
- **ci:** provide non-zero round_seed in chaos-tests ([04da88c](commit/04da88c4405bdfcdefa3c0b81018d79f3ed959e9))
- **ci:** provide non-zero round_seed in SubstrateConfig constructors ([b8fa3ef](commit/b8fa3ef504b0fb7158df6c0de01d513d29a45d54))
- **ci:** re-add cargo to PATH after rust-cache on macOS ([2547b98](commit/2547b98a1d27788b23cf1644bbdabcdd4740e793))
- **ci:** resolve all CD pipeline failures — release-please, cross-compile, Windows, publish ([73e4353](commit/73e43533c44261859e369608ec7fe4f2cc8c69e2))
- **ci:** resolve clippy warnings and fuzz install resilience ([9e9b095](commit/9e9b095814fec7eae9cbeb1e82e53e29352d252e))
- **ci:** resolve compilation, formatting, and CI workflow errors ([f82aa21](commit/f82aa2167f7b0810200ce75a65dfe7daa86f841e))
- **ci:** resolve fmt, clippy, and ZK circuit test failures ([3a0d0b0](commit/3a0d0b09b68aa9c4a299c45ba095f048145cafc9))
- **ci:** resolve fmt, compilation, and Cargo.lock issues ([43768a6](commit/43768a64c8cb17c3f32874184987508b99cfbfd2))
- **ci:** resolve macOS cargo PATH loss after rust-cache restore ([e7994c1](commit/e7994c117a3fbebebe9ae5c57d9b2495c67efcec))
- **ci:** resolve macOS cargo=rustup-init Homebrew conflict ([99b1daf](commit/99b1daf47a1a57a7378c75242964f4f0a41ea3d4))
- **ci:** resolve Python toml import error and cargo-fuzz manifest issue ([ad328b5](commit/ad328b54ef5dda81e40769cefa38de288b401451))
- **ci:** resolve remaining fmt, SBOM, and fuzz CI issues ([99d13ea](commit/99d13ea9ca06c557c15e156728cc0abf3b328b39))
- **ci:** resolve rustdoc broken intra-doc links ([df6ad3f](commit/df6ad3fbc548640ab354c311f25f4fa9f4c1927f))
- **ci:** resolve rustdoc redundant explicit link target in zk/src/lib.rs ([e480478](commit/e4804788fb5b33e2fed14f47fb12c4c6be4236be))
- **ci:** resolve rustdoc warnings treated as errors ([3429b14](commit/3429b14802e62389e83e1f6bb75b592d738e17af))
- **ci:** update remaining protocol identifier test assertion to 4.0.0 ([dd2c417](commit/dd2c4179d2bb72161867dcf4e65ec9917473740c))
- **ci:** update test expectations for protocol version and undo rate limit ([1ac950f](commit/1ac950fc854599c9b577141338918da092b2b5f6))
- **ci:** use dtolnay/rust-toolchain for all platforms including macOS ([98c1976](commit/98c19762070b43505d90733083aedd7cfc9ef5d4))
- **ci:** wrap governance error return in Err(), remove dead seen_sequences field ([364d114](commit/364d114bb9074469fc1d9b1273f6c7add761d4cd))
- clippy bench warnings, drop MSRV matrix (deps require 1.86+) ([fd8498c](commit/fd8498c53bd69595ac299234ffdad9562829714d))
- commit Cargo.lock for security auditing and reproducible builds ([d68ef8d](commit/d68ef8da5338e711c089e0e150852ad972217b95))
- commit Cargo.lock for security auditing and reproducible builds ([a600515](commit/a600515b21cf984e5f0ee44cffc9caae30b6893a))
- configure cargo audit to ignore known transitive dep vulnerabilities ([e90549f](commit/e90549fb0fede5f37c88582614fe7b4abc9373e6))
- configure cargo audit to ignore known transitive dep vulnerabilities ([53434d8](commit/53434d89118d3b62fd4c475184f5dbe611e3679f))
- **docs:** resolve rustdoc warnings that break CI with -D warnings ([504aa08](commit/504aa081d7f3b9d41f451585d0f76892fc3b0642))
- **layer3:** strengthen links_to, add destroy_item, explicit blake3 dep ([14d807e](commit/14d807e37040cb494752d8bc016839f88df2ed65))
- make PROOF_BUNDLE_VERSION public to fix rustdoc private-intra-doc-links ([cc11864](commit/cc11864104dcc5243361b1c18454aa1e6d553360))
- **pre-rollup:** 4 critical gaps — replay protection, state root, event pruning, economics wiring ([facd051](commit/facd0515af3821d984c567a89287efdfcf4297e9))
- resolve all CI failures — fmt, clippy, and dependabot config ([a1cc009](commit/a1cc00900e2b84b609ffaab007dcecd9633e4bee))
- resolve all CI failures — fmt, clippy, and dependabot config ([6a1f00d](commit/6a1f00d2b8e216b0093dc7e6e82ce4c01fcb38c3))
- resolve all remaining CI failures ([1ef8eda](commit/1ef8edae4178c634444b4bfdf832427c18228511))
- resolve all remaining CI failures (audit action + toolchain pinning) ([4937f16](commit/4937f16d3f0159dd1909bc5da3eacab147feab64))
- single persistent SlashingEngine shared between consensus and API ([e4b0b40](commit/e4b0b40e360136f5077aca3b2f5ccfcb02c39412))
- **sprint-1:** CI overhaul, multi-OS testing, Docker testnet, fuzz targets ([8467c93](commit/8467c9395a5ddd009a2bacf3b5e9dd743fecace2))
- **sprint-1:** CI overhaul, multi-OS testing, Docker testnet, fuzz targets ([cfd9113](commit/cfd911341d4ba90578a415afdd041c4783b1e80c))
- **sprint-2:** critical hardening — 6 security fixes ([b31d148](commit/b31d148899b4ff620ce0a78de819385db801cda3))
- **sprint-2:** Critical Hardening — 6 Security Fixes ([4e398fc](commit/4e398fce0cf4dfc9f819efbf9edb0dce3cf05e91))
- three hotfix sprint blocking issues ([4cfd97d](commit/4cfd97d0dcf628f1bd1542fec54e2f18ba795810))
- upgrade CI toolchain to Rust 1.85 ([9e45fd1](commit/9e45fd10c427d23eea730dd7a72e27a6cf91adfa))
- upgrade CI toolchain to Rust 1.85 ([5286bbe](commit/5286bbed2bc2b1bda65f7a54e5d0d340cf9b95f9))
- use stable Rust toolchain in CI ([bcab7ef](commit/bcab7ef33ee38d56b9636f2d56f932b6a869ef9a))
- use stable Rust toolchain in CI to resolve transitive dep MSRV issues ([0fa9700](commit/0fa9700e31898ed5894f145b22b82a6554267755))

### Performance

- **substrate:** O(n) → O(new_events) — replace graph walk with unprocessed event queue ([2f45765](commit/2f457655324259aa183c94668dd6bd4fe83bb750))

### Documentation

- comprehensive documentation audit — align all markdown with current codebase ([e37d6ef](commit/e37d6ef91c9c2b271ba99d6f970617dad7e8d591))
- comprehensive repository beautification (excluding workflows due to permissions) ([5d8088d](commit/5d8088de797abff9f926086e8fa4695b809d4b21))
- comprehensive repository beautification and enhancement ([52bca8e](commit/52bca8e7f3d6871509dded8034c093a9a719cf1d))
- configure community channels and issue templates ([9b038df](commit/9b038dfc503b3c9f7b8068ec2066f7630b89371f))
- implement radical transparency dashboard and status tracking ([d1bf6d7](commit/d1bf6d7690a6878fcceaca02134d458ef1aa3bac))
- overhaul all markdown — match docs to actual codebase ([2381c77](commit/2381c77c6b22684d2592394417b9b2e7706bb0c3))
- update all markdown — honest content + preserved visual design ([c2f8034](commit/c2f803415fcdaeddc9e2ede2caed2e6761544643))
- update Discord server link to https://discord.gg/qYkpAeSYR ([d4face4](commit/d4face4940ea600dab6c74c0af9f6589aedc48a6))
- update README with direct community and tracking links ([144b670](commit/144b67089ea069fc78be5b4bd33d16e36a11ddf3))

### Build

- **deps:** bump actions/checkout from 4 to 6 ([94120e8](commit/94120e8a63d9cd9c6125cc0a3fb8ae30f7b7e2c8))
- **deps:** bump actions/checkout from 4 to 6 ([fd771f8](commit/fd771f816fd62a3817bc85ca1cb5d5db6770ab68))
- **deps:** update bincode requirement from 1.3 to 3.0 ([ebc42a5](commit/ebc42a56c6abbaaefd29c7853d1c4e30f3af5899))
- **deps:** update bincode requirement from 1.3 to 3.0 ([ee4118d](commit/ee4118d371e456bfb9ce327b1867dc72df2bc6a4))
- **deps:** update criterion requirement from 0.5 to 0.8 ([bff04f3](commit/bff04f32609872c2746eae84f14936eee2e8410b))
- **deps:** update criterion requirement from 0.5 to 0.8 ([4652f14](commit/4652f148ecc266ed2d6ad62e245d4b8f644fdae8))
- **deps:** update libp2p requirement from 0.53 to 0.56 ([734156b](commit/734156b780c85244f97ee01814b9cf5c39aa4224))
- **deps:** update libp2p requirement from 0.53 to 0.56 ([bc7165a](commit/bc7165a6f6f7e8eb210a25470b5dfb43430b58fb))
- **deps:** update rand requirement from 0.8 to 0.10 ([259659f](commit/259659fabace680f308f875023c80902eb1cbe11))
- **deps:** update rand requirement from 0.8 to 0.10 ([709914a](commit/709914a79ec2cc0185c5038aa6af54f01b99f53e))
- **deps:** update sha2 requirement from 0.10 to 0.11 ([0646dcd](commit/0646dcd70951ad63f4603e6f8a5fe00efeafbc48))
- **deps:** update sha2 requirement from 0.10 to 0.11 ([a53aa93](commit/a53aa935e01aa4455414017bfc2287fdd178a4ef))
- **deps:** update thiserror requirement from 1.0 to 2.0 ([2073b7b](commit/2073b7b9be610b7e7efef38b8da1c16829d835fd))
- **deps:** update thiserror requirement from 1.0 to 2.0 ([12a762e](commit/12a762eafa50ef94221d3334b258e96d6c559497))

### CI

- **release-please:** add workflow_dispatch trigger for manual re-runs ([fa80aef](commit/fa80aef19c14bcb82bcd43fe43f08c2606af8498))

## [0.1.0] - 2026-05-15

### Added

- substrate: CausalGraph with DAG storage, vector clock ordering, topological sort
- substrate: ConsensusEngine with BFT finality (Hashgraph + AlephBFT hybrid), VRF leader selection, round-based commit
- substrate: GossipProtocol with libp2p (QUIC, GossipSub, mDNS, request-response)
- substrate: Event with Ed25519 signatures, bincode serialization
- substrate: CRDTs: GCounter, OrSet, LWWRegister — all implement CvRDT trait
- substrate: VectorClock (BTreeMap<NodeId, u64>) with CvRDT merge for partition reconciliation
- substrate: SlashingEngine with equivocation (500pts), liveness (100pts), and invalid attestation (300pts) detection
- substrate: SledSlashingStore + InMemorySlashingStore for persistent slashing state
- substrate: SlashingUndoManager for governance-based reversal of slash decisions
- substrate: BLS12-381 signature aggregation (blst crate) for N-to-1 verification
- substrate: ThresholdKeyManager for t-of-n key sharing
- substrate: EncryptedKeyStore with rotation proofs
- substrate: Protocol version negotiation in P2P layer (VersionHandshake)
- substrate: State snapshot system (StateSnapshot::take/verify/serialize/deserialize)
- substrate: Event pruning by finalized round (CausalGraph::prune_finalized())
- substrate: Token-bucket rate limiter for event submission
- substrate: Crypto schemes abstraction (CryptoProfile with Hash/Signature/VRF/ZK schemes)
- shards: 6 domain shards — Financial, Computational, Physical, Biological, Identity, Economics
- shards: ShardRouter with automatic dispatch (implements EventProcessor trait)
- shards: Cross-shard messaging with causality verification (CrossShardMessage)
- shards: Replay protection via per-creator nonce tracking with sled persistence
- shards: Fee enforcement via FeeSchedule + QuotaSystem integration
- shards: Identity shard with DID (did:omnia:<hex>), Shamir recovery, biometric anchor, AI agent identity
- shards: Financial shard with causal account balances, transfer/mint/burn operations
- shards: Computational shard with task queue and proof registry
- shards: Physical shard with append-only provenance log
- shards: Biological shard with consent registry and ZK queries
- shards: Economics shard with UBC balances, epoch advancement, governance
- binding: RF Fingerprinting with PUF/RF-DNA spectral signatures (stub — needs SDR hardware)
- binding: Quantum-Resistant Commitments with Ed25519 + CRYSTALS-Dilithium hybrid signatures
- binding: Three-phase PQC migration: ClassicalOnly -> Hybrid -> PostQuantum
- binding: ProvenanceLog (append-only CRDT) with full lifecycle (Created -> Transferred -> Verified -> Destroyed)
- binding: PhysicalAnchor — unified verification of RF fingerprint + quantum commitment + provenance chain
- binding: PqcKeyRotationManager for post-quantum key rotation
- zk: Settlement-agnostic architecture (SettlementLayer trait)
- zk: Ethereum adapter with OmniaRollup.sol contract (deposit, withdrawal with 7-day challenge, batch submission)
- zk: Bitcoin, Solana, Celestia settlement adapters (stubs — return NotImplemented)
- zk: Groth16/Bn254 proof system with arkworks R1CS + Groth16
- zk: Poseidon SNARK-friendly hash function (BN254, t=3, R_F=8, R_P=57)
- zk: ExpandedRollupCircuit with Merkle path verification + per-event state transition constraints
- zk: PowersOfTau trusted setup ceremony (Phase 1) with multi-participant contributions
- zk: ProofBundle — chain-agnostic format with version, state roots, transition proof, L1 anchor
- zk: RollupOperator — collects finalized events, builds batch, generates Groth16 proof, settles on L1
- economics: UBC (Universal Basic Compute) — soulbound token, 1000 UBC/month default, non-transferable
- economics: QuotaSystem with 30-day epochs, register/spend/reward/balance_of/advance_epoch
- economics: Quadratic voting with reputation decay via PPM fixed-point arithmetic
- economics: Proof-of-Useful-Work with 3 types: AI Training, Scientific Simulation, Distributed Storage
- economics: TimeLockVoting for long-duration stake commitments
- economics: Fixed-point arithmetic (PPM) for cross-platform deterministic governance
- node: omnia-node binary with CLI (clap), health/metrics HTTP (axum), graceful shutdown
- node: REST API with events/shards/governance/economics/node endpoints + utoipa Swagger UI
- node: CLI subcommands: keygen, setup-contribute, setup-verify, snapshot, restore, run
- node: TOML configuration file support (NodeConfigFile, --config flag)
- node: --protocol-version CLI flag for advertising protocol version on the network
- node: Docker setup with multi-stage build, docker-compose for 5-node testnet
- chaos-tests: ChaosNetwork framework with partitions, crash recovery, byzantine, message loss
- chaos-tests: 4 integration test suites (partition.rs, crash_recovery.rs, byzantine.rs, message_loss.rs)
- fuzz: 11 libFuzzer targets with seed corpora and OSS-Fuzz Dockerfile
- formal-verification: TLA+ specification of consensus (Agreement, NoEquivocation, Validity invariants)
- formal-verification: TLA+ specification of CRDT convergence (GCounter, OrSet, LWWRegister)
- monitoring: Grafana dashboard with 9 panels + alert rules
- monitoring: Prometheus configuration + docker-compose integration
- CI: 5 GitHub Actions workflows (ci, benchmarks, chaos-tests, network-tests, nightly-fuzz)
- CI: Cross-platform testing (Ubuntu/macOS/Windows), cargo audit, cargo vet, SBOM generation, reproducible builds
- CD: release-please for automated versioning + changelog, release workflow for binary + contract + Docker publishing
- Supply chain: cargo-vet audits, CycloneDX SBOM, dependency policy

### Known Limitations

- Poseidon parameters use a Cauchy MDS matrix and BLAKE3-derived round constants (not the Grain LFSR from the paper)
- TLA+ CRDT model uses finite state spaces (3 nodes, MaxVal=3) — not exhaustive
- BLS key generation uses zero seed by default in tests — production must provide entropy
- EncryptedKeyStore uses XOR-based encryption (not AES-256-GCM) — production must upgrade
- RF fingerprinting is a stub (Hamming distance comparison) — needs SDR hardware
- Bitcoin/Solana/Celestia settlement adapters are stubs — return NotImplemented
- UsefulWorkProof verification is a stub (checks non-zero hash + positive compute units only)
- OmniaRollup.sol verifyProof is a Phase 0 stub (checks non-empty only)
- BiometricAnchor is a stub (BLAKE3 hash of salted template)
- sled 0.34 is alpha-quality — production deployments should migrate to rocksdb or redb

## [1.0.0-spec] - 2026-05-10

### Added

- Initial release of Omnia Protocol specification.
- Five-layer architecture definition.
- Causal graph consensus mechanism outline.
- Zero-Knowledge Proofs integration concept.
- Implementation roadmap (Phase 0 to Phase 3).
- Basic README.md, CONTRIBUTING.md, LICENSE, SECURITY.md.
- Initial diagrams for architecture, governance, supply chain, consensus comparison, and identity system.

---

🔙 **Back**: [README.md](./README.md) | 🔄 **Related**: [docs/reference/roadmap.md](./docs/reference/roadmap.md)  
🚀 **Next**: [docs/reference/roadmap.md](./docs/reference/roadmap.md) | 📜 **Source of Truth**: [Restructuring Blueprint](./docs/reference/blueprint-reference.md)
