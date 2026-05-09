# ISMCTS Coup Engine Architecture

This document outlines a practical plan for building a Coup engine using Information Set Monte Carlo Tree Search (ISMCTS).

## Goal

Build an engine that chooses strong Coup actions under imperfect information by searching over possible hidden card assignments consistent with the public game history.

The engine should eventually support:

- Hidden-information reasoning.
- Bluffing and challenge decisions.
- Blocking decisions.
- Opponent modeling.
- Configurable search strength.
- Deterministic testing through seeded randomness.

## Why ISMCTS

Coup is a good fit for ISMCTS because:

- Players have hidden influence cards.
- Public action history constrains but does not reveal the full game state.
- The action space is relatively small.
- Games are short enough for many simulations.
- Bluffing can be represented through sampled hidden states and opponent policies.

Plain MCTS is not sufficient because it assumes a fully known state. Naive determinized MCTS can also leak hidden information into future decisions. ISMCTS reduces this problem by searching from the current player's information set instead of a single known state.

## Core Concepts

### Public State

The public state contains everything all players can observe:

- Player coin counts.
- Number of remaining influence cards per player.
- Revealed dead cards.
- Current turn player.
- Current action or reaction phase.
- Public action history.
- Game outcome, if terminal.

This state should not contain unrevealed cards.

### Private State

The private state contains information known only to a specific player:

- That player's unrevealed influence cards.
- Any sampled hidden cards used during a simulation.

The real game state may contain all hidden cards internally, but the engine must not use those cards directly when choosing an action.

### Information Set

An information set represents all full game states that are possible from the current player's perspective.

For a player, it is defined by:

- The public state.
- The player's own hidden cards.
- The set of possible opponent card assignments consistent with known cards and revealed cards.

### Determinization

A determinization is one sampled full game state from the information set.

Each ISMCTS iteration should:

1. Sample one possible hidden card assignment.
2. Run tree selection and simulation using that sampled state.
3. Backpropagate the result into information-set tree nodes.

## High-Level Architecture

```text
Game Engine
  - Rules
  - State transitions
  - Legal action generation
  - Terminal detection

Information Model
  - Public state extraction
  - Hidden-state sampler
  - Belief weighting
  - Card consistency checks

ISMCTS Search
  - Search nodes keyed by information-set observations
  - Selection policy
  - Expansion
  - Rollout policy
  - Backpropagation

Policies
  - Search policy
  - Rollout policy
  - Opponent reaction policy
  - Heuristic evaluation

Engine Interface
  - choose_action
  - choose_challenge_response
  - choose_block_response
  - configuration and seeded RNG
```

## Recommended Rust Modules

The exact module names can evolve, but this is a useful starting structure:

```text
src/
  game/
    mod.rs
    card.rs
    action.rs
    state.rs
    rules.rs
  engine/
    mod.rs
    ismcts.rs
    belief.rs
    rollout.rs
    eval.rs
    opponent.rs
  tests/
```

## Tentative Rust Code Architecture

The code should keep the rules engine independent from the AI engine. The `game` modules should know nothing about ISMCTS, while the `engine` modules should call into the rules engine through stable state-transition and legal-action APIs.

### Top-Level Module Layout

```rust
// src/lib.rs
pub mod game;
pub mod engine;
```

```rust
// src/game/mod.rs
pub mod action;
pub mod card;
pub mod player;
pub mod rules;
pub mod state;

pub use action::*;
pub use card::*;
pub use player::*;
pub use rules::*;
pub use state::*;
```

```rust
// src/engine/mod.rs
pub mod belief;
pub mod eval;
pub mod ismcts;
pub mod opponent;
pub mod rollout;

pub use ismcts::{IsmctsEngine, SearchConfig};
```

### Game Types

```rust
// src/game/card.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Card {
    Duke,
    Assassin,
    Captain,
    Ambassador,
    Contessa,
}

pub const COPIES_PER_CARD: usize = 3;

pub fn full_deck() -> Vec<Card> {
    use Card::*;
    [Duke, Assassin, Captain, Ambassador, Contessa]
        .into_iter()
        .flat_map(|card| std::iter::repeat(card).take(COPIES_PER_CARD))
        .collect()
}
```

```rust
// src/game/player.rs
pub type PlayerId = usize;

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub coins: u8,
    pub revealed: Vec<Card>,
    pub influence_count: u8,
}

#[derive(Debug, Clone)]
pub struct PrivatePlayerState {
    pub player: PlayerState,
    pub hidden: Vec<Card>,
}
```

`PlayerState` is public-safe. `PrivatePlayerState` is only used inside a full internal state or a sampled determinization.

