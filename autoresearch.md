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
- Initial setup: benchmark aggregates 4-player vs Random, 4-player vs Balanced heuristic, and 2-player head-to-head vs Balanced heuristic. Baseline strength_score=80 (23/80 4p random, 18/60 4p heuristic, 39/80 h2h).
- Kept: evaluation now compares root material to the average living opponent instead of fixed opponent_sum/2. Raised score to 96, mostly from 4p random and h2h.
- Kept: evaluation adds coup-pressure bonuses at 7+ and 10+ coins. Raised score to 99 and greatly reduced total decisions/timeouts.
- Discarded: hidden influence material weights of 12 and 8 both hurt badly. Keep hidden_count weight at 10.
- Discarded: coup-pressure bonus sizes 7/12 and 3/5 underperformed current 5/8.
- Discarded: changing final ISMCTS best child from mean reward to visit count hurt aggregate strength.
- Discarded: heuristic rollout Coup base 100 and 70 both underperformed base 80; 70 helped h2h but hurt multiplayer.
- Discarded: changing heuristic jitter from 0..4 to none, 0..8, or 0..2 hurt aggregate; keep 0..4.
