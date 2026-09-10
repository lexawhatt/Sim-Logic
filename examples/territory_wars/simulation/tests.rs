use super::*;

fn started(seed: u64) -> Game {
    let mut game = Game::new(seed);
    assert!(game.start((HEIGHT / 2) * WIDTH + WIDTH / 2));
    game
}

fn assert_accounting(game: &Game) {
    let mut counts = [0; FACTIONS];
    let mut land = 0;
    for &owner in game.owners() {
        if owner != WATER {
            land += 1;
        }
        if owner as usize >= FACTIONS {
            assert!(owner == NEUTRAL || owner == WATER);
        } else {
            counts[owner as usize] += 1;
        }
    }
    assert_eq!(land, game.land_count());
    for (faction, &count) in counts.iter().enumerate() {
        assert_eq!(game.factions()[faction].land, count);
        if count == 0 {
            assert_eq!(game.factions()[faction].troops, 0);
            assert_eq!(game.campaign_remaining(faction), 0);
        }
    }
    let armies: u32 = game.factions().iter().map(|faction| faction.troops).sum();
    let deployed: u32 = game
        .campaigns
        .iter()
        .flatten()
        .map(|army| army.remaining)
        .sum();
    assert!(armies + deployed <= CELLS as u32 * CAPACITY_PER_CELL);
    assert!(game.active_campaigns() <= FACTIONS);
}

fn duel() -> Game {
    let mut game = Game::new(17);
    game.owners.fill(WATER);
    for faction in &mut game.factions {
        faction.land = 0;
        faction.troops = 0;
    }
    let start = HEIGHT / 2 * WIDTH + WIDTH / 2;
    for cell in start..start + 10 {
        let faction = usize::from(cell >= start + 5);
        game.owners[cell] = faction as u8;
        game.factions[faction].land += 1;
    }
    game.factions[0].troops = 400;
    game.factions[1].troops = 400;
    game.land_count = 10;
    game.phase = Phase::Running;
    game.bots_enabled = false;
    game
}

fn automatic_player(game: &mut Game) {
    if game.phase() != Phase::Running
        || game.campaign_remaining(0) > 0
        || !game.elapsed_ticks().is_multiple_of(15)
    {
        return;
    }
    if game.borders(0, NEUTRAL) {
        let _ = game.expand(25);
    } else {
        let target = (1..FACTIONS)
            .filter(|&target| game.borders(0, target as u8))
            .min_by_key(|&target| {
                let state = game.factions()[target];
                state.troops / state.land.max(1) as u32
            });
        if let Some(target) = target {
            let _ = game.order(0, target as u8, 35);
        }
    }
}

#[test]
fn terrain_is_connected_bounded_and_varies_with_seed() {
    let mut previous = None;
    for seed in 0..12 {
        let game = Game::new(seed);
        assert!((2_500..5_000).contains(&game.land_count()));
        assert_eq!(game.owners().len(), CELLS);
        let first = game
            .owners()
            .iter()
            .position(|&owner| owner == NEUTRAL)
            .unwrap();
        let mut seen = vec![false; CELLS];
        let mut queue = vec![first];
        seen[first] = true;
        let mut cursor = 0;
        while cursor < queue.len() {
            let cell = queue[cursor];
            cursor += 1;
            for adjacent in neighbors(cell).into_iter().flatten() {
                if !seen[adjacent] && game.owners()[adjacent] == NEUTRAL {
                    seen[adjacent] = true;
                    queue.push(adjacent);
                }
            }
        }
        assert_eq!(queue.len(), game.land_count());
        if let Some(previous) = previous {
            assert_ne!(game.owners, previous);
        }
        previous = Some(game.owners);
    }
}