```rust
// src/game/action.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Claim {
    Duke,
    Assassin,
    Captain,
    Ambassador,
    Contessa,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    Income,
    ForeignAid,
    Coup { target: PlayerId },
    Tax,
    Assassinate { target: PlayerId },
    Exchange,
    Steal { target: PlayerId },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Reaction {
    Pass,
    Challenge,
    Block { claim: Claim },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Decision {
    Action(Action),
    Reaction(Reaction),
    Reveal { card_index: usize },
    ExchangeReturn { card_indices: Vec<usize> },
}
```

`Decision` is the type the rules engine consumes at any decision point. Keeping action, reaction, reveal, and exchange decisions under one enum makes ISMCTS easier because the tree can reason over one move type.

### State And Phase Model

```rust
// src/game/state.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Phase {
    AwaitingAction {
        actor: PlayerId,
    },
    AwaitingChallenge {
        actor: PlayerId,
        action: Action,
        next_responder: PlayerId,
    },
    AwaitingBlock {
        actor: PlayerId,
        action: Action,
        target: Option<PlayerId>,
        next_responder: PlayerId,
    },
    AwaitingBlockChallenge {
        blocker: PlayerId,
        claim: Claim,
        action: Action,
        next_responder: PlayerId,
    },
    AwaitingReveal {
        player: PlayerId,
        reason: RevealReason,
    },
    AwaitingExchangeReturn {
        player: PlayerId,
        hand_size: usize,
    },
    Terminal {
        winner: PlayerId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RevealReason {
    LostChallenge,
    FailedAssassinationBlock,
    Couped,
    Assassinated,
}

#[derive(Debug, Clone)]
pub struct GameState {
    pub players: Vec<PrivatePlayerState>,
    pub deck: Vec<Card>,
    pub discard: Vec<Card>,
    pub phase: Phase,
    pub turn: u32,
    pub history: Vec<PublicEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PublicState {
    pub players: Vec<PlayerState>,
    pub discard: Vec<Card>,
    pub phase: Phase,
    pub turn: u32,
    pub history: Vec<PublicEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PublicEvent {
    ActionDeclared { actor: PlayerId, action: Action },
    Challenge { challenger: PlayerId },
    BlockDeclared { blocker: PlayerId, claim: Claim },
    CardRevealed { player: PlayerId, card: Card },
    PlayerEliminated { player: PlayerId },
}
```

The `PublicState` should be hashable because ISMCTS nodes need stable information-set keys. If the full history becomes too large, replace it with a compact summary or fixed-size claim history.

### Player View

```rust
// src/game/state.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerView {
    pub player_id: PlayerId,
    pub public: PublicState,
    pub hidden: Vec<Card>,
}

impl GameState {
    pub fn public_state(&self) -> PublicState {
        todo!()
    }

    pub fn player_view(&self, player_id: PlayerId) -> PlayerView {
        todo!()
    }
}
```

Real action selection should receive `PlayerView`, not `GameState`, to prevent accidental hidden-information leakage.

### Rules API

```rust
// src/game/rules.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    IllegalDecision,
    WrongPlayer,
    InvalidTarget,
    GameAlreadyOver,
}

pub struct Rules;

impl Rules {
    pub fn legal_decisions(state: &GameState, player_id: PlayerId) -> Vec<Decision> {
        todo!()
    }

    pub fn apply_decision<R: rand::Rng + ?Sized>(
        state: &mut GameState,
        player_id: PlayerId,
        decision: Decision,
        rng: &mut R,
    ) -> Result<(), RuleError> {
        todo!()
    }

    pub fn is_terminal(state: &GameState) -> bool {
        matches!(state.phase, Phase::Terminal { .. })
    }

    pub fn winner(state: &GameState) -> Option<PlayerId> {
        match state.phase {
            Phase::Terminal { winner } => Some(winner),
            _ => None,
        }
    }
}
```

The rules API should be deterministic except where an explicit RNG is passed. That makes tests and search reproducible.

### Belief And Sampling

```rust
// src/engine/belief.rs
use crate::game::{GameState, PlayerId, PlayerView};

#[derive(Debug, Clone)]
pub struct BeliefModel {
    pub use_weighted_claims: bool,
}

impl BeliefModel {
    pub fn sample_state<R: rand::Rng + ?Sized>(
        &self,
        view: &PlayerView,
        rng: &mut R,
    ) -> GameState {
        todo!()
    }

    pub fn sample_many<R: rand::Rng + ?Sized>(
        &self,
        view: &PlayerView,
        count: usize,
        rng: &mut R,
    ) -> Vec<GameState> {
        (0..count).map(|_| self.sample_state(view, rng)).collect()
    }
}
```

