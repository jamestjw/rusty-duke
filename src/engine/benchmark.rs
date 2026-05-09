use std::time::Instant;

use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::engine::{Bot, HeuristicBot, IsmctsBot, RandomBot, RolloutPolicyKind, SearchConfig};
use crate::{GameState, Move, PlayerId};

#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub games: usize,
    pub player_count: usize,
    pub ismcts_iterations: usize,
    pub max_depth: usize,
    pub rollout_policy: RolloutPolicyKind,
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
            rollout_policy: RolloutPolicyKind::Random,
            max_decisions_per_game: 500,
            seed: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BenchmarkResult {
    pub games: usize,
    pub ismcts_wins: usize,
    pub opponent_wins: usize,
    pub draws_or_timeouts: usize,
    pub decisions: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkBot {
    Random,
    Heuristic(crate::engine::HeuristicProfile),
    Ismcts {
        iterations: usize,
        max_depth: usize,
        rollout_policy: RolloutPolicyKind,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeadToHeadResult {
    pub left: BenchmarkBot,
    pub right: BenchmarkBot,
    pub games: usize,
    pub left_wins: usize,
    pub right_wins: usize,
    pub draws_or_timeouts: usize,
    pub decisions: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct MultiOpponentConfig {
    pub games: usize,
    pub player_count: usize,
    pub ismcts_bot: BenchmarkBot,
    pub opponent_bot: BenchmarkBot,
    pub max_decisions_per_game: usize,
    pub seed: u64,
}

impl HeadToHeadResult {
    pub fn left_win_rate(&self) -> f64 {
        if self.games == 0 {
            return 0.0;
        }
        self.left_wins as f64 / self.games as f64
    }

    pub fn decisive_left_win_rate(&self) -> f64 {
        let decisive = self.left_wins + self.right_wins;
        if decisive == 0 {
            return 0.0;
        }
        self.left_wins as f64 / decisive as f64
    }
}

impl BenchmarkResult {
    pub fn ismcts_win_rate(self) -> f64 {
        if self.games == 0 {
            return 0.0;
        }
        self.ismcts_wins as f64 / self.games as f64
    }

    pub fn decisive_ismcts_win_rate(self) -> f64 {
        let decisive = self.ismcts_wins + self.opponent_wins;
        if decisive == 0 {
            return 0.0;
        }
        self.ismcts_wins as f64 / decisive as f64
    }

    pub fn average_decisions_per_game(self) -> f64 {
        if self.games == 0 {
            return 0.0;
        }
        self.decisions as f64 / self.games as f64
    }

    pub fn average_ms_per_game(self) -> f64 {
        if self.games == 0 {
            return 0.0;
        }
        self.elapsed_ms as f64 / self.games as f64
    }
}

pub fn benchmark_ismcts_vs_random(config: BenchmarkConfig) -> BenchmarkResult {
    benchmark_ismcts_vs_opponents(MultiOpponentConfig {
        games: config.games,
        player_count: config.player_count,
        ismcts_bot: BenchmarkBot::Ismcts {
            iterations: config.ismcts_iterations,
            max_depth: config.max_depth,
            rollout_policy: config.rollout_policy,
        },
        opponent_bot: BenchmarkBot::Random,
        max_decisions_per_game: config.max_decisions_per_game,
        seed: config.seed,
    })
}

pub fn benchmark_ismcts_vs_opponents(config: MultiOpponentConfig) -> BenchmarkResult {
    let mut result = BenchmarkResult {
        games: config.games,
        ismcts_wins: 0,
        opponent_wins: 0,
        draws_or_timeouts: 0,
        decisions: 0,
        elapsed_ms: 0,
    };
    let start = Instant::now();

    for game_index in 0..config.games {
        let game_result = play_multi_opponent_game(&config, game_index as u64);
        result.decisions += game_result.decisions;
        match game_result.winner {
            Some(0) => result.ismcts_wins += 1,
            Some(_) => result.opponent_wins += 1,
            None => result.draws_or_timeouts += 1,
        }
    }
    result.elapsed_ms = start.elapsed().as_millis();

    result
}

pub fn benchmark_head_to_head(
    left: BenchmarkBot,
    right: BenchmarkBot,
    games: usize,
    seed: u64,
    max_decisions_per_game: usize,
) -> HeadToHeadResult {
    let start = Instant::now();
    let mut result = HeadToHeadResult {
        left,
        right,
        games,
        left_wins: 0,
        right_wins: 0,
        draws_or_timeouts: 0,
        decisions: 0,
        elapsed_ms: 0,
    };

    for game_index in 0..games {
        let left_seat = game_index % 2;
        let winner = play_benchmark_game(
            [left, right],
            left_seat,
            seed.wrapping_add(game_index as u64),
            max_decisions_per_game,
        );
        result.decisions += winner.decisions;
        match winner.winner {
            Some(player) if player == left_seat => result.left_wins += 1,
            Some(_) => result.right_wins += 1,
            None => result.draws_or_timeouts += 1,
        }
    }

    result.elapsed_ms = start.elapsed().as_millis();
    result
}

#[derive(Debug, Clone, Copy)]
struct GameBenchmarkResult {
    winner: Option<PlayerId>,
    decisions: usize,
}

fn play_multi_opponent_game(config: &MultiOpponentConfig, game_offset: u64) -> GameBenchmarkResult {
    let game_seed = config.seed.wrapping_add(game_offset.wrapping_mul(2));
    let rng_seed = config
        .seed
        .wrapping_add(game_offset.wrapping_mul(2).wrapping_add(1));
    let Ok(mut game) = GameState::new(config.player_count, game_seed) else {
        return GameBenchmarkResult {
            winner: None,
            decisions: 0,
        };
    };
    let mut rng = StdRng::seed_from_u64(rng_seed);

    for decisions in 0..config.max_decisions_per_game {
        if let Some(winner) = game.winner() {
            return GameBenchmarkResult {
                winner: Some(winner),
                decisions,
            };
        }

        let Some(player) = game.active_player() else {
            return GameBenchmarkResult {
                winner: game.winner(),
                decisions,
            };
        };
        let Ok(observation) = game.observation_for(player) else {
            return GameBenchmarkResult {
                winner: None,
                decisions,
            };
        };
        let bot = if player == 0 {
            config.ismcts_bot
        } else {
            config.opponent_bot
        };
        let mv = choose_benchmark_move(bot, &observation, &mut rng);

        if apply_or_fallback(&mut game, player, mv).is_none() {
            return GameBenchmarkResult {
                winner: None,
                decisions,
            };
        }
    }

    GameBenchmarkResult {
        winner: None,
        decisions: config.max_decisions_per_game,
    }
}

fn apply_or_fallback(game: &mut GameState, player: PlayerId, mv: Option<Move>) -> Option<()> {
    let legal = game.legal_moves(player);
    let mv = mv
        .filter(|mv| legal.contains(mv))
        .or_else(|| legal.first().cloned())?;
    game.apply_move(player, mv).ok()
}

fn play_benchmark_game(
    bots: [BenchmarkBot; 2],
    left_seat: usize,
    seed: u64,
    max_decisions_per_game: usize,
) -> GameBenchmarkResult {
    let Ok(mut game) = GameState::new(2, seed) else {
        return GameBenchmarkResult {
            winner: None,
            decisions: 0,
        };
    };
    let mut rng = StdRng::seed_from_u64(seed.wrapping_add(1));

    for decisions in 0..max_decisions_per_game {
        if let Some(winner) = game.winner() {
            return GameBenchmarkResult {
                winner: Some(winner),
                decisions,
            };
        }

        let Some(player) = game.active_player() else {
            return GameBenchmarkResult {
                winner: game.winner(),
                decisions,
            };
        };
        let Ok(observation) = game.observation_for(player) else {
            return GameBenchmarkResult {
                winner: None,
                decisions,
            };
        };
        let bot_index = if player == left_seat { 0 } else { 1 };
        let mv = choose_benchmark_move(bots[bot_index], &observation, &mut rng);
        if apply_or_fallback(&mut game, player, mv).is_none() {
            return GameBenchmarkResult {
                winner: None,
                decisions,
            };
        }
    }

    GameBenchmarkResult {
        winner: None,
        decisions: max_decisions_per_game,
    }
}

fn choose_benchmark_move<R: rand::Rng + ?Sized>(
    bot: BenchmarkBot,
    observation: &crate::Observation,
    rng: &mut R,
) -> Option<Move> {
    match bot {
        BenchmarkBot::Random => {
            let mut bot = RandomBot;
            bot.choose_move(observation, rng)
        }
        BenchmarkBot::Heuristic(profile) => {
            let mut bot = HeuristicBot::new(profile);
            bot.choose_move(observation, rng)
        }
        BenchmarkBot::Ismcts {
            iterations,
            max_depth,
            rollout_policy,
        } => {
            let mut bot = IsmctsBot::new(SearchConfig {
                iterations,
                max_depth,
                exploration: 1.4,
                rollout_policy,
            });
            bot.choose_move(observation, rng)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::HeuristicProfile;

    #[test]
    fn benchmark_harness_runs_seeded_games() {
        let result = benchmark_ismcts_vs_random(BenchmarkConfig {
            games: 4,
            player_count: 3,
            ismcts_iterations: 5,
            max_depth: 20,
            rollout_policy: RolloutPolicyKind::Random,
            max_decisions_per_game: 300,
            seed: 7,
        });

        assert_eq!(result.games, 4);
        assert_eq!(
            result.ismcts_wins + result.opponent_wins + result.draws_or_timeouts,
            4
        );
        assert!(result.decisions > 0);
    }

    #[test]
    fn head_to_head_harness_runs_seeded_games() {
        let result = benchmark_head_to_head(
            BenchmarkBot::Heuristic(HeuristicProfile::Balanced),
            BenchmarkBot::Random,
            4,
            31,
            300,
        );

        assert_eq!(result.games, 4);
        assert_eq!(
            result.left_wins + result.right_wins + result.draws_or_timeouts,
            4
        );
        assert!(result.decisions > 0);
    }

    #[test]
    fn multi_opponent_harness_runs_seeded_games() {
        let result = benchmark_ismcts_vs_opponents(MultiOpponentConfig {
            games: 4,
            player_count: 4,
            ismcts_bot: BenchmarkBot::Ismcts {
                iterations: 5,
                max_depth: 20,
                rollout_policy: RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
            },
            opponent_bot: BenchmarkBot::Random,
            max_decisions_per_game: 300,
            seed: 41,
        });

        assert_eq!(result.games, 4);
        assert_eq!(
            result.ismcts_wins + result.opponent_wins + result.draws_or_timeouts,
            4
        );
        assert!(result.decisions > 0);
    }

    #[test]
    #[ignore = "benchmark: run with `cargo test ismcts_iteration_benchmark -- --ignored --nocapture`"]
    fn ismcts_iteration_benchmark() {
        for iterations in [1, 10, 25, 50, 100, 200] {
            let result = benchmark_ismcts_vs_random(BenchmarkConfig {
                games: 100,
                player_count: 3,
                ismcts_iterations: iterations,
                max_depth: 80,
                rollout_policy: RolloutPolicyKind::Random,
                max_decisions_per_game: 500,
                seed: 11,
            });

            println!(
                "iterations={iterations:>3} games={} ismcts_wins={} opponent_wins={} timeouts={} win_rate={:.2} decisive={:.2} avg_decisions={:.1} avg_ms={:.1}",
                result.games,
                result.ismcts_wins,
                result.opponent_wins,
                result.draws_or_timeouts,
                result.ismcts_win_rate(),
                result.decisive_ismcts_win_rate(),
                result.average_decisions_per_game(),
                result.average_ms_per_game(),
            );
        }
    }

    #[test]
    #[ignore = "benchmark: run with `cargo test rollout_policy_benchmark -- --ignored --nocapture`"]
    fn rollout_policy_benchmark() {
        for rollout_policy in [
            RolloutPolicyKind::Random,
            RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
        ] {
            let result = benchmark_ismcts_vs_random(BenchmarkConfig {
                games: 100,
                player_count: 3,
                ismcts_iterations: 100,
                max_depth: 80,
                rollout_policy,
                max_decisions_per_game: 500,
                seed: 17,
            });

            println!(
                "rollout={rollout_policy:?} games={} ismcts_wins={} opponent_wins={} timeouts={} win_rate={:.2} decisive={:.2} avg_decisions={:.1} avg_ms={:.1}",
                result.games,
                result.ismcts_wins,
                result.opponent_wins,
                result.draws_or_timeouts,
                result.ismcts_win_rate(),
                result.decisive_ismcts_win_rate(),
                result.average_decisions_per_game(),
                result.average_ms_per_game(),
            );
        }
    }

    #[test]
    #[ignore = "benchmark: run with `cargo test heuristic_profile_benchmark -- --ignored --nocapture`"]
    fn heuristic_profile_benchmark() {
        for profile in [
            HeuristicProfile::Balanced,
            HeuristicProfile::Aggressive,
            HeuristicProfile::Conservative,
            HeuristicProfile::ChallengeHeavy,
            HeuristicProfile::BlockHeavy,
            HeuristicProfile::Economic,
        ] {
            let rollout_policy = RolloutPolicyKind::Heuristic(profile);
            let result = benchmark_ismcts_vs_random(BenchmarkConfig {
                games: 250,
                player_count: 3,
                ismcts_iterations: 100,
                max_depth: 80,
                rollout_policy,
                max_decisions_per_game: 500,
                seed: 23,
            });

            println!(
                "profile={profile:?} games={} ismcts_wins={} opponent_wins={} timeouts={} win_rate={:.2} decisive={:.2} avg_decisions={:.1} avg_ms={:.1}",
                result.games,
                result.ismcts_wins,
                result.opponent_wins,
                result.draws_or_timeouts,
                result.ismcts_win_rate(),
                result.decisive_ismcts_win_rate(),
                result.average_decisions_per_game(),
                result.average_ms_per_game(),
            );
        }
    }

    #[test]
    #[ignore = "benchmark: run with `cargo test round_robin_benchmark -- --ignored --nocapture`"]
    fn round_robin_benchmark() {
        let bots = [
            BenchmarkBot::Random,
            BenchmarkBot::Heuristic(HeuristicProfile::Balanced),
            BenchmarkBot::Heuristic(HeuristicProfile::BlockHeavy),
            BenchmarkBot::Ismcts {
                iterations: 100,
                max_depth: 80,
                rollout_policy: RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
            },
        ];

        for left_index in 0..bots.len() {
            for right_index in (left_index + 1)..bots.len() {
                let result = benchmark_head_to_head(
                    bots[left_index],
                    bots[right_index],
                    100,
                    101 + (left_index * 17 + right_index) as u64,
                    500,
                );

                println!(
                    "left={:?} right={:?} games={} left_wins={} right_wins={} timeouts={} left_rate={:.2} decisive={:.2}",
                    result.left,
                    result.right,
                    result.games,
                    result.left_wins,
                    result.right_wins,
                    result.draws_or_timeouts,
                    result.left_win_rate(),
                    result.decisive_left_win_rate(),
                );
            }
        }
    }

    #[test]
    #[ignore = "benchmark: run with `cargo test four_player_random_benchmark -- --ignored --nocapture`"]
    fn four_player_random_benchmark() {
        let result = benchmark_ismcts_vs_opponents(MultiOpponentConfig {
            games: 250,
            player_count: 4,
            ismcts_bot: BenchmarkBot::Ismcts {
                iterations: 100,
                max_depth: 80,
                rollout_policy: RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
            },
            opponent_bot: BenchmarkBot::Random,
            max_decisions_per_game: 700,
            seed: 211,
        });

        println!(
            "4p opponents=Random games={} ismcts_wins={} opponent_wins={} timeouts={} win_rate={:.2} decisive={:.2} avg_decisions={:.1} avg_ms={:.1}",
            result.games,
            result.ismcts_wins,
            result.opponent_wins,
            result.draws_or_timeouts,
            result.ismcts_win_rate(),
            result.decisive_ismcts_win_rate(),
            result.average_decisions_per_game(),
            result.average_ms_per_game(),
        );
    }

    #[test]
    #[ignore = "benchmark: run with `cargo test four_player_heuristic_benchmark -- --ignored --nocapture`"]
    fn four_player_heuristic_benchmark() {
        let result = benchmark_ismcts_vs_opponents(MultiOpponentConfig {
            games: 250,
            player_count: 4,
            ismcts_bot: BenchmarkBot::Ismcts {
                iterations: 100,
                max_depth: 80,
                rollout_policy: RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
            },
            opponent_bot: BenchmarkBot::Heuristic(HeuristicProfile::Balanced),
            max_decisions_per_game: 700,
            seed: 307,
        });

        println!(
            "4p opponents=Heuristic(Balanced) games={} ismcts_wins={} opponent_wins={} timeouts={} win_rate={:.2} decisive={:.2} avg_decisions={:.1} avg_ms={:.1}",
            result.games,
            result.ismcts_wins,
            result.opponent_wins,
            result.draws_or_timeouts,
            result.ismcts_win_rate(),
            result.decisive_ismcts_win_rate(),
            result.average_decisions_per_game(),
            result.average_ms_per_game(),
        );
    }
}
