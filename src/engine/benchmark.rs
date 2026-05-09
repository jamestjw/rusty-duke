use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::engine::{Bot, IsmctsBot, RandomBot, SearchConfig};
use crate::{GameState, Move, PlayerId};

#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub games: usize,
    pub player_count: usize,
    pub ismcts_iterations: usize,
    pub max_depth: usize,
    pub max_decisions_per_game: usize,
    pub seed: u64,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            games: 100,
            player_count: 3,
            ismcts_iterations: 100,
            max_depth: 80,
            max_decisions_per_game: 500,
            seed: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BenchmarkResult {
    pub games: usize,
    pub ismcts_wins: usize,
    pub random_wins: usize,
    pub draws_or_timeouts: usize,
}

impl BenchmarkResult {
    pub fn ismcts_win_rate(self) -> f64 {
        if self.games == 0 {
            return 0.0;
        }
        self.ismcts_wins as f64 / self.games as f64
    }
}

pub fn benchmark_ismcts_vs_random(config: BenchmarkConfig) -> BenchmarkResult {
    let mut result = BenchmarkResult {
        games: config.games,
        ismcts_wins: 0,
        random_wins: 0,
        draws_or_timeouts: 0,
    };

    for game_index in 0..config.games {
        match play_ismcts_vs_random_game(&config, game_index as u64) {
            Some(0) => result.ismcts_wins += 1,
            Some(_) => result.random_wins += 1,
            None => result.draws_or_timeouts += 1,
        }
    }

    result
}

fn play_ismcts_vs_random_game(config: &BenchmarkConfig, game_offset: u64) -> Option<PlayerId> {
    let game_seed = config.seed.wrapping_add(game_offset.wrapping_mul(2));
    let rng_seed = config
        .seed
        .wrapping_add(game_offset.wrapping_mul(2).wrapping_add(1));
    let mut game = GameState::new(config.player_count, game_seed).ok()?;
    let mut rng = StdRng::seed_from_u64(rng_seed);
    let mut ismcts = IsmctsBot::new(SearchConfig {
        iterations: config.ismcts_iterations,
        max_depth: config.max_depth,
        exploration: 1.4,
    });
    let mut random = RandomBot;

    for _ in 0..config.max_decisions_per_game {
        if let Some(winner) = game.winner() {
            return Some(winner);
        }

        let Some(player) = game.active_player() else {
            return game.winner();
        };
        let observation = game.observation_for(player).ok()?;
        let mv = if player == 0 {
            ismcts.choose_move(&observation, &mut rng)
        } else {
            random.choose_move(&observation, &mut rng)
        };

        apply_or_fallback(&mut game, player, mv)?;
    }

    None
}

fn apply_or_fallback(game: &mut GameState, player: PlayerId, mv: Option<Move>) -> Option<()> {
    let legal = game.legal_moves(player);
    let mv = mv
        .filter(|mv| legal.contains(mv))
        .or_else(|| legal.first().cloned())?;
    game.apply_move(player, mv).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_harness_runs_seeded_games() {
        let result = benchmark_ismcts_vs_random(BenchmarkConfig {
            games: 4,
            player_count: 3,
            ismcts_iterations: 5,
            max_depth: 20,
            max_decisions_per_game: 300,
            seed: 7,
        });

        assert_eq!(result.games, 4);
        assert_eq!(
            result.ismcts_wins + result.random_wins + result.draws_or_timeouts,
            4
        );
    }

    #[test]
    #[ignore = "benchmark: run with `cargo test ismcts_iteration_benchmark -- --ignored --nocapture`"]
    fn ismcts_iteration_benchmark() {
        for iterations in [1, 10, 25, 50, 100, 200] {
            let result = benchmark_ismcts_vs_random(BenchmarkConfig {
                games: 250,
                player_count: 3,
                ismcts_iterations: iterations,
                max_depth: 80,
                max_decisions_per_game: 500,
                seed: 11,
            });

            println!(
                "iterations={iterations:>3} games={} ismcts_wins={} random_wins={} timeouts={} win_rate={:.2}",
                result.games,
                result.ismcts_wins,
                result.random_wins,
                result.draws_or_timeouts,
                result.ismcts_win_rate(),
            );
        }
    }
}
