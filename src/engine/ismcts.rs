use std::collections::HashMap;

use rand::Rng;

use crate::engine::bot::Bot;
use crate::engine::rollout::random_rollout;
use crate::{GameState, Move, Observation};

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

#[derive(Debug, Clone)]
pub struct IsmctsBot {
    pub config: SearchConfig,
}

impl IsmctsBot {
    pub fn new(config: SearchConfig) -> Self {
        Self { config }
    }
}

impl Default for IsmctsBot {
    fn default() -> Self {
        Self::new(SearchConfig::default())
    }
}

impl Bot for IsmctsBot {
    fn choose_move<R: Rng + ?Sized>(
        &mut self,
        observation: &Observation,
        rng: &mut R,
    ) -> Option<Move> {
        let root_state = GameState::determinize(observation, rng.r#gen()).ok()?;
        let root_player = root_state.active_player()?;
        let root_moves = root_state.legal_moves(root_player);
        if root_moves.is_empty() {
            return None;
        }

        let mut root = RootStats::new(root_moves);
        for _ in 0..self.config.iterations.max(1) {
            let mut state = GameState::determinize(observation, rng.r#gen()).ok()?;
            let Some(player) = state.active_player() else {
                continue;
            };
            if player != root_player {
                continue;
            }

            let legal = state.legal_moves(player);
            root.mark_available(&legal);
            let mv = root.select(&legal, self.config.exploration, rng)?;

            if state.apply_move(player, mv.clone()).is_err() {
                continue;
            }

            let reward = random_rollout(&mut state, root_player, self.config.max_depth, rng);
            root.record(&mv, reward);
        }

        root.best_move()
    }
}

#[derive(Debug, Clone, Default)]
struct MoveStats {
    visits: u32,
    available: u32,
    total_reward: f64,
}

#[derive(Debug, Clone)]
struct RootStats {
    total_visits: u32,
    moves: HashMap<Move, MoveStats>,
}

impl RootStats {
    fn new(moves: Vec<Move>) -> Self {
        Self {
            total_visits: 0,
            moves: moves
                .into_iter()
                .map(|mv| (mv, MoveStats::default()))
                .collect(),
        }
    }

    fn mark_available(&mut self, legal: &[Move]) {
        for mv in legal {
            if let Some(stats) = self.moves.get_mut(mv) {
                stats.available += 1;
            }
        }
    }

    fn select<R: Rng + ?Sized>(
        &self,
        legal: &[Move],
        exploration: f64,
        rng: &mut R,
    ) -> Option<Move> {
        let unvisited: Vec<_> = legal
            .iter()
            .filter(|mv| self.moves.get(*mv).is_some_and(|stats| stats.visits == 0))
            .cloned()
            .collect();
        if !unvisited.is_empty() {
            let index = rng.gen_range(0..unvisited.len());
            return Some(unvisited[index].clone());
        }

        legal.iter().cloned().max_by(|left, right| {
            let left_score = self.ucb_score(left, exploration);
            let right_score = self.ucb_score(right, exploration);
            left_score.total_cmp(&right_score)
        })
    }

    fn ucb_score(&self, mv: &Move, exploration: f64) -> f64 {
        let Some(stats) = self.moves.get(mv) else {
            return f64::NEG_INFINITY;
        };
        if stats.visits == 0 {
            return f64::INFINITY;
        }

        let exploitation = stats.total_reward / stats.visits as f64;
        let available = stats.available.max(1) as f64;
        let exploration = exploration * (available.ln() / stats.visits as f64).sqrt();
        exploitation + exploration
    }

    fn record(&mut self, mv: &Move, reward: f64) {
        let Some(stats) = self.moves.get_mut(mv) else {
            return;
        };
        stats.visits += 1;
        stats.total_reward += reward;
        self.total_visits += 1;
    }

    fn best_move(&self) -> Option<Move> {
        self.moves
            .iter()
            .filter(|(_, stats)| stats.visits > 0)
            .max_by(|(_, left), (_, right)| {
                let left_score = left.total_reward / left.visits as f64;
                let right_score = right.total_reward / right.visits as f64;
                left_score.total_cmp(&right_score)
            })
            .map(|(mv, _)| mv.clone())
            .or_else(|| self.moves.keys().next().cloned())
    }
}
