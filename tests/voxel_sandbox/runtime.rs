use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use sim_logic::prelude::*;

use super::{app, model, projection, scene, view};

const STEP: Duration = Duration::from_millis(20);
const GAME: (f32, f32) = (550.0, 360.0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> LogicResult<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sim-logic-voxel-redteam-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn save(&self) -> PathBuf {
        self.0.join("world.save")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn application(path: PathBuf) -> LogicResult<(Application<app::Action>, WorldFactoryId)> {
    app::build_application(TimeConfig::new(STEP, 4)?, path)
}

fn runner(path: PathBuf) -> LogicResult<HeadlessRunner<app::Action>> {
    let (application, initial) = application(path)?;
    Ok(application.build_headless(initial)?)
}

fn viewport() -> LogicResult<LogicalViewport> {
    Ok(LogicalViewport::new(1100.0, 720.0)?)
}

fn pointer((x, y): (f32, f32)) -> LogicResult<InputEvent> {
    Ok(InputEvent::pointer_moved(PointerSample::new(
        LogicalScreenPosition::new(x, y),
        viewport()?,
    )?))
}

fn key(key: PhysicalKeyCode) -> [InputEvent; 2] {
    [
        InputEvent::key(key, ButtonState::Pressed),
        InputEvent::key(key, ButtonState::Released),
    ]
}

fn click(position: (f32, f32), button: MouseButton) -> LogicResult<[InputEvent; 3]> {
    Ok([
        pointer(position)?,
        InputEvent::mouse_button(button, ButtonState::Pressed),
        InputEvent::mouse_button(button, ButtonState::Released),
    ])
}

fn button_position(button: view::Button) -> LogicResult<(f32, f32)> {
    let [x, y, width, height] = view::Layout::new(viewport()?).panel(view::Panel::Button(button));
    Ok((x + width * 0.5, y + height * 0.5))
}

fn report(
    runner: &mut HeadlessRunner<app::Action>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(elapsed, events, viewport()?))
    else {
        return Err("voxel frame rejected before execution".into());
    };
    Ok(report)
}

fn advance(
    runner: &mut HeadlessRunner<app::Action>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    let report = report(runner, elapsed, events)?;
    assert!(report.failure().is_none(), "{:?}", report.failure());
    Ok(report)
}

fn session(runner: &HeadlessRunner<app::Action>) -> LogicResult<&app::Session> {
    runner
        .app_resource::<app::Session>()
        .ok_or_else(|| "session".into())
}

fn local(runner: &HeadlessRunner<app::Action>) -> LogicResult<&scene::Local> {
    runner
        .resource::<scene::Local>()
        .ok_or_else(|| "local region".into())
}

fn ready(runner: &mut HeadlessRunner<app::Action>) -> LogicResult {
    for _ in 0..3 {
        if local(runner)?.phase == scene::Phase::Ready {
            return Ok(());
        }
        advance(runner, Duration::ZERO, &[])?;
    }
    assert_eq!(local(runner)?.phase, scene::Phase::Ready);
    Ok(())
}

fn aim_at_ground(runner: &mut HeadlessRunner<app::Action>) -> LogicResult {
    advance(
        runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::ArrowDown,
            ButtonState::Pressed,
        )],
    )?;
    for _ in 0..5 {
        advance(runner, STEP * 4, &[])?;
    }
    advance(
        runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::ArrowDown,
            ButtonState::Released,
        )],
    )?;
    assert!(session(runner)?.game.target().is_some());
    Ok(())
}

fn break_one(runner: &mut HeadlessRunner<app::Action>) -> LogicResult<([i32; 3], model::Block)> {
    aim_at_ground(runner)?;
    let hit = session(runner)?.game.target().ok_or("ground target")?;
    let block = session(runner)?.game.active_region().get(hit.cell);
    let edits = session(runner)?.edits;
    advance(runner, Duration::ZERO, &click(GAME, MouseButton::Left)?)?;
    assert_eq!(session(runner)?.edits, edits + 1);
    assert_eq!(
        session(runner)?.game.active_region().get(hit.cell),
        model::Block::Air
    );
    Ok((hit.cell, block))
}

