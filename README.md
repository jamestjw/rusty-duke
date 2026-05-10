# Rusty Duke

A Coup game engine and AI bot in Rust. Implements the full game rules (2–6 players),
partial-information observations, and multiple AI decision-makers from random play to
Information Set Monte Carlo Tree Search (ISMCTS).

## Features

- **Complete rules engine** — income, foreign aid, tax, assassinate, steal, exchange,
  coup, challenges, blocks, influence loss, and exchange returns
- **Partial-information model** — `Observation` encodes only what one player can see;
  `determinize` samples a consistent full state from an information set
- **Multiple bots** — `RandomBot`, `HeuristicBot` (6 personality profiles),
  `IsmctsBot` (configurable search)
- **Deterministic with seeds** — all randomness is seeded; same seed + same
  observation = same move
- **Benchmark harness** — self-play and head-to-head with configurable opponents,
  outputs win rates and timing

## Architecture

```
             Engine                   Search
          ┌──────────┐          ┌───────────────────┐
          │GameState │◄───obs───│   IsmctsBot       │
          │          │──move──► │  ┌──────────────┐ │
          │ legal()  │          │  │ SearchTree   │ │
          │ apply()  │          │  │  Node/Edge   │ │
          │ observ() │          │  │  UCB select  │ │
          └──────────┘          │  └──────────────┘ │
               ▲                │  ┌──────────────┐ │
               │                │  │  Rollout     │ │
          ┌────┴──────┐         │  │  (random or  │ │
          │Observation│         │  │   heuristic) │ │
          │ (public + │         │  └──────────────┘ │
          │  own hand)│         └───────────────────┘
          └───────────┘
```

The project has two layers:

### 1. Game Engine (`src/lib.rs`)

The core Coup rules. The key type is `GameState` which owns the full truth state
(all players' hidden cards, deck, current phase). Public API:

| Method | Purpose |
|---|---|
| `GameState::new(n, seed)` | Create a new game with `n` players |
| `state.legal_moves(player)` | All legal moves for a player at the current phase |
| `state.apply_move(player, move)` | Execute a move, transitioning state |
| `state.observation_for(player)` | What `player` can see (their hand, coin counts, revealed cards) |
| `state.active_player()` | Who needs to act now |
| `state.winner()` / `is_terminal()` | Game-over detection |

`Observation` encodes only the information visible to one player — opponent cards
are hidden, replaced by a count. `GameState::determinize(&observation, seed)` samples
a full state consistent with the observation, which is how bots reason about hidden
information without cheating.

The `Phase` enum tracks the game's state machine through 7 phases:

- `AwaitingAction` — active player chooses a move
- `AwaitingChallenge` — others may challenge a claimed action
- `AwaitingBlock` — target may block (foreign aid / assassinate / steal)
- `AwaitingBlockChallenge` — action-owner may challenge a block
- `AwaitingInfluenceLoss` — a player must reveal a card
- `AwaitingExchangeReturn` — exchanger picks cards to keep
- `Terminal` — game over

### 2. AI Engine (`src/engine/`)

#### Bots

All bots implement the `Bot` trait:

```rust
pub trait Bot {
    fn choose_move<R: Rng + ?Sized>(
        &mut self,
        observation: &Observation,
        rng: &mut R,
    ) -> Option<Move>;
}
```

**`RandomBot`** — determinizes the observation, picks a legal move uniformly at random.

**`HeuristicBot`** — scores each legal move using a hand-tuned heuristic with
+/- jitter for variety. Six profiles:

| Profile | Behavior |
|---|---|
| `Balanced` | Default equal-weight play |
| `Aggressive` | Favours coup, assassinate, steal |
| `Conservative` | Prefers income, exchange, pass on challenges |
| `Economic` | Favours tax, foreign aid, income |
| `ChallengeHeavy` | Challenges claims aggressively |
| `BlockHeavy` | Blocks aggressively (even when bluffing) |

**`IsmctsBot`** — Information Set Monte Carlo Tree Search. Each iteration:

1. Samples a determinization from the current observation (plausible hidden cards)
2. Traverses the search tree using UCB1 with availability heuristic
3. Expands one unvisited legal move
4. Rollouts to terminal or depth limit (random or heuristic)
5. Backpropagates the result (`1.0` for win, `0.0` for loss, or a heuristic score)

After all iterations, picks the move with the highest total reward across visits.

Configurable via `SearchConfig`:

| Parameter | Default | Effect |
|---|---|---|
| `iterations` | 1,000 | More = stronger, linearly slower |
| `max_depth` | 80 | Rollout depth limit |
| `exploration` | 1.4 | UCB exploration constant |
| `rollout_policy` | `Random` | `Random` or `Heuristic(profile)` |

#### Evaluation (`src/engine/eval.rs`)

Scores a non-terminal state for the root player:
- Terminal: 1.0 (win) or 0.0 (loss)
- Otherwise: material comparison of (influence × 10 + coins + coup pressure)
  between root and average living opponent, scaled to 0–1

#### Rollouts (`src/engine/rollout.rs`)

Plays out a game from a given state to a depth limit. Two policies:

- **Random** — picks uniformly; fast but noisy
- **Heuristic** — scores each move using Coup-aware heuristics (truth likelihood,
  target value, challenge risk, block value, personality bonuses)

## Usage

### Library

```rust
use rusty_duke::*;
use rusty_duke::engine::*;

// Create a game
let mut game = GameState::new(3, 42).unwrap();

// Get a player's view
let obs = game.observation_for(0).unwrap();

// Pick a move with ISMCTS
let mut bot = IsmctsBot::new(SearchConfig {
    iterations: 500,
    max_depth: 80,
    exploration: 1.4,
    rollout_policy: RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
});
let mut rng = rand::rngs::StdRng::seed_from_u64(99);
let mv = bot.choose_move(&obs, &mut rng).unwrap();

// Apply it
game.apply_move(0, mv).unwrap();
```

### Benchmarking

The built-in benchmark harness runs seeded self-play and reports win rates:

```rust
use rusty_duke::engine::*;

let result = benchmark_ismcts_vs_random(BenchmarkConfig {
    games: 100,
    player_count: 3,
    ismcts_iterations: 200,
    ..Default::default()
});
println!("Win rate: {:.2}", result.ismcts_win_rate());
```

Run the ignored benchmark tests:

```sh
cargo test ismcts_iteration_benchmark -- --ignored --nocapture
cargo test round_robin_benchmark -- --ignored --nocapture
```

### Tests

```sh
cargo test
```

All game logic and AI decisions are deterministic with fixed seeds.

## Design Decisions

- **Observation-based API**, not direct state access — bots only see what a real
  player would see, preventing information leakage
- **Determinize per iteration** rather than once per decision — each ISMCTS
  iteration samples a new hidden-card assignment, reducing strategy fusion artifacts
- **Availability tracking in UCB** — not all moves are legal in every determinization;
  availability counters prevent UCB from overvaluing moves that rarely appear
- **Material-score evaluation** — simple but sufficient; terminal rewards dominate
- **RNG passed explicitly** — no global state, making all search deterministic with
  a fixed seed
