//! # Rusty Duke
//!
//! A pure Rust implementation of the card game [Coup](https://coup.games/).
//!
//! ## Overview
//!
//! This crate provides a complete, deterministic game engine for Coup, supporting:
//! - Full game rules (actions, challenges, blocks, coups, exchanges)
//! - Partial observability with `Observation` and `determinize` for ISMCTS
//! - Pluggable AI bots (`RandomBot`, `HeuristicBot`, `IsmctsBot`)
//! - Benchmarking harness for bot evaluation
//!
//! ## Game Flow
//!
//! 1. Create a [`GameState`] with [`GameState::new`].
//! 2. Call [`GameState::legal_moves`] for the active player.
//! 3. Call [`GameState::apply_move`] to execute a move.
//! 4. Repeat until a winner is determined ([`GameState::winner`]).
//!
//! ## Example
//!
//! ```
//! use rusty_duke::{GameState, Move};
//!
//! let mut game = GameState::new(3, 42).unwrap();
//! while game.winner().is_none() {
//!     let moves = game.legal_moves(game.active_player().unwrap());
//!     game.apply_move(game.active_player().unwrap(), moves[0].clone()).unwrap();
//! }
//! println!("Winner: {:?}", game.winner());
//! ```

use rand::prelude::*;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use std::collections::HashSet;

pub mod engine;

/// Unique identifier for a player (0-indexed).
pub type PlayerId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Card {
    Duke,
    Assassin,
    Captain,
    Ambassador,
    Contessa,
}

/// A card that has been revealed, losing its owner an influence slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InfluenceCard {
    /// The card type.
    pub card: Card,
    /// Whether this card has been revealed (and thus lost).
    pub revealed: bool,
}

/// The state of a single player during a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    /// The number of coins this player holds.
    pub coins: u8,
    /// This player's influence cards, ordered front to back.
    ///
    /// A card with `revealed: true` has been lost and no longer
    /// provides influence. A player is eliminated when all cards
    /// are revealed.
    pub influence: Vec<InfluenceCard>,
}

impl PlayerState {
    /// Returns `true` if this player still has hidden influence cards.
    pub fn is_alive(&self) -> bool {
        self.influence.iter().any(|card| !card.revealed)
    }

    /// Returns the number of unrevealed influence cards.
    pub fn hidden_count(&self) -> usize {
        self.influence.iter().filter(|card| !card.revealed).count()
    }

    /// Returns the list of card types that have been revealed.
    pub fn revealed_cards(&self) -> Vec<Card> {
        self.influence
            .iter()
            .filter(|card| card.revealed)
            .map(|card| card.card)
            .collect()
    }
}

/// An action that a player may declare on their turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// Take 1 coin from the treasury (no block possible).
    ForeignAid,
    /// Take 3 coins, claiming to be the Duke.
    Tax,
    /// Pay 3 coins to force a target to reveal an influence card.
    Assassinate { target: PlayerId },
    /// Take up to 2 coins from a target, claiming to be the Captain.
    Steal { target: PlayerId },
    /// Swap influence cards with the deck, claiming to be the Ambassador.
    Exchange,
}

impl ActionKind {
    pub fn claim(self) -> Option<Card> {
        match self {
            ActionKind::ForeignAid => None,
            ActionKind::Tax => Some(Card::Duke),
            ActionKind::Assassinate { .. } => Some(Card::Assassin),
            ActionKind::Steal { .. } => Some(Card::Captain),
            ActionKind::Exchange => Some(Card::Ambassador),
        }
    }

    pub fn target(self) -> Option<PlayerId> {
        match self {
            ActionKind::Assassinate { target } | ActionKind::Steal { target } => Some(target),
            ActionKind::ForeignAid | ActionKind::Tax | ActionKind::Exchange => None,
        }
    }
}

/// A declaration of an action by a specific player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeclaredAction {
    /// The player who declared the action.
    pub actor: PlayerId,
    /// The kind of action being declared.
    pub kind: ActionKind,
}

/// The current phase of a game turn, determining what moves are legal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Awaiting the active player's action declaration.
    AwaitingAction {
        /// The player who must act.
        actor: PlayerId,
    },
    /// Awaiting responses to a challenge of a declared action.
    AwaitingChallenge {
        /// The action being challenged.
        action: DeclaredAction,
        /// Index into the list of eligible challengers.
        responder_index: usize,
    },
    /// Awaiting responses to a block of a declared action.
    AwaitingBlock {
        /// The action being blocked.
        action: DeclaredAction,
        /// Index into the list of eligible blockers.
        responder_index: usize,
    },
    /// Awaiting responses to a challenge of a block.
    AwaitingBlockChallenge {
        /// The action that was blocked.
        action: DeclaredAction,
        /// The player who played the block.
        blocker: PlayerId,
        /// The card claimed for the block.
        block_card: Card,
        /// Index into the list of eligible block challengers.
        responder_index: usize,
    },
    /// Awaiting a player to lose an influence card.
    AwaitingInfluenceLoss {
        /// The player who must reveal a card.
        player: PlayerId,
        /// The phase to resume after the loss.
        next: Box<Phase>,
    },
    /// Awaiting an exchange action's return cards.
    AwaitingExchangeReturn {
        /// The player performing the exchange.
        player: PlayerId,
        /// The cards drawn from the deck.
        drawn: Vec<Card>,
    },
    /// The game has ended.
    Terminal {
        /// The winning player.
        winner: PlayerId,
    },
}

/// Possible actions a player can take on their turn.
///
/// Note: `Challenge` and `PassChallenge` are used in response to declared
/// actions, and `Block` / `PassBlock` are used in response to blocks.
/// `RevealInfluence` is used when a player must lose an influence card.
/// `ExchangeReturn` specifies which cards to keep after an exchange.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Move {
    /// Take 1 coin (no contestable action).
    Income,
    /// Take 2 coins from the treasury (can be blocked by Duke).
    ForeignAid,
    /// Take 3 coins (claim Duke).
    Tax,
    /// Force target to reveal an influence card (claim Assassin, costs 3).
    Assassinate { target: PlayerId },
    /// Take up to 2 coins from target (claim Captain).
    Steal { target: PlayerId },
    /// Draw 2 cards, return 2 (claim Ambassador).
    Exchange,
    /// Force target to reveal an influence card (costs 7, no claim).
    Coup { target: PlayerId },
    /// Challenge the current declared action.
    Challenge,
    /// Accept the current declared action without challenging.
    PassChallenge,
    /// Block the current action by claiming a card.
    Block { claim: Card },
    /// Decline to block the current action.
    PassBlock,
    /// Reveal a specific influence card when forced to lose one.
    RevealInfluence { card_index: usize },
    /// Return the chosen cards after an exchange.
    ExchangeReturn { keep: Vec<Card> },
}