fn travel(runner: &mut HeadlessRunner<app::Action>) -> LogicResult {
    let generation = runner.world_generation();
    let report = advance(runner, Duration::ZERO, &key(PhysicalKeyCode::KeyN))?;
    assert!(matches!(
        report.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_ne!(runner.world_generation(), generation);
    assert_eq!(local(runner)?.phase, scene::Phase::Loading);
    ready(runner)
}

#[test]
fn loading_rejects_game_actions_and_idle_frames_share_mesh_storage() -> LogicResult {
    let scratch = Scratch::new()?;
    let mut runner = runner(scratch.save())?;
    assert_eq!(local(&runner)?.phase, scene::Phase::Loading);
    assert!(!runner.resource::<View3d>().ok_or("view")?.enabled());
    assert_eq!(runner.components::<projection::ChunkPart>().count(), 0);
    advance(
        &mut runner,
        Duration::ZERO,
        &click(GAME, MouseButton::Left)?,
    )?;
    assert_eq!(session(&runner)?.edits, 0);
    assert_eq!(local(&runner)?.phase, scene::Phase::Queued);
    assert!(!runner.resource::<View3d>().ok_or("view")?.enabled());
    ready(&mut runner)?;
    assert!(runner.resource::<View3d>().ok_or("view")?.enabled());
    assert_eq!(local(&runner)?.rebuilt_chunks, model::CHUNK_COUNT as u64);
    let assets: Vec<_> = runner
        .components::<MeshVisual3d>()
        .map(|(entity, visual)| (entity, visual.asset().clone()))
        .collect();
    assert!(!assets.is_empty());
    for _ in 0..3 {
        advance(&mut runner, Duration::ZERO, &[])?;
    }
    assert_eq!(local(&runner)?.rebuilt_chunks, model::CHUNK_COUNT as u64);
    for (entity, asset) in assets {
        assert!(
            runner
                .component::<MeshVisual3d>(entity)?
                .asset()
                .shares_storage(&asset)
        );
    }
    Ok(())
}

#[test]
fn edits_rebuild_only_dirty_chunks_and_survive_both_directions_of_travel() -> LogicResult {
    let scratch = Scratch::new()?;
    let mut runner = runner(scratch.save())?;
    ready(&mut runner)?;
    let revisions: [_; model::CHUNK_COUNT] = std::array::from_fn(|chunk| {
        session(&runner)
            .unwrap()
            .game
            .active_region()
            .chunk_revision(chunk)
    });
    let assets: Vec<_> = runner
        .components::<projection::ChunkPart>()
        .map(|(entity, part)| {
            Ok((
                entity,
                part.chunk,
                runner.component::<MeshVisual3d>(entity)?.asset().clone(),
            ))
        })
        .collect::<LogicResult<_>>()?;
    let meadow = break_one(&mut runner)?;
    let dirty: Vec<_> = revisions
        .iter()
        .enumerate()
        .filter_map(|(chunk, revision)| {
            (*revision
                != session(&runner)
                    .ok()?
                    .game
                    .active_region()
                    .chunk_revision(chunk))
            .then_some(chunk)
        })
        .collect();
    assert!(!dirty.is_empty());
    assert_eq!(
        local(&runner)?.rebuilt_chunks,
        (model::CHUNK_COUNT + dirty.len()) as u64
    );
    for (entity, chunk, asset) in assets {
        if dirty.contains(&chunk) {
            assert!(runner.component::<MeshVisual3d>(entity).is_err());
        } else {
            assert!(
                runner
                    .component::<MeshVisual3d>(entity)?
                    .asset()
                    .shares_storage(&asset)
            );
        }
    }
    travel(&mut runner)?;
    assert_eq!(session(&runner)?.game.active, model::RegionId::Canyon);
    let canyon = break_one(&mut runner)?;
    let inventory = session(&runner)?.game.inventory.clone();
    travel(&mut runner)?;
    assert_eq!(session(&runner)?.game.active, model::RegionId::Meadow);
    assert_eq!(
        session(&runner)?.game.active_region().get(meadow.0),
        model::Block::Air
    );
    assert_eq!(session(&runner)?.game.inventory, inventory);
    travel(&mut runner)?;
    assert_eq!(
        session(&runner)?.game.active_region().get(canyon.0),
        model::Block::Air
    );
    assert_eq!(session(&runner)?.game.inventory, inventory);
    assert_eq!(session(&runner)?.edits, 2);
    Ok(())
}

#[test]
fn ui_capture_cancellation_and_pause_keep_gameplay_exclusive() -> LogicResult {
    let scratch = Scratch::new()?;
    let mut runner = runner(scratch.save())?;
    ready(&mut runner)?;
    aim_at_ground(&mut runner)?;
    let selected = session(&runner)?.game.inventory.selected();
    let slot = button_position(view::Button::Slot(2))?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            pointer(slot)?,
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            pointer(GAME)?,
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
    )?;
    assert_eq!(session(&runner)?.edits, 0);
    assert_eq!(session(&runner)?.game.inventory.selected(), selected);
    for cancellation in [InputEvent::PointerLeft, InputEvent::FocusLost] {
        advance(
            &mut runner,
            Duration::ZERO,
            &[
                pointer(slot)?,
                InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
                cancellation,
            ],
        )?;
        assert!(local(&runner)?.pointer.captured().is_none());
        assert_eq!(session(&runner)?.edits, 0);
    }
    advance(
        &mut runner,
        Duration::ZERO,
        &click(slot, MouseButton::Left)?,
    )?;
    assert_eq!(
        session(&runner)?.game.inventory.selected(),
        model::Block::Wood
    );
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::KeyP))?;
    assert!(runner.is_paused());
    let ticks = session(&runner)?.ticks;
    let paused_position = session(&runner)?.game.player.position;
    let paused = advance(&mut runner, STEP * 4, &click(GAME, MouseButton::Left)?)?;
    assert_eq!(paused.fixed_ticks_attempted(), 0);
    assert_eq!(session(&runner)?.ticks, ticks);
    assert_eq!(session(&runner)?.game.player.position, paused_position);
    assert_eq!(session(&runner)?.edits, 0);
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::Digit2))?;
    assert_eq!(
        session(&runner)?.game.inventory.selected(),
        model::Block::Stone
    );
    travel(&mut runner)?;
    assert!(runner.is_paused());
    assert!(local(&runner)?.paused);
    assert_eq!(session(&runner)?.ticks, ticks);
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::KeyP))?;
    assert!(!runner.is_paused());
    Ok(())
}

