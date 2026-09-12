//! Hidden HUD components must follow committed ECS state, not optimistic stamps.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use sim_logic::prelude::*;

use super::{app, model, scene, view};

fn viewport() -> LogicResult<LogicalViewport> {
    Ok(LogicalViewport::new(1100.0, 720.0)?)
}

fn key(key: PhysicalKeyCode) -> [InputEvent; 2] {
    [
        InputEvent::key(key, ButtonState::Pressed),
        InputEvent::key(key, ButtonState::Released),
    ]
}

fn frame(
    runner: &mut HeadlessRunner<app::Action>,
    events: &[InputEvent],
    viewport: LogicalViewport,
) -> LogicResult<LogicFrameReport> {
    frame_for(runner, events, viewport, Duration::ZERO)
}

fn frame_for(
    runner: &mut HeadlessRunner<app::Action>,
    events: &[InputEvent],
    viewport: LogicalViewport,
    elapsed: Duration,
) -> LogicResult<LogicFrameReport> {
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(elapsed, events, viewport))
    else {
        return Err("frame rejected before execution".into());
    };
    Ok(report)
}

fn ready(reject: Arc<AtomicBool>) -> LogicResult<HeadlessRunner<app::Action>> {
    let (mut application, initial) =
        app::build_application(Default::default(), "unused-ui-retry-v2.save".into())?;
    application.add_fallible_frame_system(
        move |panels: Query<(LogicEntityRef, &view::Panel)>,
              mut commands: Commands|
              -> LogicResult {
            if reject.swap(false, Ordering::SeqCst) {
                let (entity, _) = panels.iter().next().ok_or("panel fixture")?;
                commands.despawn(entity.handle())?;
                commands.despawn(entity.handle())?;
            }
            Ok(())
        },
    );
    let mut runner = application.build_headless(initial)?;
    for _ in 0..100 {
        assert!(frame(&mut runner, &[], viewport()?)?.failure().is_none());
        if runner.resource::<scene::Local>().ok_or("local")?.phase == scene::Phase::Ready {
            return Ok(runner);
        }
    }
    Err("terrain did not become ready".into())
}

fn palette_label(runner: &HeadlessRunner<app::Action>, index: usize) -> LogicResult<LogicEntity> {
    runner.components::<view::Label>()
        .find_map(|(entity, label)| matches!(label, view::Label::Button(view::Button::CreativeBlock(candidate)) if *candidate == index).then_some(entity))
        .ok_or_else(|| "palette descriptor missing".into())
}

#[test]
fn failed_open_and_close_batches_retry_actual_hidden_component_state() -> LogicResult {
    let reject = Arc::new(AtomicBool::new(false));
    let mut runner = ready(Arc::clone(&reject))?;
    let label = palette_label(&runner, 31)?;
    assert!(runner.component::<ScreenTextVisual>(label).is_err());
    // A held key already queued movement before opening. It remains physically
    // held across the rejected batch and both recovery frames below.
    assert!(
        frame(
            &mut runner,
            &[InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed)],
            viewport()?,
        )?
        .failure()
        .is_none()
    );
    assert_eq!(
        runner
            .resource::<scene::Local>()
            .ok_or("local")?
            .movement
            .forward,
        1.0
    );

    reject.store(true, Ordering::SeqCst);
    assert!(
        frame(&mut runner, &key(PhysicalKeyCode::KeyE), viewport()?)?
            .failure()
            .is_some()
    );
    assert!(runner.component::<ScreenTextVisual>(label).is_err());
    assert!(
        !runner.is_paused(),
        "rejected opening did not commit its timing command"
    );
    let stopped = runner
        .app_resource::<app::Session>()
        .ok_or("session")?
        .game
        .player;
    let ticks = runner
        .app_resource::<app::Session>()
        .ok_or("session")?
        .ticks;
    let retried = frame_for(&mut runner, &[], viewport()?, Duration::from_millis(100))?;
    assert!(retried.failure().is_none());
    assert!(
        retried.fixed_ticks_attempted() > 0,
        "exercise unpaused scheduler catch-up before the retry barrier"
    );
    assert_eq!(
        runner
            .app_resource::<app::Session>()
            .ok_or("session")?
            .game
            .player,
        stopped
    );
    assert_eq!(
        runner
            .app_resource::<app::Session>()
            .ok_or("session")?
            .ticks,
        ticks
    );
    assert!(runner.is_paused());
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::Creative
    );
    assert_eq!(
        runner
            .resource::<scene::Local>()
            .ok_or("local")?
            .movement
            .forward,
        0.0
    );
    assert_eq!(
        runner.component::<ScreenTextVisual>(label)?.text(),
        model::Block::SOLID[31].name()
    );
    assert!(
        frame_for(&mut runner, &[], viewport()?, Duration::from_millis(100))?
            .failure()
            .is_none()
    );
    assert_eq!(
        runner
            .app_resource::<app::Session>()
            .ok_or("session")?
            .game
            .player,
        stopped
    );

    reject.store(true, Ordering::SeqCst);
    assert!(
        frame(&mut runner, &key(PhysicalKeyCode::KeyE), viewport()?)?
            .failure()
            .is_some()
    );
    assert!(runner.component::<ScreenTextVisual>(label).is_ok());
    assert!(
        runner.is_paused(),
        "rejected close retained the old clock state"
    );
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::None
    );
    assert!(frame(&mut runner, &[], viewport()?)?.failure().is_none());
    assert!(runner.component::<ScreenTextVisual>(label).is_err());
    assert!(
        !runner.is_paused(),
        "retry must honor closing rather than reopening from the stale clock"
    );
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::None
    );
    Ok(())
}

#[test]
fn unchanged_visible_labels_share_preparation_and_resize_uses_registered_font() -> LogicResult {
    let mut runner = ready(Arc::new(AtomicBool::new(false)))?;
    let title = runner
        .components::<view::Label>()
        .find_map(|(entity, label)| matches!(label, view::Label::Title).then_some(entity))
        .ok_or("title descriptor")?;
    let old = runner.component::<ScreenTextVisual>(title)?.clone();
    assert!(frame(&mut runner, &[], viewport()?)?.failure().is_none());
    let unchanged = runner.component::<ScreenTextVisual>(title)?;
    assert!(std::ptr::eq(old.shaped_line(), unchanged.shaped_line()));

    assert!(
        frame(&mut runner, &[], LogicalViewport::new(660.0, 432.0)?)?
            .failure()
            .is_none()
    );
    let resized = runner.component::<ScreenTextVisual>(title)?;
    assert_eq!(resized.text(), old.text());
    assert_ne!(resized.font(), old.font());
    assert_ne!(resized.position(), old.position());
    assert!(!std::ptr::eq(old.shaped_line(), resized.shaped_line()));
    assert!(
        runner
            .component::<ScreenTextVisual>(palette_label(&runner, 31)?)
            .is_err()
    );
    Ok(())
}