The first implementation can sample uniformly. Weighted sampling can be added without changing the ISMCTS interface.

### Search Engine API

```rust
// src/engine/ismcts.rs
use std::collections::HashMap;

use crate::engine::belief::BeliefModel;
use crate::game::{Decision, GameState, PlayerId, PlayerView, PublicState};

#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub iterations: usize,
    pub max_depth: usize,
    pub exploration: f64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            iterations: 1_000,
            max_depth: 80,
            exploration: 1.4,
        }
    }
}

pub struct IsmctsEngine {
    pub config: SearchConfig,
    pub belief: BeliefModel,
}

impl IsmctsEngine {
    pub fn choose_decision<R: rand::Rng + ?Sized>(
        &self,
        view: &PlayerView,
        rng: &mut R,
    ) -> Option<Decision> {
        todo!()
    }
}
```

Use `choose_decision` rather than `choose_action` so the same engine can eventually handle actions, challenges, blocks, reveals, and exchange returns.

### ISMCTS Tree Structures

```rust
// src/engine/ismcts.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InfoSetKey {
    root_player: PlayerId,
    public: PublicState,
    private_cards: Vec<crate::game::Card>,
}

#[derive(Debug, Default)]
struct SearchTree {
    nodes: HashMap<InfoSetKey, Node>,
}

#[derive(Debug, Default)]
struct Node {
    visits: u32,
    total_reward: f64,
    children: HashMap<Decision, Edge>,
}

#[derive(Debug, Default)]
struct Edge {
    visits: u32,
    available: u32,
    total_reward: f64,
}
```

Tracking `available` is useful in ISMCTS because not every action is available in every determinization. UCB can use edge visits and availability to avoid overvaluing actions that appear in fewer sampled states.

### Selection And Rollout Shape

```rust
// src/engine/ismcts.rs
impl IsmctsEngine {
    fn run_iteration<R: rand::Rng + ?Sized>(
        &self,
        tree: &mut SearchTree,
        root_view: &PlayerView,
        root_player: PlayerId,
        rng: &mut R,
    ) {
        let mut state = self.belief.sample_state(root_view, rng);
        let reward = self.simulate(tree, &mut state, root_player, 0, rng);
        // Backpropagation can be done recursively inside simulate or by storing a path.
        let _ = reward;
    }

    fn simulate<R: rand::Rng + ?Sized>(
        &self,
        tree: &mut SearchTree,
        state: &mut GameState,
        root_player: PlayerId,
        depth: usize,
        rng: &mut R,
    ) -> f64 {
        todo!()
    }

    fn select_ucb(&self, node: &Node, legal: &[Decision]) -> Decision {
        todo!()
    }
}
```

The implementation can start with a path-based loop instead of recursion if that is easier to reason about in Rust.

### Rollout Policy

```rust
// src/engine/rollout.rs
use crate::game::{Decision, GameState, PlayerId};

pub trait RolloutPolicy {
    fn choose_rollout_decision<R: rand::Rng + ?Sized>(
        &self,
        state: &GameState,
        player_id: PlayerId,
        legal: &[Decision],
        rng: &mut R,
    ) -> Decision;
}

#[derive(Debug, Clone, Default)]
pub struct HeuristicRolloutPolicy;

impl RolloutPolicy for HeuristicRolloutPolicy {
    fn choose_rollout_decision<R: rand::Rng + ?Sized>(
        &self,
        state: &GameState,
        player_id: PlayerId,
        legal: &[Decision],
        rng: &mut R,
    ) -> Decision {
        todo!()
    }
}
```

A trait keeps rollout behavior swappable without changing the search engine.

### Evaluation

```rust
// src/engine/eval.rs
use crate::game::{GameState, PlayerId, Rules};

pub fn evaluate(state: &GameState, root_player: PlayerId) -> f64 {
    if let Some(winner) = Rules::winner(state) {
        return if winner == root_player { 1.0 } else { 0.0 };
    }

    let root = &state.players[root_player];
    let root_score = root.player.influence_count as f64 * 10.0 + root.player.coins as f64;

    let opponent_score: f64 = state
        .players
        .iter()
        .enumerate()
        .filter(|(id, _)| *id != root_player)
        .map(|(_, player)| player.player.influence_count as f64 * 10.0 + player.player.coins as f64)
        .sum();

    0.5 + (root_score - opponent_score / 2.0) / 100.0
}
```

Keep evaluation simple at first. The terminal result matters more than perfect depth-limit scoring in the first implementation.

### Opponent Policy