#[test]
fn right_click_builds_one_selected_block_and_breaking_returns_inventory() -> LogicResult {
    let scratch = Scratch::new()?;
    let mut runner = runner(scratch.save())?;
    ready(&mut runner)?;
    aim_at_ground(&mut runner)?;
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::Digit3))?;
    let target = session(&runner)?.game.target().ok_or("placement target")?;
    let cell = target.adjacent.ok_or("placement face")?;
    let inventory = session(&runner)?.game.inventory.clone();
    let slot = button_position(view::Button::Slot(1))?;
    advance(
        &mut runner,
        Duration::ZERO,
        &click(slot, MouseButton::Right)?,
    )?;
    assert_eq!(session(&runner)?.edits, 0, "UI owns its entire panel area");
    assert_eq!(session(&runner)?.game.inventory, inventory);
    advance(
        &mut runner,
        Duration::ZERO,
        &click(GAME, MouseButton::Right)?,
    )?;
    assert_eq!(
        session(&runner)?.game.active_region().get(cell),
        model::Block::Wood
    );
    assert_eq!(
        session(&runner)?.game.inventory.count(model::Block::Wood),
        inventory.count(model::Block::Wood) - 1
    );
    assert_eq!(session(&runner)?.edits, 1);
    advance(
        &mut runner,
        Duration::ZERO,
        &click(GAME, MouseButton::Left)?,
    )?;
    assert_eq!(
        session(&runner)?.game.active_region().get(cell),
        model::Block::Air
    );
    assert_eq!(session(&runner)?.game.inventory, inventory);
    assert_eq!(session(&runner)?.edits, 2);
    Ok(())
}