/// Errors that can occur during game operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameError {
    /// The specified player index is out of bounds.
    InvalidPlayer,
    /// The player count is not between 2 and 6.
    InvalidPlayerCount,
    /// The move is not legal in the current game state.
    InvalidMove,
    /// It is not the specified player's turn.
    NotPlayersTurn,
    /// The player does not have enough coins for the action.
    NotEnoughCoins,
    /// The specified target is invalid.
    InvalidTarget,
    /// The player does not have the required influence card.
    InvalidInfluence,
    /// The exchange return is invalid.
    InvalidExchangeReturn,
    /// The game is already over.
    GameOver,
}

/// A player's view of the game, hiding opponents' hidden cards.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObservedPlayer {
    /// The number of coins this player has.
    pub coins: u8,
    /// The number of hidden (unrevealed) influence cards.
    pub hidden_influence: usize,
    /// The card types this player has revealed.
    pub revealed: Vec<Card>,
    /// Whether this player is still alive.
    pub alive: bool,
}

/// A player's complete view of the game state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Observation {
    /// The player making this observation.
    pub viewer: PlayerId,
    /// Each player's observed state.
    pub players: Vec<ObservedPlayer>,
    /// The viewer's own hidden influence cards.
    pub own_hidden_cards: Vec<Card>,
    /// The number of cards remaining in the deck.
    pub deck_size: usize,
    /// The index of the player whose turn it is, if any.
    pub current_player: Option<PlayerId>,
    /// The current phase of the game.
    pub phase: Phase,
}

/// The full game state, used by the engine to manage game progression.
#[derive(Debug, Clone)]
pub struct GameState {
    /// All players' states.
    pub players: Vec<PlayerState>,
    /// The remaining deck of cards.
    pub deck: Vec<Card>,
    /// The current game phase.
    pub phase: Phase,
    rng: StdRng,
}

impl GameState {
    /// Creates a new game with the given number of players and random seed.
    ///
    /// Returns an error if `player_count` is not between 2 and 6 (inclusive).
    pub fn new(player_count: usize, seed: u64) -> Result<Self, GameError> {
        if !(2..=6).contains(&player_count) {
            return Err(GameError::InvalidPlayerCount);
        }

        let mut rng = StdRng::seed_from_u64(seed);
        let mut deck = standard_deck();
        deck.shuffle(&mut rng);

        let mut players = Vec::with_capacity(player_count);
        for _ in 0..player_count {
            let first = deck.pop().expect("standard deck has enough cards");
            let second = deck.pop().expect("standard deck has enough cards");
            players.push(PlayerState {
                coins: 2,
                influence: vec![
                    InfluenceCard {
                        card: first,
                        revealed: false,
                    },
                    InfluenceCard {
                        card: second,
                        revealed: false,
                    },
                ],
            });
        }

        Ok(Self {
            players,
            deck,
            phase: Phase::AwaitingAction { actor: 0 },
            rng,
        })
    }

    /// Returns the player whose turn it is, if the game is not over.
    ///
    /// In challenge and block phases, this is the player currently
    /// expected to respond.
    #[must_use]
    pub fn active_player(&self) -> Option<PlayerId> {
        match &self.phase {
            Phase::AwaitingAction { actor } => Some(*actor),
            Phase::AwaitingChallenge {
                action,
                responder_index,
            } => self
                .challenge_responders(*action)
                .get(*responder_index)
                .copied(),
            Phase::AwaitingBlock {
                action,
                responder_index,
            } => self
                .block_responders(*action)
                .get(*responder_index)
                .copied(),
            Phase::AwaitingBlockChallenge {
                blocker,
                responder_index,
                ..
            } => self
                .block_challenge_responders(*blocker)
                .get(*responder_index)
                .copied(),
            Phase::AwaitingInfluenceLoss { player, .. } => Some(*player),
            Phase::AwaitingExchangeReturn { player, .. } => Some(*player),
            Phase::Terminal { .. } => None,
        }
    }

    /// Returns the winner if the game is over, or `None` if it is still
    /// in progress. A player wins when all opponents have lost all their
    /// influence cards.
    #[must_use]
    pub fn winner(&self) -> Option<PlayerId> {
        match self.phase {
            Phase::Terminal { winner } => Some(winner),
            _ => self.only_living_player(),
        }
    }