#[test]
fn start_rejects_water_bad_indices_and_repeated_clicks() {
    let mut game = Game::new(9);
    assert!(!game.start(0));
    assert!(!game.start(usize::MAX));
    game.tick();
    assert_eq!(game.elapsed_ticks(), 0);
    assert_eq!(game.phase(), Phase::Choosing);
    assert!(game.start(HEIGHT / 2 * WIDTH + WIDTH / 2));
    let before = game.factions;
    assert!(!game.start(HEIGHT / 2 * WIDTH + WIDTH / 2 + 1));
    assert_eq!(game.factions, before);
    assert_accounting(&game);
    assert!(
        game.factions
            .iter()
            .all(|faction| faction.land == INITIAL_LAND)
    );
}

#[test]
fn thin_coast_start_still_gives_every_faction_connected_land() {
    let mut game = Game::new(0x51_4d_4c);
    let coast = game
        .owners()
        .iter()
        .position(|&owner| owner == NEUTRAL)
        .unwrap();
    assert!(game.start(coast));
    assert!(
        game.factions
            .iter()
            .all(|faction| faction.land == INITIAL_LAND)
    );
    assert_accounting(&game);
}

#[test]
fn invalid_orders_preserve_every_observable_and_random_state() {
    let mut game = started(3);
    let owners = game.owners.clone();
    let factions = game.factions;
    let diagnostics = game.diagnostics;
    let random = game.random.0;
    for (attacker, target, percent, error) in [
        (usize::MAX, NEUTRAL, 50, OrderError::InvalidFaction),
        (0, WATER, 50, OrderError::InvalidTarget),
        (0, 0, 50, OrderError::InvalidTarget),
        (0, 222, 50, OrderError::InvalidTarget),
        (0, NEUTRAL, 0, OrderError::InvalidPercent),
        (0, NEUTRAL, 101, OrderError::InvalidPercent),
        (0, 1, 50, OrderError::NoBorder),
        (0, NEUTRAL, 1, OrderError::InsufficientTroops),
    ] {
        assert_eq!(game.order(attacker, target, percent), Err(error));
        assert_eq!(game.owners, owners);
        assert_eq!(game.factions, factions);
        assert_eq!(game.diagnostics, diagnostics);
        assert_eq!(game.random.0, random);
        assert_eq!(game.active_campaigns(), 0);
    }
}

#[test]
fn neutral_campaign_conserves_dispatch_minus_exact_cell_costs() {
    let mut game = started(12);
    game.expand(50).unwrap();
    assert_eq!(game.campaign_remaining(0), INITIAL_TROOPS / 2);
    assert_eq!(game.factions[0].troops, INITIAL_TROOPS / 2);
    let before = game.factions[0].land;
    for _ in 0..100 {
        let land = game.factions[0].land;
        game.advance_campaign(0);
        assert!(game.factions[0].land - land <= CAPTURES_PER_TICK);
        if game.campaign_remaining(0) == 0 {
            break;
        }
    }
    let captured = game.factions[0].land - before;
    assert_eq!(
        captured,
        combat::neutral_capture_estimate(INITIAL_TROOPS / 2)
    );
    assert_eq!(
        game.factions[0].troops + captured as u32 * NEUTRAL_COST,
        INITIAL_TROOPS
    );
    assert_eq!(game.campaign_remaining(0), 0);
    assert_accounting(&game);
}

#[test]
fn active_campaign_rejection_does_not_dispatch_twice() {
    let mut game = started(12);
    game.expand(50).unwrap();
    let troops = game.factions[0].troops;
    let campaign = game.campaigns;
    assert_eq!(game.expand(100), Err(OrderError::CampaignActive));
    assert_eq!(game.factions[0].troops, troops);
    assert_eq!(game.campaigns, campaign);
}

#[test]
fn blocked_campaign_returns_surviving_troops() {
    let mut game = duel();
    game.campaigns[0] = Some(Campaign {
        target: NEUTRAL,
        remaining: 120,
    });
    game.factions[0].troops = 180;
    game.advance_campaign(0);
    assert_eq!(game.factions[0].troops, 300);
    assert_eq!(game.campaign_remaining(0), 0);
    assert_accounting(&game);
}

