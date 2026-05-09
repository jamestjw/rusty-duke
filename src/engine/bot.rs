use rand::Rng;
use rand::seq::SliceRandom;

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
