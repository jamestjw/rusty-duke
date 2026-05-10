use rand::SeedableRng;
use rand::rngs::StdRng;
use rusty_duke::GameState;
use rusty_duke::engine::eval::evaluate;
use rusty_duke::engine::rollout::rollout;
use rusty_duke::engine::{
    Bot, HeuristicBot, HeuristicProfile, IsmctsBot, RolloutPolicyKind, SearchConfig,
};
use rusty_duke::{ActionKind, Card, DeclaredAction, Move, Observation, ObservedPlayer, Phase};

fn base_players() -> Vec<ObservedPlayer> {
    vec![
        ObservedPlayer {
            coins: 2,
            hidden_influence: 1,
            revealed: vec![Card::Contessa],
            alive: true,
        },
        ObservedPlayer {
            coins: 4,
            hidden_influence: 1,
            revealed: vec![Card::Contessa],
            alive: true,
        },
        ObservedPlayer {
            coins: 0,
            hidden_influence: 0,
            revealed: vec![Card::Assassin, Card::Assassin],
            alive: false,
        },
        ObservedPlayer {
            coins: 0,
            hidden_influence: 0,
            revealed: vec![Card::Assassin, Card::Ambassador],
            alive: false,
        },
        ObservedPlayer {
            coins: 0,
            hidden_influence: 0,
            revealed: vec![Card::Ambassador, Card::Ambassador],
            alive: false,
        },
        ObservedPlayer {
            coins: 0,
            hidden_influence: 0,
            revealed: vec![Card::Duke, Card::Duke],
            alive: false,
        },
    ]
}

fn obs(phase: Phase) -> Observation {
    Observation {
        viewer: 0,
        players: base_players(),
        own_hidden_cards: vec![Card::Captain],
        deck_size: 3,
        current_player: Some(0),
        phase,
    }
}