#[test]
fn dispatching_reserves_makes_home_land_easier_to_capture() {
    let mut defended = duel();
    let mut exposed = duel();
    exposed.factions[1].troops = 0;
    defended.order(0, 1, 80).unwrap();
    exposed.order(0, 1, 80).unwrap();
    for _ in 0..10 {
        defended.advance_campaign(0);
        exposed.advance_campaign(0);
    }
    assert!(exposed.factions[0].land > defended.factions[0].land);
    assert_accounting(&defended);
    assert_accounting(&exposed);
}

#[test]
fn eliminating_a_faction_retires_its_deployed_army() {
    let mut game = duel();
    game.factions[1].troops = 0;
    game.campaigns[1] = Some(Campaign {
        target: 0,
        remaining: 100,
    });
    game.order(0, 1, 80).unwrap();
    for _ in 0..10 {
        game.advance_campaign(0);
    }
    assert_eq!(game.factions[1].land, 0);
    assert_eq!(game.campaign_remaining(1), 0);
    assert_eq!(game.diagnostics[1].last_decision, Decision::Eliminated);
    game.check_outcome();
    assert_eq!(game.phase(), Phase::Won);
    assert_accounting(&game);
}

#[test]
fn failed_assault_spends_army_without_creating_land() {
    let mut game = duel();
    game.order(0, 1, 5).unwrap();
    game.advance_campaign(0);
    assert_eq!(game.factions[0].land, 5);
    assert_eq!(game.factions[1].land, 5);
    assert_eq!(game.diagnostics[0].last_spent, 20);
    assert_eq!(game.factions[1].troops, 392);
    assert_accounting(&game);
}

#[test]
fn pure_formulas_cover_capacity_rounding_and_extreme_inputs() {
    let payout = economy::income(500, 10, 100);
    assert_eq!(
        payout,
        economy::IncomeBreakdown {
            territory: 20,
            interest: 20,
            credited: 40,
            capacity: 800
        }
    );
    assert_eq!(economy::income(795, 10, 0).credited, 5);
    assert_eq!(economy::income(900, 10, 0).credited, 0);
    assert_eq!(economy::income(u32::MAX, usize::MAX, u32::MAX).credited, 0);
    assert_eq!(economy::income(1_000, 0, 0).credited, 0);
    assert_eq!(economy::dispatch(99, 50), Some(49));
    assert_eq!(economy::dispatch(u32::MAX, 100), Some(u32::MAX));
    assert_eq!(economy::dispatch(500, 0), None);
    assert_eq!(combat::capture_cost(400, 5), 112);
    assert_eq!(combat::capture_cost(0, 0), NEUTRAL_COST);
    assert_eq!(combat::capture_cost(u32::MAX, 1), u32::MAX);
}

#[test]
fn same_seed_and_orders_replay_exactly_and_reuse_storage() {
    let mut left = started(104);
    let mut right = started(104);
    let owners_pointer = left.owners.as_ptr();
    let frontier_pointer = left.frontier.as_ptr();
    let capacity = left.frontier.capacity();
    for tick in 0..800 {
        automatic_player(&mut left);
        automatic_player(&mut right);
        left.tick();
        right.tick();
        assert_eq!(left.owners, right.owners, "tick {tick}");
        assert_eq!(left.factions, right.factions, "tick {tick}");
        assert_eq!(left.campaigns, right.campaigns, "tick {tick}");
        assert_eq!(left.diagnostics, right.diagnostics, "tick {tick}");
        assert_eq!(left.phase, right.phase, "tick {tick}");
        assert_accounting(&left);
    }
    assert_eq!(left.owners.as_ptr(), owners_pointer);
    assert_eq!(left.frontier.as_ptr(), frontier_pointer);
    assert_eq!(left.frontier.capacity(), capacity);
}

