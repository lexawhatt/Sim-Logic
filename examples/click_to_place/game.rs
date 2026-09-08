use sim_logic::prelude::*;

/// Maximum placed circles, excluding the one camera entity.
pub const MAX_MARKERS: usize = 128;

/// The three controls used by this example.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemoAction {
    /// Place one circle for each left-button press with a known pointer.
    Place,
    /// Clear the circles at the next fixed stage barrier.
    Clear,
    /// Exit at the current frame stage barrier.
    Exit,
}

/// Identifies circles created by clicks for bounded counting and clearing.
#[derive(Component)]
pub struct PlacedMarker;

fn place_markers(
    input: FixedInput<DemoAction>,
    camera: Single<&ActiveCamera2d>,
    markers: Query<LogicEntityRef, With<PlacedMarker>>,
    mut commands: Commands,
) -> LogicResult {
    // Clear takes priority over placement when both arrive in the same tick.
    if input.has_press_occurrence(DemoAction::Clear) {
        for marker in &markers {
            commands.despawn(marker.handle())?;
        }
        return Ok(());
    }

    let mut available = MAX_MARKERS.saturating_sub(markers.iter().count());
    for edge in input.pressed(DemoAction::Place) {
        if available == 0 {
            break;
        }
        let Some(pointer) = edge.pointer() else {
            continue;
        };
        // The canonical camera is intentionally static. This is coordinate
        // conversion at depth zero, not a query of rendered objects.
        let position = pointer.world_position(camera.camera())?;
        commands.spawn((
            PlacedMarker,
            Transform2d::new(position)?,
            CircleVisual::new(0.35, Color::rgb8(68, 144, 255))?,
        ))?;
        available -= 1;
    }
    Ok(())
}

fn exit_on_escape(input: FrameInput<DemoAction>, mut commands: Commands) -> LogicResult {
    if input.has_press_occurrence(DemoAction::Exit) {
        commands.request_exit()?;
    }
    Ok(())
}

/// Builds the same bounded scene and systems for desktop or headless use.
pub fn build_application(
    time: TimeConfig,
) -> LogicResult<(Application<DemoAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    config.set_entity_limit(MAX_MARKERS + 1)?;
    config.set_command_limit(MAX_MARKERS)?;
    let mut application = Application::new(config)?;
    application.approve_component::<PlacedMarker>()?;
    application.bind_mouse_button(MouseButton::Left, DemoAction::Place)?;
    application.bind_mouse_button(MouseButton::Right, DemoAction::Clear)?;
    application.bind_key(PhysicalKeyCode::Escape, DemoAction::Exit)?;
    application.add_fallible_fixed_system(place_markers);
    application.add_fallible_frame_system(exit_on_escape);

    let camera = ActiveCamera2d::centered(20.0)?;
    let background = WorldBackground::new(Color::rgb8(10, 16, 28))?;
    let initial_world = application.register_world("click-to-place", move |world| {
        world.spawn(camera)?;
        world.insert_resource(background)?;
        Ok(())
    })?;
    Ok((application, initial_world))
}
