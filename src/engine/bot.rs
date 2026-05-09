use rand::Rng;
use rand::seq::SliceRandom;

use crate::engine::rollout::{HeuristicProfile, HeuristicRolloutPolicy, RolloutPolicy};
use crate::{GameState, Move, Observation};

pub trait Bot {
    fn choose_move<R: Rng + ?Sized>(
        &mut self,
        observation: &Observation,
        rng: &mut R,
    ) -> Option<Move>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RandomBot;

impl Bot for RandomBot {
    fn choose_move<R: Rng + ?Sized>(
        &mut self,
        observation: &Observation,
        rng: &mut R,
    ) -> Option<Move> {
        let state = GameState::determinize(observation, rng.r#gen()).ok()?;
        let player = state.active_player()?;
        state.legal_moves(player).choose(rng).cloned()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HeuristicBot {
    pub profile: HeuristicProfile,
}

impl HeuristicBot {
    pub fn new(profile: HeuristicProfile) -> Self {
        Self { profile }
    }
}

impl Default for HeuristicBot {
    fn default() -> Self {
        Self::new(HeuristicProfile::Balanced)
    }
}

impl Bot for HeuristicBot {
    fn choose_move<R: Rng + ?Sized>(
        &mut self,
        observation: &Observation,
        rng: &mut R,
    ) -> Option<Move> {
        let state = GameState::determinize(observation, rng.r#gen()).ok()?;
        let player = state.active_player()?;
        let legal = state.legal_moves(player);
        HeuristicRolloutPolicy::new(self.profile).choose_move(&state, player, &legal, rng)
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    #[test]
    fn heuristic_bot_returns_legal_move() {
        let game = GameState::new(3, 1).unwrap();
        let observation = game.observation_for(0).unwrap();
        let mut bot = HeuristicBot::default();
        let mut rng = StdRng::seed_from_u64(5);

        let mv = bot.choose_move(&observation, &mut rng).unwrap();

        assert!(game.legal_moves(0).contains(&mv));
    }
}
