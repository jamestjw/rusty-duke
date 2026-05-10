use rusty_duke::engine::{
    benchmark_head_to_head, benchmark_ismcts_vs_opponents, BenchmarkBot, HeuristicProfile,
    MultiOpponentConfig, RolloutPolicyKind,
};

fn main() {
    let ismcts_balanced = BenchmarkBot::Ismcts {
        iterations: 60,
        max_depth: 60,
        rollout_policy: RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
    };

    let four_random = benchmark_ismcts_vs_opponents(MultiOpponentConfig {
        games: 80,
        player_count: 4,
        tracked_bot: ismcts_balanced,
        opponent_bot: BenchmarkBot::Random,
        max_decisions_per_game: 1_500,
        seed: 10_001,
    });
    let four_heuristic = benchmark_ismcts_vs_opponents(MultiOpponentConfig {
        games: 60,
        player_count: 4,
        tracked_bot: ismcts_balanced,
        opponent_bot: BenchmarkBot::Heuristic(HeuristicProfile::Balanced),
        max_decisions_per_game: 1_500,
        seed: 20_001,
    });
    let h2h = benchmark_head_to_head(
        ismcts_balanced,
        BenchmarkBot::Heuristic(HeuristicProfile::Balanced),
        80,
        30_001,
        800,
    );

    let strength_score = four_random.tracked_wins + four_heuristic.tracked_wins + h2h.left_wins;
    let games = four_random.games + four_heuristic.games + h2h.games;
    let timeouts = four_random.draws_or_timeouts + four_heuristic.draws_or_timeouts + h2h.draws_or_timeouts;
    let decisions = four_random.decisions + four_heuristic.decisions + h2h.decisions;
    let elapsed_ms = four_random.elapsed_ms + four_heuristic.elapsed_ms + h2h.elapsed_ms;

    eprintln!(
        "4p_random wins={} opp={} timeouts={} decisive={:.3}",
        four_random.tracked_wins,
        four_random.opponent_wins,
        four_random.draws_or_timeouts,
        four_random.decisive_tracked_win_rate()
    );
    eprintln!(
        "4p_heuristic wins={} opp={} timeouts={} decisive={:.3}",
        four_heuristic.tracked_wins,
        four_heuristic.opponent_wins,
        four_heuristic.draws_or_timeouts,
        four_heuristic.decisive_tracked_win_rate()
    );
    eprintln!(
        "h2h_heuristic wins={} opp={} timeouts={} decisive={:.3}",
        h2h.left_wins,
        h2h.right_wins,
        h2h.draws_or_timeouts,
        h2h.decisive_left_win_rate()
    );

    println!("METRIC strength_score={strength_score}");
    println!("METRIC games={games}");
    println!("METRIC timeouts={timeouts}");
    println!("METRIC decisions={decisions}");
    println!("METRIC bench_elapsed_ms={elapsed_ms}");
}
