//! Iron Maze: a small original 2.5D shooter built on the shared Logic runtime.

pub mod game;
pub mod level;
pub mod render;

use game::{Controls, GameState};
use sim_logic::prelude::*;
use std::time::Duration;

pub const FIXED_STEP: Duration = Duration::from_nanos(8_333_333);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameAction {
    Left,
    Right,
    Back,
    Forward,
    TurnLeft,
    TurnRight,
    Fire,
    Restart,
    Exit,
}

const MOVEMENT: DigitalAxis2d<GameAction> = DigitalAxis2d::new(
    GameAction::Left,
    GameAction::Right,
    GameAction::Back,
    GameAction::Forward,
);

fn simulate(
    input: FixedInput<GameAction>,
    time: FixedTime,
    mut state: ResMut<GameState>,
) -> LogicResult {
    // Reset at one fixed boundary, preserving the screen pool. The first-tick
    // snapshot is consumed normally, without replaying quick pre-reset shots.
    if input.has_press_occurrence(GameAction::Restart) {
        *state = GameState::new();
        return Ok(());
    }
    state.step(
        Controls {
            movement: input.digital_axis(MOVEMENT),
            turn: f32::from(input.held(GameAction::TurnRight))
                - f32::from(input.held(GameAction::TurnLeft)),
            shoot: input.held(GameAction::Fire) || input.has_press_occurrence(GameAction::Fire),
        },
        time.seconds_f32(),
    )?;
    Ok(())
}

fn navigate(input: FrameInput<GameAction>, mut commands: Commands) -> LogicResult {
    if input.has_press_occurrence(GameAction::Exit) {
        commands.request_exit()?;
    }
    Ok(())
}

/// Builds the complete game using only public Sim;Logic APIs.
pub fn build_application() -> LogicResult<(Application<GameAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 8)?);
    config.set_entity_limit(render::MAX_QUADS + 1)?;
    config.set_render_limits(render::limits());
    let mut application = Application::new(config)?;
    application.bind_wasd(MOVEMENT)?;
    application.bind_key(PhysicalKeyCode::ArrowLeft, GameAction::TurnLeft)?;
    application.bind_key(PhysicalKeyCode::ArrowRight, GameAction::TurnRight)?;
    application.bind_key(PhysicalKeyCode::Space, GameAction::Fire)?;
    application.bind_mouse_button(MouseButton::Left, GameAction::Fire)?;
    application.bind_key(PhysicalKeyCode::Enter, GameAction::Restart)?;
    application.bind_key(PhysicalKeyCode::Escape, GameAction::Exit)?;
    application.add_fallible_fixed_system(simulate);
    application.add_fallible_frame_system(navigate);
    render::register(&mut application)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let background = WorldBackground::new(Color::rgb8(6, 9, 12))?;
    let world = application.register_world("iron-maze", move |world| {
        world.spawn(camera)?;
        world.insert_resource(background)?;
        world.insert_resource(GameState::new())?;
        render::spawn(world)
    })?;
    Ok((application, world))
}
