use std::collections::HashMap;

use rand::Rng;

use crate::engine::bot::Bot;
use crate::engine::eval::evaluate;
use crate::engine::rollout::{RolloutPolicyKind, choose_rollout_move, rollout};
use crate::{GameState, Move, Observation};

#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub iterations: usize,
    pub max_depth: usize,
    pub exploration: f64,
    pub rollout_policy: RolloutPolicyKind,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            iterations: 1_000,
            max_depth: 80,
            exploration: 1.4,
            rollout_policy: RolloutPolicyKind::Random,
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
        let root_legal = root_state.legal_moves(root_player);
        if root_legal.is_empty() {
            return None;
        }

        let root_key = InfoSetKey(observation.clone());
        let mut tree = SearchTree::default();

        for _ in 0..self.config.iterations.max(1) {
            let mut state = GameState::determinize(observation, rng.r#gen()).ok()?;
            let _ = self.simulate(&mut tree, &mut state, root_player, 0, rng);
        }

        tree.best_move(&root_key)
            .filter(|mv| root_legal.contains(mv))
            .or_else(|| root_legal.first().cloned())
    }
}

impl IsmctsBot {
    fn simulate<R: Rng + ?Sized>(
        &self,
        tree: &mut SearchTree,
        state: &mut GameState,
        root_player: usize,
        depth: usize,
        rng: &mut R,
    ) -> f64 {
        if state.is_terminal() {
            return evaluate(state, root_player);
        }
        if depth >= self.config.max_depth {
            return evaluate(state, root_player);
        }

        let Some(player) = state.active_player() else {
            return evaluate(state, root_player);
        };
        let legal = state.legal_moves(player);
        if legal.is_empty() {
            return evaluate(state, root_player);
        }

        if player != root_player {
            let Some(mv) =
                choose_rollout_move(state, player, &legal, self.config.rollout_policy, rng)
            else {
                return evaluate(state, root_player);
            };
            if state.apply_move(player, mv).is_err() {
                return evaluate(state, root_player);
            }
            return self.simulate(tree, state, root_player, depth + 1, rng);
        }

        let Ok(observation) = state.observation_for(player) else {
            return evaluate(state, root_player);
        };
        let key = InfoSetKey(observation);

        let selected = tree.select_or_expand(&key, &legal, self.config.exploration, rng);
        let Some(mv) = selected else {
            return evaluate(state, root_player);
        };

        if state.apply_move(player, mv.clone()).is_err() {
            return evaluate(state, root_player);
        }

        let reward = if tree.was_expanded_this_iteration(&key, &mv) {
            let remaining_depth = self.config.max_depth.saturating_sub(depth + 1);
            rollout(
                state,
                root_player,
                remaining_depth,
                self.config.rollout_policy,
                rng,
            )
        } else {
            self.simulate(tree, state, root_player, depth + 1, rng)
        };

        tree.record(&key, &mv, reward);
        reward
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InfoSetKey(Observation);

#[derive(Debug, Clone, Default)]
struct SearchTree {
    nodes: HashMap<InfoSetKey, Node>,
}

impl SearchTree {
    fn select_or_expand<R: Rng + ?Sized>(
        &mut self,
        key: &InfoSetKey,
        legal: &[Move],
        exploration: f64,
        rng: &mut R,
    ) -> Option<Move> {
        let node = self.nodes.entry(key.clone()).or_default();
        node.mark_available(legal);

        let unvisited: Vec<_> = legal
            .iter()
            .filter(|mv| node.edges.get(*mv).is_some_and(|edge| edge.visits == 0))
            .cloned()
            .collect();
        if !unvisited.is_empty() {
            let index = rng.gen_range(0..unvisited.len());
            return Some(unvisited[index].clone());
        }

        node.select(legal, exploration)
    }

    fn was_expanded_this_iteration(&self, key: &InfoSetKey, mv: &Move) -> bool {
        self.nodes
            .get(key)
            .and_then(|node| node.edges.get(mv))
            .is_some_and(|edge| edge.visits == 0)
    }

    fn record(&mut self, key: &InfoSetKey, mv: &Move, reward: f64) {
        let Some(node) = self.nodes.get_mut(key) else {
            return;
        };
        node.visits += 1;
        let Some(edge) = node.edges.get_mut(mv) else {
            return;
        };
        edge.visits += 1;
        edge.total_reward += reward;
    }

    fn best_move(&self, key: &InfoSetKey) -> Option<Move> {
        self.nodes.get(key)?.best_move()
    }
}

#[derive(Debug, Clone, Default)]
struct Node {
    visits: u32,
    edges: HashMap<Move, Edge>,
}

impl Node {
    fn mark_available(&mut self, legal: &[Move]) {
        for mv in legal {
            self.edges.entry(mv.clone()).or_default().available += 1;
        }
    }

    fn select(&self, legal: &[Move], exploration: f64) -> Option<Move> {
        legal.iter().cloned().max_by(|left, right| {
            let left_score = self.ucb_score(left, exploration);
            let right_score = self.ucb_score(right, exploration);
            left_score.total_cmp(&right_score)
        })
    }

    fn ucb_score(&self, mv: &Move, exploration: f64) -> f64 {
        let Some(edge) = self.edges.get(mv) else {
            return f64::NEG_INFINITY;
        };
        if edge.visits == 0 {
            return f64::INFINITY;
        }

        let exploitation = edge.total_reward / edge.visits as f64;
        let available = edge.available.max(1) as f64;
        let exploration = exploration * (available.ln() / edge.visits as f64).sqrt();
        exploitation + exploration
    }

    fn best_move(&self) -> Option<Move> {
        self.edges
            .iter()
            .filter(|(_, edge)| edge.visits > 0)
            .max_by(|(_, left), (_, right)| {
                let left_score = left.total_reward / left.visits as f64;
                let right_score = right.total_reward / right.visits as f64;
                left_score.total_cmp(&right_score)
            })
            .map(|(mv, _)| mv.clone())
    }
}

#[derive(Debug, Clone, Default)]
struct Edge {
    visits: u32,
    available: u32,
    total_reward: f64,
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;
    #[test]
    fn simulation_builds_nodes_beyond_root() {
        let game = GameState::new(3, 1).unwrap();
        let observation = game.observation_for(0).unwrap();
        let bot = IsmctsBot::new(SearchConfig {
            iterations: 1,
            max_depth: 12,
            exploration: 1.4,
            rollout_policy: RolloutPolicyKind::Random,
        });
        let mut tree = SearchTree::default();
        let mut rng = StdRng::seed_from_u64(9);

        for _ in 0..30 {
            let mut state = GameState::determinize(&observation, rng.r#gen()).unwrap();
            bot.simulate(&mut tree, &mut state, 0, 0, &mut rng);
        }

        assert!(tree.nodes.len() > 1);
    }
}
