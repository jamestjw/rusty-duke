use rand::Rng;
use rand::seq::SliceRandom;

use crate::engine::eval::evaluate;
use crate::{ActionKind, Card, GameState, Move, Phase, PlayerId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutPolicyKind {
    Random,
    Heuristic(HeuristicProfile),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeuristicProfile {
    Balanced,
    Aggressive,
    Conservative,
    ChallengeHeavy,
    BlockHeavy,
    Economic,
}

impl Default for RolloutPolicyKind {
    fn default() -> Self {
        Self::Random
    }
}

pub trait RolloutPolicy {
    fn choose_move<R: Rng + ?Sized>(
        &self,
        state: &GameState,
        player: PlayerId,
        legal: &[Move],
        rng: &mut R,
    ) -> Option<Move>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RandomRolloutPolicy;

impl RolloutPolicy for RandomRolloutPolicy {
    fn choose_move<R: Rng + ?Sized>(
        &self,
        _state: &GameState,
        _player: PlayerId,
        legal: &[Move],
        rng: &mut R,
    ) -> Option<Move> {
        legal.choose(rng).cloned()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HeuristicRolloutPolicy {
    profile: HeuristicProfile,
}

impl HeuristicRolloutPolicy {
    pub fn new(profile: HeuristicProfile) -> Self {
        Self { profile }
    }
}

impl Default for HeuristicProfile {
    fn default() -> Self {
        Self::Balanced
    }
}

impl RolloutPolicy for HeuristicRolloutPolicy {
    fn choose_move<R: Rng + ?Sized>(
        &self,
        state: &GameState,
        player: PlayerId,
        legal: &[Move],
        rng: &mut R,
    ) -> Option<Move> {
        legal
            .iter()
            .max_by_key(|mv| score_move(state, player, mv, self.profile, rng))
            .cloned()
    }
}

pub fn rollout<R: Rng + ?Sized>(
    state: &mut GameState,
    root_player: PlayerId,
    max_depth: usize,
    policy: RolloutPolicyKind,
    rng: &mut R,
) -> f64 {
    for _ in 0..max_depth {
        if state.is_terminal() {
            break;
        }

        let Some(player) = state.active_player() else {
            break;
        };
        let legal = state.legal_moves(player);
        let mv = choose_rollout_move(state, player, &legal, policy, rng);
        let Some(mv) = mv else {
            break;
        };

        if state.apply_move(player, mv).is_err() {
            break;
        }
    }

    evaluate(state, root_player)
}

pub fn choose_rollout_move<R: Rng + ?Sized>(
    state: &GameState,
    player: PlayerId,
    legal: &[Move],
    policy: RolloutPolicyKind,
    rng: &mut R,
) -> Option<Move> {
    match policy {
        RolloutPolicyKind::Random => RandomRolloutPolicy.choose_move(state, player, legal, rng),
        RolloutPolicyKind::Heuristic(profile) => {
            HeuristicRolloutPolicy::new(profile).choose_move(state, player, legal, rng)
        }
    }
}

fn score_move<R: Rng + ?Sized>(
    state: &GameState,
    player: PlayerId,
    mv: &Move,
    profile: HeuristicProfile,
    rng: &mut R,
) -> i32 {
    let jitter = rng.gen_range(0..4);
    score_move_base(state, player, mv, profile) + jitter
}

fn score_move_base(
    state: &GameState,
    player: PlayerId,
    mv: &Move,
    profile: HeuristicProfile,
) -> i32 {
    match mv {
        Move::Income => 20 + profile.income_bonus(),
        Move::ForeignAid => 28 + profile.economy_bonus(),
        Move::Tax => 45 + profile.economy_bonus() + claim_truth_score(state, player, Card::Duke),
        Move::Exchange => {
            32 + profile.exchange_bonus() + claim_truth_score(state, player, Card::Ambassador)
        }
        Move::Coup { target } => 80 + target_score(state, *target) + profile.attack_bonus(),
        Move::Assassinate { target } => {
            55 + target_score(state, *target)
                + profile.attack_bonus()
                + claim_truth_score(state, player, Card::Assassin)
        }
        Move::Steal { target } => {
            35 + state.players[*target].coins.min(2) as i32 * 8
                + profile.attack_bonus() / 2
                + claim_truth_score(state, player, Card::Captain)
        }
        Move::Challenge => challenge_score(state, player, profile),
        Move::PassChallenge => 25,
        Move::Block { claim } => block_score(state, player, *claim, profile),
        Move::PassBlock => 18,
        Move::RevealInfluence { card_index } => reveal_score(state, player, *card_index),
        Move::ExchangeReturn { keep } => exchange_return_score(keep),
    }
}

impl HeuristicProfile {
    fn attack_bonus(self) -> i32 {
        match self {
            Self::Aggressive => 18,
            Self::Conservative => -12,
            _ => 0,
        }
    }

    fn economy_bonus(self) -> i32 {
        match self {
            Self::Economic => 16,
            Self::Conservative => 8,
            Self::Aggressive => -4,
            _ => 0,
        }
    }

    fn exchange_bonus(self) -> i32 {
        match self {
            Self::Conservative => 10,
            Self::Economic => 6,
            Self::Aggressive => -4,
            _ => 0,
        }
    }

    fn income_bonus(self) -> i32 {
        match self {
            Self::Conservative => 8,
            Self::Economic => 4,
            Self::Aggressive => -4,
            _ => 0,
        }
    }

    fn challenge_bonus(self) -> i32 {
        match self {
            Self::ChallengeHeavy => 25,
            Self::Conservative => -12,
            _ => 0,
        }
    }

    fn block_bonus(self) -> i32 {
        match self {
            Self::BlockHeavy => 25,
            Self::Conservative => 8,
            Self::Aggressive => -8,
            _ => 0,
        }
    }
}

fn target_score(state: &GameState, target: PlayerId) -> i32 {
    let target_state = &state.players[target];
    let influence_bonus = if target_state.hidden_count() == 1 {
        30
    } else {
        10
    };
    influence_bonus + target_state.coins as i32
}

fn claim_truth_score(state: &GameState, player: PlayerId, claim: Card) -> i32 {
    if has_hidden_card(state, player, claim) {
        18
    } else if visible_card_count(state, player, claim) >= 3 {
        -100
    } else {
        -18
    }
}

fn challenge_score(state: &GameState, player: PlayerId, profile: HeuristicProfile) -> i32 {
    match &state.phase {
        Phase::AwaitingChallenge { action, .. } => action
            .kind
            .claim()
            .map(|claim| claim_challenge_score(state, player, claim, action.kind, profile))
            .unwrap_or(0),
        Phase::AwaitingBlockChallenge { block_card, .. } => {
            claim_challenge_score(state, player, *block_card, ActionKind::ForeignAid, profile)
        }
        _ => 0,
    }
}

fn claim_challenge_score(
    state: &GameState,
    player: PlayerId,
    claim: Card,
    action: ActionKind,
    profile: HeuristicProfile,
) -> i32 {
    let visible_count = visible_card_count(state, player, claim);
    if visible_count >= 3 {
        return 100;
    }

    let scarcity_bonus = if visible_count == 2 { 10 } else { 0 };
    let impact = match action {
        ActionKind::Assassinate { .. } => 25,
        ActionKind::Steal { .. } => 15,
        ActionKind::Tax => 10,
        ActionKind::Exchange => 6,
        ActionKind::ForeignAid => 4,
    };
    let risk_penalty = if state.players[player].hidden_count() == 1 {
        20
    } else {
        0
    };

    42 + impact + scarcity_bonus - risk_penalty + profile.challenge_bonus()
}

fn block_score(state: &GameState, player: PlayerId, claim: Card, profile: HeuristicProfile) -> i32 {
    let has_claim = has_hidden_card(state, player, claim);
    let impact = match state.phase {
        Phase::AwaitingBlock { action, .. } => match action.kind {
            ActionKind::Assassinate { .. } => 45,
            ActionKind::Steal { .. } => 30,
            ActionKind::ForeignAid => 12,
            ActionKind::Tax | ActionKind::Exchange => 0,
        },
        _ => 0,
    };

    if has_claim {
        25 + impact + profile.block_bonus()
    } else {
        impact / 3 + profile.block_bonus() / 2
    }
}

fn reveal_score(state: &GameState, player: PlayerId, card_index: usize) -> i32 {
    let Some(influence) = state.players[player].influence.get(card_index) else {
        return i32::MIN;
    };
    if influence.revealed {
        return i32::MIN;
    }

    -card_value(influence.card)
}

fn exchange_return_score(keep: &[Card]) -> i32 {
    keep.iter().copied().map(card_value).sum()
}

fn card_value(card: Card) -> i32 {
    match card {
        Card::Duke => 30,
        Card::Captain => 24,
        Card::Assassin => 22,
        Card::Ambassador => 18,
        Card::Contessa => 16,
    }
}

fn visible_card_count(state: &GameState, player: PlayerId, card: Card) -> usize {
    let own_hidden = state.players[player]
        .influence
        .iter()
        .filter(|influence| !influence.revealed && influence.card == card)
        .count();
    let revealed = state
        .players
        .iter()
        .flat_map(|player| player.influence.iter())
        .filter(|influence| influence.revealed && influence.card == card)
        .count();

    own_hidden + revealed
}

fn has_hidden_card(state: &GameState, player: PlayerId, card: Card) -> bool {
    state.players[player]
        .influence
        .iter()
        .any(|influence| !influence.revealed && influence.card == card)
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;
    use crate::{DeclaredAction, InfluenceCard, PlayerState};

    fn rigged_game(players: Vec<Vec<Card>>) -> GameState {
        let mut game = GameState::new(players.len(), 1).unwrap();
        game.players = players
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
            .collect();
        game
    }

    #[test]
    fn heuristic_policy_only_returns_legal_moves() {
        let game = GameState::new(3, 1).unwrap();
        let player = game.active_player().unwrap();
        let legal = game.legal_moves(player);
        let mut rng = StdRng::seed_from_u64(3);

        let mv = HeuristicRolloutPolicy::new(HeuristicProfile::Balanced)
            .choose_move(&game, player, &legal, &mut rng)
            .unwrap();

        assert!(legal.contains(&mv));
    }

    #[test]
    fn heuristic_blocks_assassination_when_holding_contessa() {
        let mut game = rigged_game(vec![
            vec![Card::Assassin, Card::Duke],
            vec![Card::Contessa, Card::Captain],
        ]);
        game.phase = Phase::AwaitingBlock {
            action: DeclaredAction {
                actor: 0,
                kind: ActionKind::Assassinate { target: 1 },
            },
            responder_index: 0,
        };
        let legal = vec![
            Move::PassBlock,
            Move::Block {
                claim: Card::Contessa,
            },
        ];
        let mut rng = StdRng::seed_from_u64(3);

        let mv = HeuristicRolloutPolicy::new(HeuristicProfile::Balanced)
            .choose_move(&game, 1, &legal, &mut rng)
            .unwrap();

        assert_eq!(
            mv,
            Move::Block {
                claim: Card::Contessa
            }
        );
    }

    #[test]
    fn heuristic_passes_assassination_block_without_contessa() {
        let mut game = rigged_game(vec![
            vec![Card::Assassin, Card::Duke],
            vec![Card::Captain, Card::Captain],
        ]);
        game.phase = Phase::AwaitingBlock {
            action: DeclaredAction {
                actor: 0,
                kind: ActionKind::Assassinate { target: 1 },
            },
            responder_index: 0,
        };
        let legal = vec![
            Move::PassBlock,
            Move::Block {
                claim: Card::Contessa,
            },
        ];
        let mut rng = StdRng::seed_from_u64(3);

        let mv = HeuristicRolloutPolicy::new(HeuristicProfile::Balanced)
            .choose_move(&game, 1, &legal, &mut rng)
            .unwrap();

        assert_eq!(mv, Move::PassBlock);
    }

    #[test]
    fn heuristic_challenges_impossible_claim() {
        let mut game = rigged_game(vec![
            vec![Card::Duke, Card::Captain],
            vec![Card::Assassin, Card::Contessa],
        ]);
        game.players[0].influence.push(InfluenceCard {
            card: Card::Duke,
            revealed: true,
        });
        game.players[1].influence.push(InfluenceCard {
            card: Card::Duke,
            revealed: true,
        });
        game.phase = Phase::AwaitingChallenge {
            action: DeclaredAction {
                actor: 1,
                kind: ActionKind::Tax,
            },
            responder_index: 0,
        };
        let legal = vec![Move::Challenge, Move::PassChallenge];
        let mut rng = StdRng::seed_from_u64(3);

        let mv = HeuristicRolloutPolicy::new(HeuristicProfile::Balanced)
            .choose_move(&game, 0, &legal, &mut rng)
            .unwrap();

        assert_eq!(mv, Move::Challenge);
    }

    #[test]
    fn heuristic_avoids_making_impossible_claim() {
        let mut game = rigged_game(vec![vec![Card::Captain], vec![Card::Captain]]);
        game.players[0].coins = 0;
        game.players[1].coins = 6;
        game.players[0].influence.push(InfluenceCard {
            card: Card::Assassin,
            revealed: true,
        });
        game.players[0].influence.push(InfluenceCard {
            card: Card::Assassin,
            revealed: true,
        });
        game.players[1].influence.push(InfluenceCard {
            card: Card::Assassin,
            revealed: true,
        });
        game.phase = Phase::AwaitingAction { actor: 1 };
        let legal = game.legal_moves(1);
        let mut rng = StdRng::seed_from_u64(3);

        let mv = HeuristicRolloutPolicy::new(HeuristicProfile::Balanced)
            .choose_move(&game, 1, &legal, &mut rng)
            .unwrap();

        assert_ne!(mv, Move::Assassinate { target: 0 });
    }
}