#[test]
fn save_and_load_restore_both_regions_through_a_fresh_world_generation() -> LogicResult {
    let scratch = Scratch::new()?;
    let mut runner = runner(scratch.save())?;
    ready(&mut runner)?;
    let meadow = break_one(&mut runner)?;
    travel(&mut runner)?;
    let canyon = break_one(&mut runner)?;
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::Digit3))?;
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::F5))?;
    assert!(session(&runner)?.notice.starts_with("Saved"));
    let saved_inventory = session(&runner)?.game.inventory.clone();
    travel(&mut runner)?;
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::Digit5))?;
    let old_generation = runner.world_generation();
    let old_epoch = session(&runner)?.epoch;
    let loaded = advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::F9))?;
    assert!(matches!(
        loaded.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_ne!(runner.world_generation(), old_generation);
    assert_eq!(local(&runner)?.phase, scene::Phase::Loading);
    assert_eq!(
        session(&runner)?.epoch,
        old_epoch,
        "load installs after candidate commit"
    );
    ready(&mut runner)?;
    assert_eq!(session(&runner)?.epoch, old_epoch + 1);
    assert_eq!(session(&runner)?.game.active, model::RegionId::Canyon);
    assert_eq!(session(&runner)?.game.inventory, saved_inventory);
    assert_eq!(
        session(&runner)?.game.active_region().get(canyon.0),
        model::Block::Air
    );
    travel(&mut runner)?;
    assert_eq!(
        session(&runner)?.game.active_region().get(meadow.0),
        model::Block::Air
    );
    assert_eq!(session(&runner)?.game.inventory, saved_inventory);
    Ok(())
}

#[test]
fn load_io_and_corruption_failures_preserve_the_running_game() -> LogicResult {
    let scratch = Scratch::new()?;
    let mut runner = runner(scratch.save())?;
    ready(&mut runner)?;
    let broken = break_one(&mut runner)?;
    let inventory = session(&runner)?.game.inventory.clone();
    let generation = runner.world_generation();
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::KeyP))?;
    assert!(runner.is_paused());
    for corrupt_file in [false, true] {
        if corrupt_file {
            std::fs::write(scratch.save(), b"not a valid voxel save")?;
        }
        advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::F9))?;
        assert!(session(&runner)?.notice.starts_with("Load failed:"));
        assert!(
            runner.components::<ScreenTextVisual>().any(|(_, text)| {
                text.text().contains("PAUSED") && text.text().contains("Load failed:")
            }),
            "a failed load must be visible while paused"
        );
        assert_eq!(runner.world_generation(), generation);
        assert!(session(&runner)?.pending_load.is_none());
        assert_eq!(session(&runner)?.game.inventory, inventory);
        assert_eq!(
            session(&runner)?.game.active_region().get(broken.0),
            model::Block::Air
        );
    }
    // A conflicting sibling temp file must fail without replacing either file.
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::F5))?;
    let old_save = std::fs::read(scratch.save())?;
    let temp = scratch
        .0
        .join(format!("world.save.{}.tmp", std::process::id()));
    std::fs::write(&temp, b"owned by another save attempt")?;
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::F5))?;
    assert!(session(&runner)?.notice.starts_with("Save failed:"));
    assert!(
        runner.components::<ScreenTextVisual>().any(|(_, text)| {
            text.text().contains("PAUSED") && text.text().contains("Save failed:")
        }),
        "a failed save must be visible while paused"
    );
    assert_eq!(std::fs::read(scratch.save())?, old_save);
    assert_eq!(std::fs::read(temp)?, b"owned by another save attempt");
    assert_eq!(session(&runner)?.game.inventory, inventory);
    Ok(())
}

#[test]
fn cancelled_jump_press_does_not_execute_on_a_later_fixed_tick() -> LogicResult {
    let scratch = Scratch::new()?;
    let mut runner = runner(scratch.save())?;
    ready(&mut runner)?;
    advance(&mut runner, STEP * 4, &[])?;
    assert!(session(&runner)?.game.player.grounded);
    let y = session(&runner)?.game.player.position[1];
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
            InputEvent::FocusLost,
        ],
    )?;
    assert!(
        !local(&runner)?.movement.jump,
        "focus loss must discard pending motion"
    );
    advance(&mut runner, STEP, &[])?;
    assert!(session(&runner)?.game.player.position[1] <= y);
    Ok(())
}

#[test]
fn discarded_chunk_commands_are_retried_after_a_later_system_failure() -> LogicResult {
    let scratch = Scratch::new()?;
    let (mut application, initial) = application(scratch.save())?;
    let fail_once = Arc::new(AtomicBool::new(true));
    application.add_fallible_frame_system(move || -> LogicResult {
        if fail_once.swap(false, Ordering::SeqCst) {
            return Err("deliberate later frame failure".into());
        }
        Ok(())
    });
    let mut runner = application.build_headless(initial)?;
    let failed = report(&mut runner, Duration::ZERO, &[])?;
    assert!(matches!(
        failed.failure(),
        Some(FrameFailure::System { .. })
    ));
    assert_eq!(runner.components::<projection::ChunkPart>().count(), 0);
    advance(&mut runner, Duration::ZERO, &[])?;
    ready(&mut runner)?;
    assert!(
        runner.components::<projection::ChunkPart>().count() > 0,
        "failed Commands must not leave nonexistent chunks marked as projected"
    );
    assert!(runner.resource::<View3d>().ok_or("view")?.enabled());
    Ok(())
}

