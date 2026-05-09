use crate::{GameState, PlayerId};

pub fn evaluate(state: &GameState, root_player: PlayerId) -> f64 {
    if let Some(winner) = state.winner() {
        return if winner == root_player { 1.0 } else { 0.0 };
    }

    let Some(root) = state.players.get(root_player) else {
        return 0.0;
    };

    let root_score = root.hidden_count() as f64 * 10.0 + root.coins as f64;
    let opponent_score = state
        .players
        .iter()
        .enumerate()
        .filter(|(player, state)| *player != root_player && state.is_alive())
        .map(|(_, state)| state.hidden_count() as f64 * 10.0 + state.coins as f64)
        .sum::<f64>();

    (0.5 + (root_score - opponent_score / 2.0) / 100.0).clamp(0.0, 1.0)
}
