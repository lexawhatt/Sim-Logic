use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

#[path = "../../examples/screen_hud/game.rs"]
mod game;

use super::support::*;

fn hud_entity(
    runner: &HeadlessRunner<game::DemoAction>,
    part: game::HudPart,
) -> Result<LogicEntity, Box<dyn Error>> {
    runner
        .components::<game::HudPart>()
        .find_map(|(entity, candidate)| (*candidate == part).then_some(entity))
        .ok_or_else(|| "the example should retain every panel part".into())
}

#[test]
fn shared_example_keeps_hud_layout_and_animation_live_while_world_motion_is_paused()
-> Result<(), Box<dyn Error>> {
    let (application, initial) =
        game::build_application_with_time(TimeConfig::new(FIXED_STEP, 4)?)?;
    let mut runner = application.build_headless(initial)?;
    let (body, _) = runner
        .components::<game::MovingBody>()
        .next()
        .ok_or("moving body")?;
    let panel = hud_entity(&runner, game::HudPart::Panel)?;
    let status = hud_entity(&runner, game::HudPart::Status)?;
    let progress = hud_entity(&runner, game::HudPart::ProgressFill)?;
    let activity = hud_entity(&runner, game::HudPart::Activity)?;
    assert_eq!(snapshot(&runner)?.resolved_screen_rectangles().len(), 5);

    let running = advance(&mut runner, Duration::from_millis(150), &[], 800.0)?;
    assert!(running.failure().is_none());
    assert_eq!(running.fixed_ticks_attempted(), 1);
    assert!(runner.component::<Transform2d>(body)?.translation().x() > 0.0);
    let panel_position = runner.component::<ScreenRectangleVisual>(panel)?.position();
    assert_eq!(panel_position, LogicalScreenPosition::new(476.0, 24.0));
    assert_eq!(
        runner.component::<ScreenRectangleVisual>(status)?.color(),
        Color::rgb8(75, 215, 155)
    );

    let paused = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
        800.0,
    )?;
    assert!(paused.failure().is_none());
    assert!(runner.is_paused());
    assert_eq!(
        runner.component::<ScreenRectangleVisual>(status)?.color(),
        Color::rgb8(255, 185, 70)
    );
    let frozen_body = *runner.component::<Transform2d>(body)?;
    let frozen_camera = snapshot(&runner)?.camera();
    let frozen_progress = runner.component::<ScreenRectangleVisual>(progress)?.size();
    let previous_activity = runner.component::<ScreenRectangleVisual>(activity)?.color();

    let resized = advance(&mut runner, Duration::from_millis(250), &[], 1_000.0)?;
    assert!(resized.failure().is_none());
    assert_eq!(resized.fixed_ticks_attempted(), 0);
    assert_eq!(*runner.component::<Transform2d>(body)?, frozen_body);
    assert_eq!(snapshot(&runner)?.camera(), frozen_camera);
    assert_eq!(
        runner.component::<ScreenRectangleVisual>(panel)?.position(),
        LogicalScreenPosition::new(676.0, 24.0)
    );
    assert_eq!(
        runner.component::<ScreenRectangleVisual>(progress)?.size(),
        frozen_progress
    );
    assert_ne!(
        runner.component::<ScreenRectangleVisual>(activity)?.color(),
        previous_activity
    );
    for rectangle in snapshot(&runner)?.resolved_screen_rectangles() {
        let component = runner.component::<ScreenRectangleVisual>(rectangle.source())?;
        assert_eq!(rectangle.position(), component.position());
        assert_eq!(rectangle.size(), component.size());
        assert_eq!(rectangle.color(), component.color());
    }

    let resumed = advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Released),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
        ],
        1_000.0,
    )?;
    assert!(resumed.failure().is_none());
    assert!(!runner.is_paused());
    let next_tick = advance(&mut runner, FIXED_STEP, &[], 1_000.0)?;
    assert!(next_tick.failure().is_none());
    assert!(
        runner.component::<Transform2d>(body)?.translation().x() > frozen_body.translation().x()
    );

    let exited = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Escape,
            ButtonState::Pressed,
        )],
        1_000.0,
    )?;
    assert!(exited.failure().is_none());
    assert!(exited.exit_requested());
    Ok(())
}
