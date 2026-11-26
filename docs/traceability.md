# Traceability

Generated from `docs/traceability.toml` by `traceability-check --write`; do not edit.

| Requirement | Class | Scope | Properties | Artifacts that exist | Missing artifacts |
|---|---|---|---|---|---|
| R1.1 | Functional | deferred | 10 | 5 of 8 | `tests/diff/dist_vs_local/`, `crates/dp-core/tests/prop_assign.rs`, `crates/dp-formats/tests/prop_config.rs` |
| R1.2 | Performance | deferred | 7 | 2 of 6 | `crates/dp-core/src/escalation.rs`, `crates/dp-core/tests/prop_escalation.rs`, `tests/integration/fast_pass/`, `eval/scripts/rq7_escalation/` |
| R1.3 | Soundness-critical | in scope | 11 | 7 of 7 |  |
| R1.4 | Robustness | deferred | 6 | 3 of 5 | `verification/kani/calibration.rs`, `eval/scripts/rq6_instability/` |
| R1.5 | Liveness | deferred | 15 | 3 of 9 | `verification/tla/traces/`, `verification/kani/coordinator.rs`, `tests/integration/cluster/cancel.rs`, `tests/integration/cluster/deadline.rs`, `crates/dp-core/tests/prop_cancel.rs`, `crates/dp-core/tests/prop_record.rs` |
| R1.6 | Robustness | deferred | 15 | 2 of 5 | `tests/sim/faults/`, `crates/dp-core/tests/heartbeat_margin.rs`, `crates/dp-core/tests/prop_idempotent.rs` |
| R1.7 | Functional | deferred | 10 | 5 of 6 | `crates/dp-core/tests/prop_operator.rs` |
| R2.1 | Functional | deferred | 5 | 2 of 4 | `crates/dp-formats/tests/prop_config.rs`, `tests/integration/config/` |
| R2.2 | Performance | deferred | 19 | 3 of 8 | `crates/dp-core/src/race.rs`, `crates/dp-core/tests/prop_race.rs`, `tests/sim/faults/`, `tests/integration/config/`, `eval/scripts/rq2_portfolio/` |
| R2.3 | Functional | in scope | 6 | 4 of 4 |  |
| R3.1 | Performance | deferred | 15 | 0 of 9 | `verification/tla/Sharing.tla`, `crates/dp-core/tests/prop_forward.rs`, `crates/dp-core/tests/prop_pending.rs`, `tests/diff/sharing_on_off/`, `tests/sim/faults/`, `tests/sim/hostile_pool.rs`, `tests/sim/two_job_default_deps.rs`, `tests/sim/forwarding.rs`, `eval/scripts/rq3_within_goal/` |
| R3.2 | Soundness-critical | deferred | 9 | 4 of 15 | `crates/dp-core/src/provenance.rs`, `proofs/lean/DP/Admission.lean`, `crates/dp-core/src/compose_plan.rs`, `crates/dp-core/src/admission.rs`, `crates/dp-cert/src/check.rs`, `fuzz/provenance/`, `crates/dp-core/tests/prop_provenance.rs`, `crates/dp-core/tests/prop_admission.rs`, `crates/dp-cert/tests/prop_compose.rs`, `crates/dp-pool/tests/prop_pool.rs`, `tests/sim/lying_provenance.rs` |
| R3.3 | Soundness-critical | deferred | 6 | 3 of 9 | `crates/dp-core/src/admission.rs`, `proofs/lean/DP/Admission.lean`, `crates/dp-cert/src/check.rs`, `verification/kani/parsers.rs`, `crates/dp-core/tests/prop_hygiene.rs`, `fuzz/lemma_parser/` |
| R3.4 | Soundness-critical | deferred | 13 | 3 of 10 | `proofs/lean/DP/Admission.lean`, `proofs/lean/DP/ModelSanity.lean`, `crates/dp-core/src/admission.rs`, `crates/dp-cert/src/check.rs`, `tests/diff/admission_lean_vs_rust/`, `crates/dp-core/tests/prop_admission.rs`, `tests/sim/lying_provenance.rs` |
| R3.5 | Soundness-critical | deferred | 10 | 5 of 11 | `proofs/lean/DP/Partition.lean`, `crates/dp-cert/src/partition.rs`, `crates/dp-core/src/race.rs`, `crates/dp-cert/tests/prop_partition.rs`, `tests/canaries/partition/`, `crates/dp-core/tests/prop_race.rs` |
| R3.6 | Robustness | deferred | 6 | 0 of 5 | `verification/kani/pool.rs`, `fuzz/lemma_pool_ingest/`, `crates/dp-pool/tests/prop_pool.rs`, `tests/sim/hostile_pool.rs`, `tests/sim/faults/` |
| R4.1 | Soundness-critical | deferred | 20 | 3 of 14 | `proofs/lean/DP/Canon.lean`, `crates/dp-core/src/admission.rs`, `crates/dp-cert/src/check.rs`, `crates/dp-canon/tests/`, `crates/dp-worker/tests/prop_rename.rs`, `tests/diff/canon_equisat/`, `tests/diff/canon_determinism/`, `tests/diff/canon_lean_vs_rust/`, `tests/diff/solver_names/`, `fuzz/canonicalizer/`, `tests/sim/lying_provenance.rs` |
| R4.2 | Performance | deferred | 8 | 1 of 8 | `verification/tla/Sharing.tla`, `proofs/lean/DP/Admission.lean`, `crates/dp-core/src/admission.rs`, `crates/dp-core/tests/prop_scope.rs`, `tests/diff/cross_goal_on_off/`, `tests/integration/config/`, `eval/scripts/rq4_cross_goal/` |
| R4.3 | Soundness-critical | deferred | 6 | 3 of 8 | `proofs/lean/DP/Admission.lean`, `crates/dp-core/src/admission.rs`, `crates/dp-cert/src/check.rs`, `crates/dp-core/tests/prop_ground.rs`, `tests/diff/cross_goal_on_off/` |
| R4.4 | Soundness-critical | deferred | 13 | 5 of 15 | `proofs/lean/DP/Hints.lean`, `crates/dp-core/src/hints.rs`, `crates/dp-cert/src/check.rs`, `verification/kani/parsers.rs`, `crates/dp-core/tests/prop_hints.rs`, `tests/diff/hints_lean_vs_rust/`, `tests/diff/hints_on_off/`, `fuzz/hint_parser/`, `tests/sim/lying_provenance.rs`, `eval/scripts/rq4_cross_goal/` |
| R5.1 | Soundness-critical | deferred | 12 | 4 of 8 | `crates/dp-core/src/certification.rs`, `crates/dp-cert/src/check.rs`, `crates/dp-core/tests/prop_certification.rs`, `eval/scripts/rq5_certificates/` |
| R5.2 | Soundness-critical | deferred | 15 | 4 of 14 | `proofs/lean/DP/Compose.lean`, `crates/dp-core/src/compose_plan.rs`, `crates/dp-core/src/fragment.rs`, `crates/dp-cert/src/check.rs`, `crates/dp-cert/tests/prop_compose.rs`, `tests/diff/compose_lean_vs_rust/`, `crates/dp-cert/tests/prop_partition.rs`, `crates/dp-core/tests/prop_fragment.rs`, `fuzz/cpc_proof/`, `fuzz/alethe_proof/` |
| R5.3 | Soundness-critical | deferred | 13 | 8 of 10 | `crates/dp-cert/src/check.rs`, `tests/certs/tamper/` |
| R5.4 | Functional | deferred | 8 | 5 of 7 | `crates/dp-cert/tests/prop_store.rs`, `tests/integration/certs/store_faults.rs` |
| R5.5 | Soundness-critical | deferred | 7 | 4 of 8 | `crates/dp-cert/src/check.rs`, `tests/diff/certify_all_on_off/`, `tests/integration/config/`, `eval/scripts/rq5_certificates/` |
| R6.1 | Functional | deferred | 7 | 1 of 6 | `eval/corpus/tests/`, `tests/diff/export_vs_why3find/`, `fuzz/corpus_metadata/`, `eval/scripts/corpus_hardness/`, `eval/rq/corpus.toml` |
| R6.2 | Performance | deferred | 11 | 0 of 11 | `eval/scripts/rq1_distribution/`, `eval/scripts/rq2_portfolio/`, `eval/scripts/rq3_within_goal/`, `eval/scripts/rq4_cross_goal/`, `eval/scripts/rq5_certificates/`, `eval/scripts/rq6_instability/`, `eval/scripts/rq7_escalation/`, `eval/rq/`, `eval/rq/corpus.toml`, `eval/tests/`, `eval/tests/metrics/` |
| R6.3 | Performance | deferred | 13 | 3 of 6 | `eval/tests/`, `eval/report/check/`, `eval/tests/canary_gate/` |
| R6.4 | Performance | deferred | 11 | 0 of 3 | `eval/report/`, `eval/report/check/`, `eval/tests/report_check/` |
| R7.1 | Soundness-critical | deferred | 8 | 9 of 12 | `proofs/lean/DP/Certifiable.lean`, `crates/dp-cert/src/check.rs`, `tests/sim/lying_provenance.rs` |
| R7.2 | Soundness-critical | in scope | 9 | 10 of 10 |  |
| R7.3 | Process | deferred | 4 | 5 of 6 | `verification/kani/` |
| R7.4 | Soundness-critical | deferred | 10 | 4 of 7 | `tests/canaries/witness.rs`, `tests/certs/tamper/`, `eval/tests/canary_gate/` |
| R7.5 | Soundness-critical | deferred | 10 | 7 of 9 | `verification/kani/cache_key.rs`, `crates/dp-cert/src/check.rs` |
| R7.6 | Soundness-critical | deferred | 8 | 4 of 5 | `eval/scripts/rq6_instability/` |
| R7.7 | Process | deferred | 6 | 3 of 5 | `tools/patch-check/`, `tests/integration/pins/` |
| R7.8 | Process | in scope | 6 | 4 of 4 |  |
| R7.9 | Process | deferred | 2 | 0 of 1 | `tools/patch-check/` |
| R7.10 | Soundness-critical | deferred | 9 | 4 of 9 | `crates/dp-formats/src/canonical_check.rs`, `crates/dp-cert/src/check.rs`, `verification/kani/parsers.rs`, `crates/dp-canon/tests/`, `tests/diff/smtlib_parser/` |
| R7.11 | Process | in scope | 11 | 2 of 2 |  |
| R7.12 | Functional | in scope | 3 | 1 of 1 |  |
| R7.13 | Process | in scope | 6 | 5 of 5 |  |