```rust
// src/engine/opponent.rs
#[derive(Debug, Clone)]
pub struct OpponentModel {
    pub bluff_rate: f64,
    pub challenge_rate: f64,
    pub block_rate: f64,
}

impl Default for OpponentModel {
    fn default() -> Self {
        Self {
            bluff_rate: 0.25,
            challenge_rate: 0.15,
            block_rate: 0.35,
        }
    }
}
```

This should initially influence rollouts and belief weighting. Later it can become per-player and update from observed public events.

### Public Engine Interface

For callers, the engine should expose one simple decision API:

```rust
pub trait CoupBot {
    fn choose_decision<R: rand::Rng + ?Sized>(
        &mut self,
        view: &PlayerView,
        legal: &[Decision],
        rng: &mut R,
    ) -> Decision;
}
```

Then ISMCTS can implement the same interface as simpler baseline bots:

```rust
impl CoupBot for IsmctsEngine {
    fn choose_decision<R: rand::Rng + ?Sized>(
        &mut self,
        view: &PlayerView,
        legal: &[Decision],
        rng: &mut R,
    ) -> Decision {
        self.choose_decision(view, rng)
            .filter(|decision| legal.contains(decision))
            .unwrap_or_else(|| legal[0].clone())
    }
}
```

This makes self-play and benchmarking easier because random, heuristic, and ISMCTS bots can all share the same interface.

### `game::card`

Defines Coup cards:

- Duke
- Assassin
- Captain
- Ambassador
- Contessa

Also define deck composition and helper functions for card counts.

### `game::action`

Defines all player-facing actions and reactions:

- Income
- Foreign aid
- Coup
- Tax
- Assassinate
- Exchange
- Steal
- Challenge
- Block
- Pass

Actions should include enough data to apply them, such as target player IDs.

### `game::state`

Defines the full internal game state and public projection.

Important types:

- `GameState`: complete state used by the rules engine.
- `PublicState`: observable state used by the search engine.
- `PlayerState`: coins, revealed cards, and live influence count.
- `Phase`: turn phase, action phase, challenge phase, block phase, or terminal.

### `game::rules`

Owns legal move generation and state transitions.

Responsibilities:

- Generate legal actions for the active decision-maker.
- Apply actions to a `GameState`.
- Resolve challenges.
- Resolve blocks.
- Resolve card reveal and replacement.
- Detect terminal states.

This module should be deterministic and side-effect free except for explicit RNG passed into functions that need randomness.

### `engine::belief`

Maintains and samples possible hidden card assignments.

Initial version:

- Sample uniformly from all card assignments consistent with known cards.
- Exclude revealed dead cards.
- Include the current player's own hidden cards.

Later improvements:

- Weight samples based on action claims.
- Increase probability that repeated Duke claimers hold Duke.
- Decrease probability when players decline obvious challenges.
- Track player-specific bluff tendencies.

### `engine::ismcts`

Owns the search algorithm.

Main entry point:

```rust
choose_action(public_state, private_view, config, rng) -> Action
```

Core loop:

1. Sample a determinization from the information set.
2. Traverse the tree using UCB or another bandit policy.
3. Expand one unvisited legal action.
4. Roll out to terminal state or depth limit.
5. Score the result for the root player.
6. Backpropagate the score.

Node data:

- Visit count.
- Total reward.
- Available action count, if using ISMCTS availability tracking.
- Child map keyed by action.

### `engine::rollout`

Provides simulation policies after tree expansion.

Start simple, but avoid purely random play if possible.

Basic rollout priorities:

- Coup when required or strategically strong.
- Take safe income when low risk.
- Prefer Tax if claiming Duke is plausible.
- Prefer Assassinate when target has one influence and attacker has enough coins.
- Prefer blocking actions that are plausible and valuable.
- Challenge rarely unless the claim is unlikely or high-impact.

### `engine::eval`

Scores non-terminal states when a rollout hits a depth limit.

Useful features:

- Current player alive or eliminated.
- Influence count.
- Coin count.
- Opponent influence counts.
- Opponent coin threats.
- Ability to force coup soon.
- Revealed card distribution.

Terminal states should dominate heuristic scores.

### `engine::opponent`

Models opponent behavior during search and rollout.

Initial version:

- Rule-based default policy.
- Configurable bluff, challenge, and block probabilities.

Later version:

- Per-player statistics.
- Bayesian update from observed claims.
- Different personality profiles.

## Search Configuration

Use a configuration object for tuning:

```text
iterations: number of ISMCTS iterations per decision
max_depth: rollout depth limit
exploration: UCB exploration constant
rollout_policy: random, heuristic, or mixed
belief_policy: uniform or weighted
seed: optional deterministic RNG seed
```

