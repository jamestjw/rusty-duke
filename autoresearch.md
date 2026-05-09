# Autoresearch: optimize Rusty Duke ISMCTS playing strength

## Objective
Improve the deterministic playing strength of the Rusty Duke ISMCTS/rollout engine on seeded Coup benchmark workloads. The workload tracks an ISMCTS bot using 60 iterations, depth 60, and Balanced heuristic rollouts in 4-player and 2-player games against random and heuristic opponents.

## Metrics
- **Primary**: strength_score (unitless, higher is better) — total tracked ISMCTS wins across 220 seeded benchmark games.
- **Secondary**: games, timeouts, decisions, bench_elapsed_ms — monitor workload shape and speed regressions.

## How to Run
`./autoresearch.sh` — fast-checks with `cargo check`, runs the release benchmark example, and outputs `METRIC name=value` lines.
Correctness backpressure: `autoresearch.checks.sh` runs `cargo test --quiet` after passing benchmark runs.

## Files in Scope
- `src/lib.rs` — core Coup game state, legal moves, observations, determinization.
- `src/engine/ismcts.rs` — ISMCTS search tree, selection, expansion, simulation, final move choice.
- `src/engine/rollout.rs` — random and heuristic rollout policies and move scoring.
- `src/engine/eval.rs` — non-terminal evaluation function used by search and rollouts.
- `src/engine/bot.rs` — bot wrappers for random, heuristic, and ISMCTS play.
- `src/engine/benchmark.rs` — seeded benchmark harness.
- `examples/autoresearch_bench.rs` — autoresearch benchmark driver and metrics only.
- `autoresearch.sh`, `autoresearch.checks.sh`, `autoresearch.md` — research harness and notes.

## Off Limits
- Do not change `Cargo.toml` dependencies unless absolutely necessary.
- Do not weaken or remove tests.
- Do not alter benchmark seeds/game counts to fake metric improvements; update only if more signal is needed and reinitialize the experiment.

## Constraints
- Tests must pass (`cargo test --quiet`).
- Preserve public API compatibility unless a change is clearly internal and tests cover it.
- Primary metric is deterministic strength, not runtime; large speed regressions should be noted but wins decide keep/discard.

## What's Been Tried
- Initial setup: benchmark aggregates 4-player vs Random, 4-player vs Balanced heuristic, and 2-player head-to-head vs Balanced heuristic.