#[test]
fn rejected_structural_batch_keeps_old_meshes_then_retries_the_canonical_edit() -> LogicResult {
    let scratch = Scratch::new()?;
    let (mut application, initial) = application(scratch.save())?;
    let reject = Arc::new(AtomicBool::new(false));
    let reject_in_system = Arc::clone(&reject);
    application.add_fallible_frame_system(
        move |panels: Query<(LogicEntityRef, &view::Panel)>,
              mut commands: Commands|
              -> LogicResult {
            if reject_in_system.swap(false, Ordering::SeqCst) {
                let (entity, _) = panels.iter().next().ok_or("panel for rejection")?;
                commands.despawn(entity.handle())?;
                commands.despawn(entity.handle())?;
            }
            Ok(())
        },
    );
    let mut runner = application.build_headless(initial)?;
    ready(&mut runner)?;
    aim_at_ground(&mut runner)?;
    let hit = session(&runner)?.game.target().ok_or("ground target")?;
    let revisions: [_; model::CHUNK_COUNT] = std::array::from_fn(|chunk| {
        session(&runner)
            .unwrap()
            .game
            .active_region()
            .chunk_revision(chunk)
    });
    let old_meshes: Vec<_> = runner
        .components::<projection::ChunkPart>()
        .map(|(entity, part)| (entity, part.chunk))
        .collect();
    reject.store(true, Ordering::SeqCst);
    let failed = report(
        &mut runner,
        Duration::ZERO,
        &click(GAME, MouseButton::Left)?,
    )?;
    assert!(matches!(
        failed.failure(),
        Some(FrameFailure::Commands { .. })
    ));
    assert_eq!(
        session(&runner)?.game.active_region().get(hit.cell),
        model::Block::Air
    );
    assert_eq!((failed.spawned(), failed.despawned()), (0, 0));
    for (entity, _) in &old_meshes {
        assert!(runner.component::<MeshVisual3d>(*entity).is_ok());
    }
    advance(&mut runner, Duration::ZERO, &[])?;
    for (entity, chunk) in old_meshes {
        let dirty =
            revisions[chunk] != session(&runner)?.game.active_region().chunk_revision(chunk);
        assert_eq!(
            runner.component::<MeshVisual3d>(entity).is_err(),
            dirty,
            "only chunks touched by the retained canonical edit must be replaced"
        );
    }
    Ok(())
}

#[test]
fn a_failed_load_cannot_install_during_an_unrelated_later_travel() -> LogicResult {
    let scratch = Scratch::new()?;
    let (mut application, initial) = application(scratch.save())?;
    let fail_candidate = Arc::new(AtomicBool::new(false));
    let fail_in_startup = Arc::clone(&fail_candidate);
    application.add_fallible_system(Stage::Startup, move || -> LogicResult {
        if fail_in_startup.swap(false, Ordering::SeqCst) {
            return Err("deliberate candidate startup failure".into());
        }
        Ok(())
    });
    let mut runner = application.build_headless(initial)?;
    ready(&mut runner)?;
    advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::F5))?;
    assert!(session(&runner)?.notice.starts_with("Saved"));
    let broken = break_one(&mut runner)?;
    let inventory = session(&runner)?.game.inventory.clone();
    let epoch = session(&runner)?.epoch;
    let generation = runner.world_generation();
    fail_candidate.store(true, Ordering::SeqCst);
    let failed = advance(&mut runner, Duration::ZERO, &key(PhysicalKeyCode::F9))?;
    assert!(matches!(
        failed.transition(),
        FrameTransition::PreparationFailed { .. }
    ));
    assert_eq!(runner.world_generation(), generation);
    // No idle cleanup frame: the next input immediately asks to visit Canyon.
    travel(&mut runner)?;
    assert_eq!(
        session(&runner)?.epoch,
        epoch,
        "an unrelated trip must not apply the rejected save"
    );
    assert_eq!(session(&runner)?.game.inventory, inventory);
    assert_eq!(
        session(&runner)?
            .game
            .region(model::RegionId::Meadow)
            .get(broken.0),
        model::Block::Air
    );
    assert!(session(&runner)?.pending_load.is_none());
    Ok(())
}
