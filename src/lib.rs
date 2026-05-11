use rand::prelude::*;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;

pub mod engine;

pub type PlayerId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Card {
    Duke,
    Assassin,
    Captain,
    Ambassador,
    Contessa,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfluenceCard {
    pub card: Card,
    pub revealed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    pub coins: u8,
    pub influence: Vec<InfluenceCard>,
}

impl PlayerState {
    pub fn is_alive(&self) -> bool {
        self.influence.iter().any(|card| !card.revealed)
    }

    pub fn hidden_count(&self) -> usize {
        self.influence.iter().filter(|card| !card.revealed).count()
    }

    pub fn revealed_cards(&self) -> Vec<Card> {
        self.influence
            .iter()
            .filter(|card| card.revealed)
            .map(|card| card.card)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    ForeignAid,
    Tax,
    Assassinate { target: PlayerId },
    Steal { target: PlayerId },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeclaredAction {
    pub actor: PlayerId,
    pub kind: ActionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Phase {
    AwaitingAction {
        actor: PlayerId,
    },
    AwaitingChallenge {
        action: DeclaredAction,
        responder_index: usize,
    },
    AwaitingBlock {
        action: DeclaredAction,
        responder_index: usize,
    },
    AwaitingBlockChallenge {
        action: DeclaredAction,
        blocker: PlayerId,
        block_card: Card,
        responder_index: usize,
    },
    AwaitingInfluenceLoss {
        player: PlayerId,
        next: Box<Phase>,
    },
    AwaitingExchangeReturn {
        player: PlayerId,
        drawn: Vec<Card>,
    },
    Terminal {
        winner: PlayerId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Move {
    Income,
    ForeignAid,
    Tax,
    Assassinate { target: PlayerId },
    Steal { target: PlayerId },
    Exchange,
    Coup { target: PlayerId },
    Challenge,
    PassChallenge,
    Block { claim: Card },
    PassBlock,
    RevealInfluence { card_index: usize },
    ExchangeReturn { keep: Vec<Card> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameError {
    InvalidPlayer,
    InvalidPlayerCount,
    InvalidMove,
    NotPlayersTurn,
    NotEnoughCoins,
    InvalidTarget,
    InvalidInfluence,
    InvalidExchangeReturn,
    GameOver,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObservedPlayer {
    pub coins: u8,
    pub hidden_influence: usize,
    pub revealed: Vec<Card>,
    pub alive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Observation {
    pub viewer: PlayerId,
    pub players: Vec<ObservedPlayer>,
    pub own_hidden_cards: Vec<Card>,
    pub deck_size: usize,
    pub current_player: Option<PlayerId>,
    pub phase: Phase,
}

#[derive(Debug, Clone)]
pub struct GameState {
    pub players: Vec<PlayerState>,
    pub deck: Vec<Card>,
    pub phase: Phase,
    rng: StdRng,
}

impl GameState {
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

    pub fn winner(&self) -> Option<PlayerId> {
        match self.phase {
            Phase::Terminal { winner } => Some(winner),
            _ => self.only_living_player(),
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.winner().is_some()
    }

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

    pub fn apply_move(&mut self, player: PlayerId, mv: Move) -> Result<(), GameError> {
        if player >= self.players.len() {
            return Err(GameError::InvalidPlayer);
        }
        if matches!(self.phase, Phase::Terminal { .. }) {
            return Err(GameError::GameOver);
        }
        if !matches!(mv, Move::ExchangeReturn { .. }) && !self.legal_moves(player).contains(&mv) {
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
        out: &mut Vec<Vec<Card>>,
    ) {
        if current.len() == keep_count {
            let mut next = current.clone();
            next.sort_by_key(|card| *card as u8);
            if !out.contains(&next) {
                out.push(next);
            }
            return;
        }
        for index in start..cards.len() {
            current.push(cards[index]);
            go(cards, keep_count, index + 1, current, out);
            current.pop();
        }
    }

    let mut out = Vec::new();
    go(cards, keep_count, 0, &mut Vec::new(), &mut out);
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
}