#[test]
fn several_seeded_matches_progress_with_all_bots_and_stay_bounded() {
    for seed in [0, 1, 7, 91, 0x51_4d_4c] {
        let mut game = started(seed);
        for tick in 0..2_400 {
            automatic_player(&mut game);
            game.tick();
            if tick == 200 {
                assert!(
                    game.factions[1..]
                        .iter()
                        .all(|faction| faction.land > INITIAL_LAND)
                );
            }
            if tick % 20 == 0 {
                assert_accounting(&game);
            }
        }
        println!(
            "seed={seed} phase={:?} seconds={} player_land={} land={}",
            game.phase(),
            game.elapsed_ticks() / 10,
            game.factions[0].land,
            game.land_count()
        );
        assert!(game.factions[0].land > INITIAL_LAND || game.phase() == Phase::Lost);
        assert_accounting(&game);
    }
}

#[test]
fn default_map_is_winnable_with_spaced_small_orders_without_cheats() {
    let mut game = started(0x51_4d_4c);
    // At most one click every 1.5 seconds, 25% expansion, then 35% against the
    // weakest adjacent rival. This is a feasibility regression, not a promise
    // about every strategy or an estimate of a human player's match duration.
    for _ in 0..1_800 {
        automatic_player(&mut game);
        game.tick();
        if game.phase() != Phase::Running {
            break;
        }
    }
    assert_eq!(game.phase(), Phase::Won);
    assert!(game.elapsed_ticks() > 600);
    assert!(game.bots_enabled());
    assert_accounting(&game);
}

#[test]
fn bots_have_a_ten_second_grace_but_existing_expeditions_are_not_frozen() {
    let mut game = started(42);
    for _ in 0..99 {
        game.tick();
    }
    assert_eq!(game.active_campaigns(), 0);
    assert!(
        game.factions[1..]
            .iter()
            .all(|faction| faction.land == INITIAL_LAND)
    );
    game.order(1, NEUTRAL, 50).unwrap();
    game.set_bots_enabled(false);
    let before = game.factions[1].land;
    game.tick();
    assert!(game.factions[1].land > before);
    assert!(!game.bots_enabled());
    assert_accounting(&game);
}

#[test]
fn loss_and_win_are_terminal_without_catch_up_or_repeated_orders() {
    let mut game = duel();
    game.factions[0].troops = 0;
    game.order(1, 0, 80).unwrap();
    for _ in 0..10 {
        game.advance_campaign(1);
    }
    game.check_outcome();
    assert_eq!(game.phase(), Phase::Lost);
    let troops = game.factions;
    let ticks = game.elapsed_ticks();
    for _ in 0..100 {
        game.tick();
    }
    assert_eq!(game.factions, troops);
    assert_eq!(game.elapsed_ticks(), ticks);
    assert_eq!(game.order(1, 0, 50), Err(OrderError::NotRunning));
    assert_accounting(&game);
}

#[test]
fn debug_grant_is_capped_and_disabled_bots_do_not_issue_orders() {
    let mut game = started(92);
    let granted = game.grant_troops(0, u32::MAX);
    assert_eq!(
        granted,
        INITIAL_LAND as u32 * CAPACITY_PER_CELL - INITIAL_TROOPS
    );
    assert_eq!(game.grant_troops(0, u32::MAX), 0);
    assert_eq!(game.grant_troops(usize::MAX, 20), 0);
    game.set_bots_enabled(false);
    assert!(!game.bots_enabled());
    for _ in 0..60 {
        game.tick();
    }
    assert!(
        game.factions[1..]
            .iter()
            .all(|faction| faction.land == INITIAL_LAND)
    );
    assert_eq!(game.active_campaigns(), 0);
    game.set_bots_enabled(true);
    for _ in 0..90 {
        game.tick();
    }
    assert!(
        game.factions[1..]
            .iter()
            .all(|faction| faction.land > INITIAL_LAND)
    );
    assert_accounting(&game);
}

#[test]
fn debug_victory_changes_ownership_and_ends_campaigns_consistently() {
    let mut game = started(4);
    game.expand(50).unwrap();
    game.force_player_victory();
    assert_eq!(game.phase(), Phase::Won);
    assert_eq!(game.factions()[0].land, game.land_count());
    assert_eq!(game.active_campaigns(), 0);
    assert_eq!(game.grant_troops(0, 50), 0);
    assert_accounting(&game);
}
