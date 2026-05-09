use rand::Rng;
use rand::seq::SliceRandom;

use crate::engine::eval::evaluate;
use crate::{GameState, PlayerId};

pub fn random_rollout<R: Rng + ?Sized>(
    state: &mut GameState,
    root_player: PlayerId,
    max_depth: usize,
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
        let Some(mv) = legal.choose(rng).cloned() else {
            break;
        };

        if state.apply_move(player, mv).is_err() {
            break;
        }
    }

    evaluate(state, root_player)
}