    /// Returns `true` if the game has ended (i.e., all but one player
    /// have lost all their influence).
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.winner().is_some()
    }

    /// Returns the list of legal moves for a given player.
    ///
    /// Returns an empty list if it is not the player's turn or if the
    /// player index is invalid.
    #[must_use]
    pub fn legal_moves(&self, player: PlayerId) -> Vec<Move> {
        if self.players.get(player).is_none() || Some(player) != self.active_player() {
            return Vec::new();
        }

        match &self.phase {
            Phase::AwaitingAction { actor } if *actor == player => self.legal_actions(player),
            Phase::AwaitingChallenge { .. } => vec![Move::Challenge, Move::PassChallenge],
            Phase::AwaitingBlock { action, .. } => self.legal_blocks(*action),
            Phase::AwaitingBlockChallenge { .. } => vec![Move::Challenge, Move::PassChallenge],
            Phase::AwaitingInfluenceLoss { player: loser, .. } if *loser == player => self.players
                [player]
                .influence
                .iter()
                .enumerate()
                .filter(|(_, card)| !card.revealed)
                .map(|(card_index, _)| Move::RevealInfluence { card_index })
                .collect(),
            Phase::AwaitingExchangeReturn {
                player: exchanger,
                drawn,
            } if *exchanger == player => {
                let keep_count = self.players[player].hidden_count();
                let mut cards = self.hidden_cards(player);
                cards.extend(drawn.iter().copied());
                combinations(&cards, keep_count)
                    .into_iter()
                    .map(|keep| Move::ExchangeReturn { keep })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Applies a move to the game state.
    ///
    /// Returns `Ok(())` on success, or a [`GameError`] if the move is
    /// illegal, the player is invalid, or the game is already over.
    pub fn apply_move(&mut self, player: PlayerId, mv: Move) -> Result<(), GameError> {
        if player >= self.players.len() {
            return Err(GameError::InvalidPlayer);
        }
        if matches!(self.phase, Phase::Terminal { .. }) {
            return Err(GameError::GameOver);
        }
        let mv = match mv {
            Move::ExchangeReturn { keep } => {
                let mut sorted = keep;
                sorted.sort_by_key(|card| *card as u8);
                Move::ExchangeReturn { keep: sorted }
            }
            mv => mv,
        };
        if !self.legal_moves(player).contains(&mv) {
            return Err(GameError::InvalidMove);
        }

        match mv {
            Move::Income => {
                self.players[player].coins += 1;
                self.end_turn();
            }
            Move::ForeignAid => {
                self.phase = Phase::AwaitingBlock {
                    action: DeclaredAction {
                        actor: player,
                        kind: ActionKind::ForeignAid,
                    },
                    responder_index: 0,
                };
                self.skip_empty_response_windows();
            }
            Move::Tax => self.start_claimed_action(player, ActionKind::Tax),
            Move::Assassinate { target } => {
                self.players[player].coins -= 3;
                self.start_claimed_action(player, ActionKind::Assassinate { target });
            }
            Move::Steal { target } => {
                self.start_claimed_action(player, ActionKind::Steal { target })
            }
            Move::Exchange => self.start_claimed_action(player, ActionKind::Exchange),
            Move::Coup { target } => {
                self.players[player].coins -= 7;
                self.phase = Phase::AwaitingInfluenceLoss {
                    player: target,
                    next: Box::new(self.next_turn_phase()),
                };
            }
            Move::Challenge => self.resolve_challenge(player)?,
            Move::PassChallenge => self.pass_challenge()?,
            Move::Block { claim } => self.start_block_challenge(player, claim)?,
            Move::PassBlock => self.pass_block()?,
            Move::RevealInfluence { card_index } => self.reveal_influence(player, card_index)?,
            Move::ExchangeReturn { keep } => self.exchange_return(player, keep)?,
        }

        self.finish_if_terminal();
        Ok(())
    }

    /// Returns a partial observation of the game from a specific player's
    /// perspective, hiding opponents' unrevealed cards.
    pub fn observation_for(&self, viewer: PlayerId) -> Result<Observation, GameError> {
        if viewer >= self.players.len() {
            return Err(GameError::InvalidPlayer);
        }

        let players = self
            .players
            .iter()
            .map(|player| ObservedPlayer {
                coins: player.coins,
                hidden_influence: player.hidden_count(),
                revealed: player.revealed_cards(),
                alive: player.is_alive(),
            })
            .collect();

        Ok(Observation {
            viewer,
            players,
            own_hidden_cards: self.hidden_cards(viewer),
            deck_size: self.deck.len(),
            current_player: self.active_player(),
            phase: self.phase.clone(),
        })
    }

    /// Computes a full game state from a partial observation by randomly
    /// resolving unknown cards.
    ///
    /// All cards visible to the observer (including their own hidden cards)
    /// are placed deterministically; remaining cards are shuffled into
    /// unknown positions and the deck.
    pub fn determinize(observation: &Observation, seed: u64) -> Result<Self, GameError> {
        if observation.viewer >= observation.players.len() {
            return Err(GameError::InvalidPlayer);
        }

        let mut rng = StdRng::seed_from_u64(seed);
        let mut unknown = standard_deck();
        for player in &observation.players {
            for card in &player.revealed {
                remove_one(&mut unknown, *card).ok_or(GameError::InvalidInfluence)?;
            }
        }
        for card in &observation.own_hidden_cards {
            remove_one(&mut unknown, *card).ok_or(GameError::InvalidInfluence)?;
        }
        unknown.shuffle(&mut rng);

        let mut players = Vec::with_capacity(observation.players.len());
        for (player_id, observed) in observation.players.iter().enumerate() {
            let mut influence = Vec::new();
            if player_id == observation.viewer {
                influence.extend(
                    observation
                        .own_hidden_cards
                        .iter()
                        .map(|card| InfluenceCard {
                            card: *card,
                            revealed: false,
                        }),
                );
            } else {
                for _ in 0..observed.hidden_influence {
                    influence.push(InfluenceCard {
                        card: unknown.pop().ok_or(GameError::InvalidInfluence)?,
                        revealed: false,
                    });
                }
            }
            influence.extend(observed.revealed.iter().map(|card| InfluenceCard {
                card: *card,
                revealed: true,
            }));
            players.push(PlayerState {
                coins: observed.coins,
                influence,
            });
        }

        let mut deck = Vec::with_capacity(observation.deck_size);
        for _ in 0..observation.deck_size {
            deck.push(unknown.pop().ok_or(GameError::InvalidInfluence)?);
        }

        Ok(Self {
            players,
            deck,
            phase: observation.phase.clone(),
            rng,
        })
    }

    fn legal_actions(&self, player: PlayerId) -> Vec<Move> {
        if !self.players[player].is_alive() {
            return Vec::new();
        }

        let targets: Vec<PlayerId> = self
            .players
            .iter()
            .enumerate()
            .filter(|(target, state)| *target != player && state.is_alive())
            .map(|(target, _)| target)
            .collect();

        if self.players[player].coins >= 10 {
            return targets
                .into_iter()
                .map(|target| Move::Coup { target })
                .collect();
        }

        let mut moves = vec![Move::Income, Move::ForeignAid, Move::Tax, Move::Exchange];
        if self.players[player].coins >= 3 {
            moves.extend(
                targets
                    .iter()
                    .copied()
                    .map(|target| Move::Assassinate { target }),
            );
        }
        if self.players[player].coins >= 7 {
            moves.extend(targets.iter().copied().map(|target| Move::Coup { target }));
        }
        moves.extend(targets.into_iter().map(|target| Move::Steal { target }));
        moves
    }

    fn legal_blocks(&self, action: DeclaredAction) -> Vec<Move> {
        let mut moves = vec![Move::PassBlock];
        match action.kind {
            ActionKind::ForeignAid => moves.push(Move::Block { claim: Card::Duke }),
            ActionKind::Assassinate { .. } => moves.push(Move::Block {
                claim: Card::Contessa,
            }),
            ActionKind::Steal { .. } => {
                moves.push(Move::Block {
                    claim: Card::Captain,
                });
                moves.push(Move::Block {
                    claim: Card::Ambassador,
                });
            }
            ActionKind::Tax | ActionKind::Exchange => {}
        }
        moves
    }

    fn start_claimed_action(&mut self, actor: PlayerId, kind: ActionKind) {
        self.phase = Phase::AwaitingChallenge {
            action: DeclaredAction { actor, kind },
            responder_index: 0,
        };
        self.skip_empty_response_windows();
    }

    fn resolve_challenge(&mut self, challenger: PlayerId) -> Result<(), GameError> {
        match self.phase.clone() {
            Phase::AwaitingChallenge { action, .. } => {
                let claim = action.kind.claim().expect("challengeable action has claim");
                if self.has_hidden_card(action.actor, claim) {
                    self.replace_revealed_claim(action.actor, claim)?;
                    let next = if self.block_responders(action).is_empty() {
                        self.apply_action_effects(action);
                        self.action_followup_phase(action)
                    } else {
                        Phase::AwaitingBlock {
                            action,
                            responder_index: 0,
                        }
                    };
                    self.phase = Phase::AwaitingInfluenceLoss {
                        player: challenger,
                        next: Box::new(next),
                    };
                } else {
                    self.phase = Phase::AwaitingInfluenceLoss {
                        player: action.actor,
                        next: Box::new(self.next_turn_phase()),
                    };
                }
            }
            Phase::AwaitingBlockChallenge {
                action,
                blocker,
                block_card,
                ..
            } => {
                if self.has_hidden_card(blocker, block_card) {
                    self.replace_revealed_claim(blocker, block_card)?;
                    self.phase = Phase::AwaitingInfluenceLoss {
                        player: challenger,
                        next: Box::new(self.next_turn_phase()),
                    };
                } else {
                    self.apply_action_effects(action);
                    self.phase = Phase::AwaitingInfluenceLoss {
                        player: blocker,
                        next: Box::new(self.action_followup_phase(action)),
                    };
                }
            }
            _ => return Err(GameError::InvalidMove),
        }
        Ok(())
    }

    fn pass_challenge(&mut self) -> Result<(), GameError> {
        match self.phase.clone() {
            Phase::AwaitingChallenge {
                action,
                responder_index,
            } => {
                let next = responder_index + 1;
                if next < self.challenge_responders(action).len() {
                    self.phase = Phase::AwaitingChallenge {
                        action,
                        responder_index: next,
                    };
                } else if self.block_responders(action).is_empty() {
                    self.execute_action_now(action);
                } else {
                    self.phase = Phase::AwaitingBlock {
                        action,
                        responder_index: 0,
                    };
                }
            }
            Phase::AwaitingBlockChallenge {
                action,
                blocker,
                block_card,
                responder_index,
            } => {
                let next = responder_index + 1;
                if next < self.block_challenge_responders(blocker).len() {
                    self.phase = Phase::AwaitingBlockChallenge {
                        action,
                        blocker,
                        block_card,
                        responder_index: next,
                    };
                } else {
                    self.end_turn();
                }
            }
            _ => return Err(GameError::InvalidMove),
        }
        self.skip_empty_response_windows();
        Ok(())
    }

    fn start_block_challenge(&mut self, blocker: PlayerId, claim: Card) -> Result<(), GameError> {
        let Phase::AwaitingBlock { action, .. } = self.phase else {
            return Err(GameError::InvalidMove);
        };
        self.phase = Phase::AwaitingBlockChallenge {
            action,
            blocker,
            block_card: claim,
            responder_index: 0,
        };
        self.skip_empty_response_windows();
        Ok(())
    }

    fn pass_block(&mut self) -> Result<(), GameError> {
        let Phase::AwaitingBlock {
            action,
            responder_index,
        } = self.phase
        else {
            return Err(GameError::InvalidMove);
        };
        let next = responder_index + 1;
        if next < self.block_responders(action).len() {
            self.phase = Phase::AwaitingBlock {
                action,
                responder_index: next,
            };
        } else {
            self.execute_action_now(action);
        }
        self.skip_empty_response_windows();
        Ok(())
    }

    fn reveal_influence(&mut self, player: PlayerId, card_index: usize) -> Result<(), GameError> {
        let Phase::AwaitingInfluenceLoss {
            player: loser,
            next,
        } = self.phase.clone()
        else {
            return Err(GameError::InvalidMove);
        };
        if player != loser {
            return Err(GameError::NotPlayersTurn);
        }
        let card = self.players[player]
            .influence
            .get_mut(card_index)
            .ok_or(GameError::InvalidInfluence)?;
        if card.revealed {
            return Err(GameError::InvalidInfluence);
        }
        card.revealed = true;
        self.phase = *next;
        Ok(())
    }

    fn exchange_return(&mut self, player: PlayerId, keep: Vec<Card>) -> Result<(), GameError> {
        let Phase::AwaitingExchangeReturn {
            player: actor,
            drawn,
        } = self.phase.clone()
        else {
            return Err(GameError::InvalidMove);
        };
        if actor != player {
            return Err(GameError::NotPlayersTurn);
        }

        let keep_count = self.players[player].hidden_count();
        if keep.len() != keep_count {
            return Err(GameError::InvalidExchangeReturn);
        }

        let mut available = self.hidden_cards(player);
        available.extend(drawn.iter().copied());
        for card in &keep {
            remove_one(&mut available, *card).ok_or(GameError::InvalidExchangeReturn)?;
        }

        for influence in self.players[player]
            .influence
            .iter_mut()
            .filter(|influence| !influence.revealed)
            .zip(keep.into_iter())
        {
            influence.0.card = influence.1;
        }
        self.deck.extend(available);
        self.deck.shuffle(&mut self.rng);
        self.end_turn();
        Ok(())
    }

    fn action_followup_phase(&self, action: DeclaredAction) -> Phase {
        match action.kind {
            ActionKind::ForeignAid | ActionKind::Tax | ActionKind::Steal { .. } => {
                self.next_turn_phase()
            }
            ActionKind::Assassinate { target } => Phase::AwaitingInfluenceLoss {
                player: target,
                next: Box::new(self.next_turn_phase()),
            },
            ActionKind::Exchange => Phase::AwaitingExchangeReturn {
                player: action.actor,
                drawn: Vec::new(),
            },
        }
    }

    fn execute_action_now(&mut self, action: DeclaredAction) {
        self.apply_action_effects(action);
        self.phase = self.action_followup_phase(action);
    }

    fn apply_action_effects(&mut self, action: DeclaredAction) {
        match action.kind {
            ActionKind::ForeignAid => self.players[action.actor].coins += 2,
            ActionKind::Tax => self.players[action.actor].coins += 3,
            ActionKind::Steal { target } => {
                let stolen = self.players[target].coins.min(2);
                self.players[target].coins -= stolen;
                self.players[action.actor].coins += stolen;
            }
            ActionKind::Exchange => {}
            ActionKind::Assassinate { .. } => {}
        }
    }

    fn skip_empty_response_windows(&mut self) {
        loop {
            match self.phase.clone() {
                Phase::AwaitingChallenge { action, .. }
                    if self.challenge_responders(action).is_empty() =>
                {
                    if self.block_responders(action).is_empty() {
                        self.execute_action_now(action);
                    } else {
                        self.phase = Phase::AwaitingBlock {
                            action,
                            responder_index: 0,
                        };
                    }
                }
                Phase::AwaitingBlock { action, .. } if self.block_responders(action).is_empty() => {
                    self.execute_action_now(action);
                }
                Phase::AwaitingBlockChallenge {
                    blocker, action, ..
                } if self.block_challenge_responders(blocker).is_empty() => {
                    self.phase = self.next_turn_phase();
                    let _ = action;
                }
                Phase::AwaitingExchangeReturn { player, drawn } if drawn.is_empty() => {
                    let drawn = vec![self.deck.pop().unwrap(), self.deck.pop().unwrap()];
                    self.phase = Phase::AwaitingExchangeReturn { player, drawn };
                    break;
                }
                Phase::AwaitingAction { actor } if !self.players[actor].is_alive() => {
                    self.end_turn()
                }
                _ => break,
            }
        }

        if let Phase::AwaitingAction { .. } = self.phase {}
    }

    fn end_turn(&mut self) {
        self.phase = self.next_turn_phase();
        self.skip_empty_response_windows();
    }

    fn next_turn_phase(&self) -> Phase {
        let start = match self.phase {
            Phase::AwaitingAction { actor } => actor,
            Phase::AwaitingChallenge { action, .. }
            | Phase::AwaitingBlock { action, .. }
            | Phase::AwaitingBlockChallenge { action, .. } => action.actor,
            Phase::AwaitingInfluenceLoss { .. } | Phase::AwaitingExchangeReturn { .. } => {
                self.current_actor_fallback()
            }
            Phase::Terminal { winner } => return Phase::Terminal { winner },
        };

        let Some(next) = self.next_living_after(start) else {
            return Phase::Terminal { winner: start };
        };
        Phase::AwaitingAction { actor: next }
    }

    fn current_actor_fallback(&self) -> PlayerId {
        self.players
            .iter()
            .enumerate()
            .find(|(_, player)| player.is_alive())
            .map(|(player, _)| player)
            .unwrap_or(0)
    }

    fn finish_if_terminal(&mut self) {
        if let Some(winner) = self.only_living_player() {
            self.phase = Phase::Terminal { winner };
        }
        if matches!(&self.phase, Phase::AwaitingExchangeReturn { drawn, .. } if drawn.is_empty())
            && let Phase::AwaitingExchangeReturn { player, .. } =
                std::mem::replace(&mut self.phase, Phase::Terminal { winner: 0 })
        {
            let drawn = vec![
                self.deck.pop().expect("deck exhausted: invalid game state"),
                self.deck.pop().expect("deck exhausted: invalid game state"),
            ];
            self.phase = Phase::AwaitingExchangeReturn { player, drawn };
        }
        if matches!(&self.phase, Phase::AwaitingAction { actor } if !self.players[*actor].is_alive()) {
            self.end_turn()
        }
    }

    fn challenge_responders(&self, action: DeclaredAction) -> Vec<PlayerId> {
        self.players
            .iter()
            .enumerate()
            .filter(|(player, state)| *player != action.actor && state.is_alive())
            .map(|(player, _)| player)
            .collect()
    }

    fn block_responders(&self, action: DeclaredAction) -> Vec<PlayerId> {
        match action.kind {
            ActionKind::ForeignAid => self
                .players
                .iter()
                .enumerate()
                .filter(|(player, state)| *player != action.actor && state.is_alive())
                .map(|(player, _)| player)
                .collect(),
            ActionKind::Assassinate { target } | ActionKind::Steal { target } => {
                if self.players[target].is_alive() {
                    vec![target]
                } else {
                    Vec::new()
                }
            }
            ActionKind::Tax | ActionKind::Exchange => Vec::new(),
        }
    }

    fn block_challenge_responders(&self, blocker: PlayerId) -> Vec<PlayerId> {
        self.players
            .iter()
            .enumerate()
            .filter(|(player, state)| *player != blocker && state.is_alive())
            .map(|(player, _)| player)
            .collect()
    }

    fn next_living_after(&self, player: PlayerId) -> Option<PlayerId> {
        for offset in 1..=self.players.len() {
            let candidate = (player + offset) % self.players.len();
            if self.players[candidate].is_alive() {
                return Some(candidate);
            }
        }
        None
    }

    fn only_living_player(&self) -> Option<PlayerId> {
        let living: Vec<_> = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, player)| player.is_alive())
            .map(|(player, _)| player)
            .collect();
        if living.len() == 1 {
            Some(living[0])
        } else {
            None
        }
    }

    fn has_hidden_card(&self, player: PlayerId, claim: Card) -> bool {
        self.players[player]
            .influence
            .iter()
            .any(|card| !card.revealed && card.card == claim)
    }

    fn hidden_cards(&self, player: PlayerId) -> Vec<Card> {
        self.players[player]
            .influence
            .iter()
            .filter(|card| !card.revealed)
            .map(|card| card.card)
            .collect()
    }

    fn replace_revealed_claim(&mut self, player: PlayerId, claim: Card) -> Result<(), GameError> {
        let card = self.players[player]
            .influence
            .iter_mut()
            .find(|card| !card.revealed && card.card == claim)
            .ok_or(GameError::InvalidInfluence)?;
        self.deck.push(claim);
        self.deck.shuffle(&mut self.rng);
        card.card = self.deck.pop().ok_or(GameError::InvalidInfluence)?;
        Ok(())
    }
}