fn main() {
    let action = DeclaredAction {
        actor: 1,
        kind: ActionKind::Steal { target: 0 },
    };
    let challenge_obs = obs(Phase::AwaitingChallenge {
        action,
        responder_index: 0,
    });
    let block_obs = obs(Phase::AwaitingBlock {
        action,
        responder_index: 0,
    });

    for profile in [
        HeuristicProfile::Balanced,
        HeuristicProfile::Conservative,
        HeuristicProfile::ChallengeHeavy,
        HeuristicProfile::BlockHeavy,
    ] {
        let mut bot = HeuristicBot::new(profile);
        let mut rng = StdRng::seed_from_u64(1);
        println!(
            "heuristic {profile:?} challenge: {:?}",
            bot.choose_move(&challenge_obs, &mut rng)
        );

        let mut rng = StdRng::seed_from_u64(1);
        println!(
            "heuristic {profile:?} block: {:?}",
            bot.choose_move(&block_obs, &mut rng)
        );
    }

    for seed in 1..=10 {
        let mut bot = IsmctsBot::new(SearchConfig {
            iterations: 50_000,
            max_depth: 80,
            exploration: 1.4,
            rollout_policy: RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
        });
        let mut rng = StdRng::seed_from_u64(seed);
        let challenge = bot.choose_move(&challenge_obs, &mut rng);

        let mut bot = IsmctsBot::new(SearchConfig {
            iterations: 50_000,
            max_depth: 80,
            exploration: 1.4,
            rollout_policy: RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
        });
        let mut rng = StdRng::seed_from_u64(seed);
        let block = bot.choose_move(&block_obs, &mut rng);

        println!("ismcts seed {seed}: challenge={challenge:?} block={block:?}");
    }

    println!("\nChallenge decision EV:");
    for mv in [Move::Challenge, Move::PassChallenge] {
        let mut sum_eval = 0.0;
        let mut sum_rollout = 0.0;
        let mut wins = 0;
        let mut losses = 0;

        for seed in 1..=1000 {
            let mut state = GameState::determinize(&challenge_obs, seed).unwrap();
            state.apply_move(0, mv.clone()).unwrap();

            if state.is_terminal() {
                if state.winner() == Some(0) {
                    wins += 1;
                } else {
                    losses += 1;
                }
            }

            sum_eval += evaluate(&state, 0);

            let mut rng = StdRng::seed_from_u64(seed + 20_000);
            sum_rollout += rollout(
                &mut state,
                0,
                80,
                RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
                &mut rng,
            );
        }

        println!(
            "after {mv:?}: eval_avg={} rollout_avg={} terminal_wins={} terminal_losses={}",
            sum_eval / 1000.0,
            sum_rollout / 1000.0,
            wins,
            losses
        );
    }

    println!("\nChallenge sampled outcomes:");
    for mv in [Move::Challenge, Move::PassChallenge] {
        println!("{mv:?}:");
        for seed in 1..=20 {
            let mut state = GameState::determinize(&challenge_obs, seed).unwrap();
            let opponent_hidden = state.players[1]
                .influence
                .iter()
                .find(|card| !card.revealed)
                .map(|card| card.card)
                .unwrap();
            state.apply_move(0, mv.clone()).unwrap();

            let mut rng = StdRng::seed_from_u64(seed + 20_000);
            let score = rollout(
                &mut state,
                0,
                80,
                RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
                &mut rng,
            );
            println!("seed {seed}: opponent_hidden={opponent_hidden:?} score={score}");
        }
    }

    for mv in [
        Move::PassBlock,
        Move::Block { claim: Card::Captain },
        Move::Block {
            claim: Card::Ambassador,
        },
    ] {
        let mut sum_eval = 0.0;
        let mut sum_rollout = 0.0;
        for seed in 1..=1000 {
            let mut state = GameState::determinize(&block_obs, seed).unwrap();
            state.apply_move(0, mv.clone()).unwrap();
            sum_eval += evaluate(&state, 0);

            let mut rng = StdRng::seed_from_u64(seed + 10_000);
            sum_rollout += rollout(
                &mut state,
                0,
                80,
                RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
                &mut rng,
            );
        }
        println!(
            "after {mv:?}: eval_avg={} rollout_avg={}",
            sum_eval / 1000.0,
            sum_rollout / 1000.0
        );
    }

    println!("\nPassBlock sampled outcomes:");
    for seed in 1..=20 {
        let mut state = GameState::determinize(&block_obs, seed).unwrap();
        let opponent_hidden = state.players[1]
            .influence
            .iter()
            .find(|card| !card.revealed)
            .map(|card| card.card)
            .unwrap();
        state.apply_move(0, Move::PassBlock).unwrap();

        let mut rng = StdRng::seed_from_u64(seed + 10_000);
        let score = rollout(
            &mut state,
            0,
            80,
            RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
            &mut rng,
        );
        println!("seed {seed}: opponent_hidden={opponent_hidden:?} score={score}");
    }

    println!("\nTrace seed 4 after PassBlock:");
    let mut state = GameState::determinize(&block_obs, 4).unwrap();
    state.apply_move(0, Move::PassBlock).unwrap();
    let mut rng = StdRng::seed_from_u64(10_004);
    for step in 0..30 {
        if state.is_terminal() {
            println!("step {step}: terminal winner={:?}", state.winner());
            break;
        }
        let Some(player) = state.active_player() else {
            println!("step {step}: no active player");
            break;
        };
        let legal = state.legal_moves(player);
        let mv = rusty_duke::engine::rollout::choose_rollout_move(
            &state,
            player,
            &legal,
            RolloutPolicyKind::Heuristic(HeuristicProfile::Balanced),
            &mut rng,
        )
        .unwrap();
        println!(
            "step {step}: player={player} coins=({},{}) phase={:?} move={mv:?}",
            state.players[0].coins, state.players[1].coins, state.phase
        );
        state.apply_move(player, mv).unwrap();
    }
}