Start with small defaults for development:

```text
iterations = 1_000
max_depth = 80
exploration = 1.4
```

Increase iterations once the rules and tests are stable.

## Decision Types

Coup has multiple decision points, not only turn actions. The engine should support each explicitly.

### Active Turn Action

Choose the action for the current player's turn:

- Income
- Foreign aid
- Coup
- Tax
- Assassinate
- Exchange
- Steal

### Challenge Decision

Choose whether to challenge another player's claimed action or block.

This decision should consider:

- Probability the claim is false.
- Value of successful challenge.
- Cost of failed challenge.
- Current player's influence count.
- Whether the action is dangerous enough to contest.

### Block Decision

Choose whether to block an action targeting the player or table.

This decision should consider:

- Whether the player likely has the blocking card.
- Whether bluffing a block is worth the challenge risk.
- Impact of allowing the action.

### Card Reveal Decision

When forced to lose influence, choose which card to reveal.

This can often be heuristic-driven:

- Preserve cards with stronger future utility.
- Preserve cards that support useful blocks.
- Preserve cards that support current table image.

## Implementation Phases

### Phase 1: Complete Rules Engine

Build and test the game rules before search.

Deliverables:

- Card and action types.
- Full game state.
- Legal action generation.
- State transition logic.
- Challenge and block resolution.
- Terminal detection.
- Unit tests for every action.

### Phase 2: Uniform Hidden-State Sampling

Add information-set sampling.

Deliverables:

- Public state projection.
- Private player view.
- Sampler for full states consistent with public information.
- Tests proving sampled states never violate known card counts.

### Phase 3: Basic ISMCTS

Add the first working search engine.

Deliverables:

- ISMCTS node structure.
- Selection, expansion, simulation, and backpropagation.
- Random or simple heuristic rollouts.
- Configurable iteration count.
- Seeded deterministic tests.

### Phase 4: Heuristic Rollouts

Replace mostly random rollouts with Coup-aware behavior.

Deliverables:

- Action-priority rollout policy.
- Basic challenge policy.
- Basic block policy.
- Depth-limited evaluation.

### Phase 5: Belief Weighting

Improve hidden-card sampling based on public behavior.

Deliverables:

- Claim history tracking.
- Weighted card assignment sampling.
- Challenge likelihood estimates.
- Bluff tendency estimates.

### Phase 6: Evaluation And Tuning

Measure engine strength and tune parameters.

Deliverables:

- Self-play harness.
- Baseline bots.
- Win-rate reports.
- Regression tests for obvious tactical decisions.

## Testing Strategy

Use deterministic tests wherever possible.

Important test categories:

- Rules tests for every action and reaction.
- Card count and sampling consistency tests.
- Terminal state tests.
- Seeded ISMCTS decision tests.
- Tactical scenario tests.
- Self-play smoke tests.

Example tactical scenarios:

- Must coup with 10 or more coins.
- Prefer couping a one-influence opponent when possible.
- Challenge impossible claims when all copies of a card are visible.
- Avoid challenging likely truthful low-impact claims.
- Block stealing when holding Captain or Ambassador.

## Known Pitfalls

### Hidden Information Leakage

Never let the engine use the true hidden cards of opponents when choosing real actions.

Only sampled states inside search iterations may contain opponent hidden cards.

### Weak Rollouts

Purely random rollouts can make the search noisy and produce bad bluffing behavior. Add simple heuristics early.

### Strategy Fusion

ISMCTS can still suffer from strategy fusion if future decisions accidentally depend too much on sampled hidden information. Keep decision nodes tied to observable information where practical.

### Multi-Player Credit Assignment

Coup is not two-player zero-sum in larger games. Rewards should score the root player's outcome, not assume every opponent has the same objective.

### Challenge And Block Complexity

Many important Coup decisions happen in reactions, not only main actions. Treat reaction choices as first-class search decisions.

## Initial Minimal Version

The smallest useful implementation is:

1. Fully tested rules engine.
2. Uniform hidden-state sampler.
3. ISMCTS for active turn actions only.
4. Heuristic policies for reactions during rollout.
5. Simple terminal reward: win = 1.0, loss = 0.0.

Once that works, extend ISMCTS to challenge, block, and reveal decisions.

## Success Criteria

The first version is successful if it can:

- Play legal complete games without panics.
- Never use hidden opponent cards directly when making real decisions.
- Beat a random legal-action bot consistently.
- Make obvious tactical decisions correctly.
- Produce deterministic decisions with a fixed seed.