fn standard_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(15);
    for card in [
        Card::Duke,
        Card::Assassin,
        Card::Captain,
        Card::Ambassador,
        Card::Contessa,
    ] {
        deck.extend([card; 3]);
    }
    deck
}

fn remove_one(cards: &mut Vec<Card>, target: Card) -> Option<Card> {
    let index = cards.iter().position(|card| *card == target)?;
    Some(cards.remove(index))
}

fn combinations(cards: &[Card], keep_count: usize) -> Vec<Vec<Card>> {
    fn go(
        cards: &[Card],
        keep_count: usize,
        start: usize,
        current: &mut Vec<Card>,
        out: &mut HashSet<Vec<Card>>,
    ) {
        if current.len() == keep_count {
            let mut next = current.clone();
            next.sort_by_key(|card| *card as u8);
            out.insert(next);
            return;
        }
        for index in start..cards.len() {
            current.push(cards[index]);
            go(cards, keep_count, index + 1, current, out);
            current.pop();
        }
    }

    let mut out = HashSet::new();
    go(cards, keep_count, 0, &mut Vec::new(), &mut out);
    let mut out: Vec<_> = out.into_iter().collect();
    out.sort_by_key(|v| {
        let mut sorted = v.clone();
        sorted.sort_by_key(|card| *card as u8);
        sorted
    });
    out
}

