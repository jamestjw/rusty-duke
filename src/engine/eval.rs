use crate::{GameState, PlayerId};

pub fn evaluate(state: &GameState, root_player: PlayerId) -> f64 {
    if let Some(winner) = state.winner() {
        return if winner == root_player { 1.0 } else { 0.0 };
    }

    let Some(root) = state.players.get(root_player) else {
        return 0.0;
    };

    let root_score = material_score(root.hidden_count(), root.coins);
    let mut opponent_score = 0.0;
    let mut opponent_count = 0.0;
    for (_, opponent) in state
        .players
        .iter()
        .enumerate()
        .filter(|(player, state)| *player != root_player && state.is_alive())
    {
        opponent_score += material_score(opponent.hidden_count(), opponent.coins);
        opponent_count += 1.0;
    }
    let average_opponent_score = if opponent_count > 0.0 {
        opponent_score / opponent_count
    } else {
        0.0
    };

    (0.5 + (root_score - average_opponent_score) / 100.0).clamp(0.0, 1.0)
}

fn material_score(hidden_count: usize, coins: u8) -> f64 {
    let coup_pressure = if coins >= 10 {
        8.0
    } else if coins >= 7 {
        5.0
    } else {
        0.0
    };
    hidden_count as f64 * 10.0 + coins as f64 + coup_pressure
}
