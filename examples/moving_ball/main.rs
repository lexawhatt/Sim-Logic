use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PlayerAction {
    Up,
    Down,
    Left,
    Right,
    NextWorld,
    Pause,
    Exit,
}

const MOVEMENT: DigitalAxis2d<PlayerAction> = DigitalAxis2d::new(
    PlayerAction::Left,
    PlayerAction::Right,
    PlayerAction::Down,
    PlayerAction::Up,
);

#[derive(Component)]
struct Ball;

fn toggle_pause(
    input: FrameInput<PlayerAction>,
    time: FrameTime,
    mut commands: Commands,
) -> LogicResult {
    if input.has_press_occurrence(PlayerAction::Pause) {
        commands.set_paused(!time.is_paused())?;
    }
    Ok(())
}

fn exit_on_escape(input: FrameInput<PlayerAction>, mut commands: Commands) -> LogicResult {
    if input.has_press_occurrence(PlayerAction::Exit) {
        commands.request_exit()?;
    }
    Ok(())
}

fn main() -> LogicResult {
    let mut app = Application::<PlayerAction>::new(AppConfig::default())?;
    app.approve_component::<Ball>()?;
    app.bind_wasd_and_arrows(MOVEMENT)?;
    app.bind_key(PhysicalKeyCode::Enter, PlayerAction::NextWorld)?;
    app.bind_key(PhysicalKeyCode::Space, PlayerAction::Pause)?;
    app.bind_key(PhysicalKeyCode::Escape, PlayerAction::Exit)?;
    let movement = DigitalMovement2d::new(MOVEMENT, 8.0)?;

    let camera_b = ActiveCamera2d::centered(32.0)?;
    let ball_b = CircleVisual::new(1.0, Color::rgb8(245, 132, 31))?;
    let background_b = WorldBackground::new(Color::rgb8(30, 18, 12))?;
    let transform_b = Transform2d::from_xy(3.0, 0.0)?;
    let world_b = app.register_world("orange-world", move |world| {
        world.insert_resource(background_b)?;
        world.spawn(camera_b)?;
        world.spawn((Ball, transform_b, ball_b, movement))?;
        Ok(())
    })?;

    let camera_a = ActiveCamera2d::centered(32.0)?;
    let ball_a = CircleVisual::new(1.0, Color::rgb8(68, 144, 255))?;
    let background_a = WorldBackground::new(Color::rgb8(10, 16, 28))?;
    let world_a = app.register_world("blue-world", move |world| {
        world.insert_resource(background_a)?;
        world.insert_resource(WorldReplacementOnPress::new(
            PlayerAction::NextWorld,
            world_b,
        ))?;
        world.spawn(camera_a)?;
        world.spawn((Ball, ball_a, movement))?;
        Ok(())
    })?;

    app.add_digital_movement2d_system();
    app.add_world_replacement_on_press_system();
    app.add_fallible_frame_system(toggle_pause);
    app.add_fallible_frame_system(exit_on_escape);

    println!(
        "Move with WASD or arrow keys. Space pauses/resumes; Enter replaces the World when running; Escape exits."
    );
    let report = app.run(world_a)?;
    println!(
        "stopped after {} logical frames and {} World transition(s): {:?}",
        report.logic_frames(),
        report.committed_transitions(),
        report.exit_reason()
    );
    if let Some(logic) = report.last_logic_frame() {
        println!(
            "last logical frame: index={}, fixed_ticks={}, transition={:?}, failure={:?}",
            logic.frame_index(),
            logic.fixed_ticks_attempted(),
            logic.transition(),
            logic.failure()
        );
    }
    if let Some(render) = report.last_render_frame() {
        println!("last render frame: status={:?}", render.status());
    }
    Ok(())
}