pub trait CoupGame {
    fn current_player(&self) -> Option<PlayerId>;
    fn legal_moves(&self) -> Vec<Move>;
    fn apply_current_move(&mut self, mv: Move) -> Result<(), GameError>;
    fn is_terminal(&self) -> bool;
    fn winner(&self) -> Option<PlayerId>;
    fn observation_for(&self, player: PlayerId) -> Result<Observation, GameError>;
}

impl CoupGame for GameState {
    fn current_player(&self) -> Option<PlayerId> {
        self.active_player()
    }

    fn legal_moves(&self) -> Vec<Move> {
        self.active_player()
            .map(|player| self.legal_moves(player))
            .unwrap_or_default()
    }

    fn apply_current_move(&mut self, mv: Move) -> Result<(), GameError> {
        let player = self.active_player().ok_or(GameError::GameOver)?;
        self.apply_move(player, mv)
    }

    fn is_terminal(&self) -> bool {
        GameState::is_terminal(self)
    }

    fn winner(&self) -> Option<PlayerId> {
        GameState::winner(self)
    }

    fn observation_for(&self, player: PlayerId) -> Result<Observation, GameError> {
        GameState::observation_for(self, player)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rigged_game(players: Vec<Vec<Card>>, deck: Vec<Card>) -> GameState {
        GameState {
            players: players
                .into_iter()
                .map(|cards| PlayerState {
                    coins: 2,
                    influence: cards
                        .into_iter()
                        .map(|card| InfluenceCard {
                            card,
                            revealed: false,
                        })
                        .collect(),
                })
                .collect(),
            deck,
            phase: Phase::AwaitingAction { actor: 0 },
            rng: StdRng::seed_from_u64(1),
        }
    }

    #[test]
    fn setup_deals_two_cards_and_two_coins() {
        let game = GameState::new(4, 7).unwrap();
        assert_eq!(game.players.len(), 4);
        assert_eq!(game.deck.len(), 7);
        assert!(game.players.iter().all(|player| player.coins == 2));
        assert!(game.players.iter().all(|player| player.hidden_count() == 2));
        assert_eq!(game.active_player(), Some(0));
    }

    #[test]
    fn income_adds_one_and_advances_turn() {
        let mut game = GameState::new(3, 1).unwrap();
        game.apply_move(0, Move::Income).unwrap();
        assert_eq!(game.players[0].coins, 3);
        assert_eq!(game.active_player(), Some(1));
    }

    #[test]
    fn player_with_ten_coins_must_coup() {
        let mut game = GameState::new(3, 1).unwrap();
        game.players[0].coins = 10;
        let legal = game.legal_moves(0);
        assert!(legal.iter().all(|mv| matches!(mv, Move::Coup { .. })));
        assert_eq!(legal.len(), 2);
    }

    #[test]
    fn coup_costs_seven_and_reveals_target_influence() {
        let mut game = GameState::new(2, 1).unwrap();
        game.players[0].coins = 7;
        game.apply_move(0, Move::Coup { target: 1 }).unwrap();
        assert_eq!(game.players[0].coins, 0);
        assert_eq!(game.active_player(), Some(1));
        game.apply_move(1, Move::RevealInfluence { card_index: 0 })
            .unwrap();
        assert_eq!(game.players[1].hidden_count(), 1);
    }

    #[test]
    fn false_tax_claim_loses_influence_and_action_fails() {
        let mut game = rigged_game(
            vec![
                vec![Card::Captain, Card::Contessa],
                vec![Card::Duke, Card::Duke],
            ],
            vec![Card::Assassin, Card::Ambassador, Card::Captain],
        );
        game.apply_move(0, Move::Tax).unwrap();
        game.apply_move(1, Move::Challenge).unwrap();
        game.apply_move(0, Move::RevealInfluence { card_index: 0 })
            .unwrap();
        assert_eq!(game.players[0].coins, 2);
        assert_eq!(game.players[0].hidden_count(), 1);
        assert_eq!(game.active_player(), Some(1));
    }

    #[test]
    fn true_tax_claim_punishes_challenger_then_tax_resolves() {
        let mut game = rigged_game(
            vec![
                vec![Card::Duke, Card::Contessa],
                vec![Card::Captain, Card::Captain],
            ],
            vec![Card::Assassin, Card::Ambassador, Card::Captain],
        );
        game.apply_move(0, Move::Tax).unwrap();
        game.apply_move(1, Move::Challenge).unwrap();
        game.apply_move(1, Move::RevealInfluence { card_index: 0 })
            .unwrap();
        assert_eq!(game.players[0].coins, 5);
        assert_eq!(game.players[1].hidden_count(), 1);
        assert_eq!(game.active_player(), Some(1));
    }

    #[test]
    fn foreign_aid_can_be_blocked_by_duke() {
        let mut game = GameState::new(3, 1).unwrap();
        game.apply_move(0, Move::ForeignAid).unwrap();
        assert_eq!(game.active_player(), Some(1));
        game.apply_move(1, Move::Block { claim: Card::Duke })
            .unwrap();
        game.apply_move(0, Move::PassChallenge).unwrap();
        game.apply_move(2, Move::PassChallenge).unwrap();
        assert_eq!(game.players[0].coins, 2);
        assert_eq!(game.active_player(), Some(1));
    }

    #[test]
    fn steal_takes_up_to_two_coins() {
        let mut game = rigged_game(
            vec![
                vec![Card::Captain, Card::Duke],
                vec![Card::Contessa, Card::Duke],
            ],
            vec![Card::Assassin, Card::Ambassador, Card::Captain],
        );
        game.players[1].coins = 1;
        game.apply_move(0, Move::Steal { target: 1 }).unwrap();
        game.apply_move(1, Move::PassChallenge).unwrap();
        assert_eq!(game.active_player(), Some(1));
        game.apply_move(1, Move::PassBlock).unwrap();
        assert_eq!(game.players[0].coins, 3);
        assert_eq!(game.players[1].coins, 0);
    }

    #[test]
    fn observation_hides_opponent_cards() {
        let game = rigged_game(
            vec![
                vec![Card::Duke, Card::Contessa],
                vec![Card::Captain, Card::Assassin],
            ],
            vec![Card::Ambassador, Card::Ambassador, Card::Duke],
        );
        let obs = game.observation_for(0).unwrap();
        assert_eq!(obs.own_hidden_cards, vec![Card::Duke, Card::Contessa]);
        assert_eq!(obs.players[1].hidden_influence, 2);
        assert!(obs.players[1].revealed.is_empty());
    }

    #[test]
    fn determinization_preserves_visible_information() {
        let mut game = rigged_game(
            vec![
                vec![Card::Duke, Card::Contessa],
                vec![Card::Captain, Card::Assassin],
            ],
            vec![
                Card::Ambassador,
                Card::Ambassador,
                Card::Duke,
                Card::Captain,
                Card::Assassin,
                Card::Contessa,
                Card::Duke,
                Card::Captain,
                Card::Assassin,
                Card::Contessa,
                Card::Ambassador,
            ],
        );
        game.players[1].influence[0].revealed = true;
        let obs = game.observation_for(0).unwrap();
        let det = GameState::determinize(&obs, 9).unwrap();
        assert_eq!(det.hidden_cards(0), vec![Card::Duke, Card::Contessa]);
        assert_eq!(det.players[1].revealed_cards(), vec![Card::Captain]);
        let total_cards = det.deck.len()
            + det
                .players
                .iter()
                .map(|player| player.influence.len())
                .sum::<usize>();
        assert_eq!(total_cards, 15);
    }

    #[test]
    fn cannot_target_self_or_dead_players() {
        let mut game = GameState::new(3, 1).unwrap();
        game.players[0].coins = 7;
        game.players[2].influence[0].revealed = true;
        game.players[2].influence[1].revealed = true;

        let legal = game.legal_moves(0);
        assert!(!legal.contains(&Move::Coup { target: 0 }));
        assert!(!legal.contains(&Move::Coup { target: 2 }));
        assert_eq!(
            game.apply_move(0, Move::Coup { target: 0 }),
            Err(GameError::InvalidMove)
        );
        assert_eq!(
            game.apply_move(0, Move::Coup { target: 2 }),
            Err(GameError::InvalidMove)
        );
    }

    #[test]
    fn assassinate_costs_three_and_forces_target_reveal_after_passes() {
        let mut game = rigged_game(
            vec![
                vec![Card::Assassin, Card::Duke],
                vec![Card::Contessa, Card::Captain],
            ],
            vec![Card::Ambassador, Card::Ambassador, Card::Duke],
        );
        game.players[0].coins = 3;

        game.apply_move(0, Move::Assassinate { target: 1 }).unwrap();
        assert_eq!(game.players[0].coins, 0);
        game.apply_move(1, Move::PassChallenge).unwrap();
        game.apply_move(1, Move::PassBlock).unwrap();

        assert_eq!(game.active_player(), Some(1));
        game.apply_move(1, Move::RevealInfluence { card_index: 0 })
            .unwrap();
        assert_eq!(game.players[1].hidden_count(), 1);
        assert_eq!(game.active_player(), Some(1));
    }

    #[test]
    fn true_assassination_block_stops_action_and_punishes_challenger() {
        let mut game = rigged_game(
            vec![
                vec![Card::Assassin, Card::Duke],
                vec![Card::Contessa, Card::Captain],
            ],
            vec![Card::Ambassador, Card::Ambassador, Card::Duke],
        );
        game.players[0].coins = 3;

        game.apply_move(0, Move::Assassinate { target: 1 }).unwrap();
        game.apply_move(1, Move::PassChallenge).unwrap();
        game.apply_move(
            1,
            Move::Block {
                claim: Card::Contessa,
            },
        )
        .unwrap();
        game.apply_move(0, Move::Challenge).unwrap();
        game.apply_move(0, Move::RevealInfluence { card_index: 0 })
            .unwrap();

        assert_eq!(game.players[0].hidden_count(), 1);
        assert_eq!(game.players[1].hidden_count(), 2);
        assert_eq!(game.active_player(), Some(1));
    }

    #[test]
    fn false_assassination_block_loses_blocker_influence_and_action_still_resolves() {
        let mut game = rigged_game(
            vec![
                vec![Card::Assassin, Card::Duke],
                vec![Card::Captain, Card::Captain],
            ],
            vec![Card::Ambassador, Card::Ambassador, Card::Duke],
        );
        game.players[0].coins = 3;

        game.apply_move(0, Move::Assassinate { target: 1 }).unwrap();
        game.apply_move(1, Move::PassChallenge).unwrap();
        game.apply_move(
            1,
            Move::Block {
                claim: Card::Contessa,
            },
        )
        .unwrap();
        game.apply_move(0, Move::Challenge).unwrap();

        assert_eq!(game.active_player(), Some(1));
        game.apply_move(1, Move::RevealInfluence { card_index: 0 })
            .unwrap();
        assert_eq!(game.players[1].hidden_count(), 1);
        assert_eq!(game.active_player(), Some(1));
        game.apply_move(1, Move::RevealInfluence { card_index: 1 })
            .unwrap();

        assert_eq!(game.players[1].hidden_count(), 0);
        assert_eq!(game.winner(), Some(0));
    }

    #[test]
    fn exchange_draws_two_and_returns_unkept_cards_to_deck() {
        let mut game = rigged_game(
            vec![
                vec![Card::Ambassador, Card::Duke],
                vec![Card::Captain, Card::Assassin],
            ],
            vec![Card::Contessa, Card::Captain, Card::Duke],
        );

        game.apply_move(0, Move::Exchange).unwrap();
        game.apply_move(1, Move::PassChallenge).unwrap();

        let drawn = match &game.phase {
            Phase::AwaitingExchangeReturn { player, drawn } => {
                assert_eq!(*player, 0);
                drawn.clone()
            }
            phase => panic!("unexpected phase: {phase:?}"),
        };
        assert_eq!(drawn.len(), 2);

        game.apply_move(
            0,
            Move::ExchangeReturn {
                keep: vec![Card::Ambassador, drawn[0]],
            },
        )
        .unwrap();

        assert_eq!(game.players[0].hidden_count(), 2);
        assert!(game.hidden_cards(0).contains(&Card::Ambassador));
        assert_eq!(game.deck.len(), 3);
        assert_eq!(game.active_player(), Some(1));
    }

    #[test]
    fn random_bot_returns_legal_move_for_observation() {
        let game = GameState::new(3, 1).unwrap();
        let observation = game.observation_for(0).unwrap();
        let mut bot = crate::engine::RandomBot;
        let mut rng = StdRng::seed_from_u64(7);

        let mv = crate::engine::Bot::choose_move(&mut bot, &observation, &mut rng).unwrap();

        assert!(game.legal_moves(0).contains(&mv));
    }

    #[test]
    fn ismcts_bot_returns_legal_move_for_observation() {
        let game = GameState::new(3, 1).unwrap();
        let observation = game.observation_for(0).unwrap();
        let mut bot = crate::engine::IsmctsBot::new(crate::engine::SearchConfig {
            iterations: 25,
            max_depth: 20,
            exploration: 1.4,
            rollout_policy: crate::engine::RolloutPolicyKind::Random,
        });
        let mut rng = StdRng::seed_from_u64(7);

        let mv = crate::engine::Bot::choose_move(&mut bot, &observation, &mut rng).unwrap();

        assert!(game.legal_moves(0).contains(&mv));
    }

    #[test]
    fn ismcts_bot_is_deterministic_with_seeded_rng() {
        let game = GameState::new(3, 1).unwrap();
        let observation = game.observation_for(0).unwrap();
        let config = crate::engine::SearchConfig {
            iterations: 25,
            max_depth: 20,
            exploration: 1.4,
            rollout_policy: crate::engine::RolloutPolicyKind::Random,
        };
        let mut first_bot = crate::engine::IsmctsBot::new(config.clone());
        let mut second_bot = crate::engine::IsmctsBot::new(config);
        let mut first_rng = StdRng::seed_from_u64(11);
        let mut second_rng = StdRng::seed_from_u64(11);

        let first = crate::engine::Bot::choose_move(&mut first_bot, &observation, &mut first_rng);
        let second =
            crate::engine::Bot::choose_move(&mut second_bot, &observation, &mut second_rng);

        assert_eq!(first, second);
    }

    #[test]
    fn eliminated_player_has_no_legal_moves() {
        let mut game = rigged_game(vec![vec![Card::Duke, Card::Assassin]], vec![]);
        // Player 0 loses all influence
        game.players[0].influence[0].revealed = true;
        game.players[0].influence[1].revealed = true;

        assert!(game.legal_moves(0).is_empty());
    }

    #[test]
    fn coup_on_eliminated_player_fails() {
        let mut game = rigged_game(
            vec![
                vec![Card::Duke, Card::Duke],
                vec![Card::Contessa, Card::Contessa],
            ],
            vec![],
        );
        // Eliminate player 1
        game.players[1].influence[0].revealed = true;
        game.players[1].influence[1].revealed = true;
        game.players[0].coins = 7;

        assert_eq!(
            game.apply_move(0, Move::Coup { target: 1 }),
            Err(GameError::InvalidMove)
        );
    }

    #[test]
    fn foreign_aid_cannot_be_blocked_by_dead_player() {
        let mut game = rigged_game(
            vec![
                vec![Card::Duke, Card::Duke],
                vec![Card::Contessa, Card::Contessa],
                vec![Card::Captain, Card::Captain],
            ],
            vec![],
        );
        // Eliminate player 2
        game.players[2].influence[0].revealed = true;
        game.players[2].influence[1].revealed = true;

        game.apply_move(0, Move::ForeignAid).unwrap();
        // Player 2 should not be a valid blocker since they're dead
        assert_eq!(
            game.apply_move(2, Move::Block { claim: Card::Duke }),
            Err(GameError::InvalidMove)
        );
    }

    #[test]
    fn steal_from_player_zero_coins_takes_nothing() {
        let mut game = rigged_game(
            vec![
                vec![Card::Captain, Card::Duke],
                vec![Card::Captain, Card::Duke],
            ],
            vec![Card::Assassin, Card::Ambassador, Card::Contessa],
        );
        game.players[1].coins = 0;
        game.players[0].coins = 2;

        game.apply_move(0, Move::Steal { target: 1 }).unwrap();
        game.apply_move(1, Move::PassChallenge).unwrap();
        game.apply_move(1, Move::PassBlock).unwrap();

        assert_eq!(game.players[0].coins, 2);
        assert_eq!(game.players[1].coins, 0);
    }

    #[test]
    fn income_cannot_be_claimed() {
        let mut game = rigged_game(
            vec![
                vec![Card::Duke, Card::Duke],
                vec![Card::Captain, Card::Assassin],
            ],
            vec![Card::Ambassador, Card::Contessa, Card::Duke],
        );
        game.players[0].coins = 11;

        // Player has >10 coins, only coup is legal
        let moves = game.legal_moves(0);
        assert!(moves
            .iter()
            .all(|mv| matches!(mv, Move::Coup { .. })));
    }

    #[test]
    fn challenge_cannot_target_foreign_aid() {
        // ForeignAid has no claim, so it cannot be challenged
        let mut game = GameState::new(3, 1).unwrap();
        game.apply_move(0, Move::ForeignAid).unwrap();
        // Player 1 should not be able to challenge ForeignAid
        // since ForeignAid has no associated card to claim
        let moves = game.legal_moves(1);
        assert!(!moves.contains(&Move::Challenge));
    }
}
