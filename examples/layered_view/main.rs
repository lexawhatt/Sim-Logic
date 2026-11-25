//! Standard Logic host: independent Base effects, clean Detail, live screen UI.
use sim_logic::{input::EditorEvent, prelude::*};
use std::num::NonZeroU32;

#[derive(Resource, Default)]
struct Probe {
    frames: u64,
    rejected: bool,
    typed: bool,
    clicked: bool,
    acceptance: bool,
    effects: bool,
    phase: f32,
}
#[derive(Component)]
struct Detail;
fn settings(divisor: u32, effects: bool, fallback: bool) -> LogicResult<LayeredView3d> {
    let effect = if effects {
        SurfaceEffects3d::new()
            .with_quantization(6)?
            .with_dither(180)
            .with_scanlines(40)
    } else {
        SurfaceEffects3d::new()
    };
    Ok(LayeredView3d::new(
        LayerSettings3d::new(LayerResolution3d::DivideSurface(
            NonZeroU32::new(divisor).ok_or("zero divisor")?,
        ))
        .with_effects(effect)
        .with_surface_policy(ThreeDSurfacePolicy::Native),
        LayerSettings3d::default().with_surface_policy(ThreeDSurfacePolicy::Native),
    )
    .with_failure_policy(LayeredFailurePolicy3d::ContinueUi)
    .with_resolution_fallback(if fallback {
        LayerResolutionFallback3d::FullSurface
    } else {
        LayerResolutionFallback3d::Reject
    }))
}
#[allow(
    clippy::too_many_arguments,
    reason = "Each argument declares a distinct managed ECS system input."
)]
fn update(
    input: FrameInput<u8>,
    text: FrameText,
    feedback: AppRes<LayeredPresentationFeedback>,
    mut probe: ResMut<Probe>,
    mut view: ResMut<View3d>,
    mut visuals: Query<&mut CuboidVisual3d>,
    mut labels: Query<&mut ScreenTextVisual>,
    mut commands: Commands,
) -> LogicResult {
    probe.frames += 1;
    probe.rejected |= feedback.last_rejection().is_some();
    probe.clicked |= input.has_press_occurrence(9);
    for event in text.ordered() {
        if let EditorEvent::Text(TextEvent::Commit(value) | TextEvent::Paste(value)) = event {
            probe.typed |= !value.is_empty();
        }
    }
    if input.has_press_occurrence(1) {
        for mut visual in &mut visuals {
            let layer = if visual.render_layer() == RenderLayer3d::Base {
                RenderLayer3d::Detail
            } else {
                RenderLayer3d::Base
            };
            visual.set_render_layer(layer);
        }
    }
    if input.has_press_occurrence(2) {
        probe.effects = !probe.effects;
        view.set_layered(Some(settings(2, probe.effects, true)?));
    }
    if input.has_press_occurrence(3) {
        view.set_layered(Some(settings(997, probe.effects, false)?));
    }
    if input.has_press_occurrence(4) {
        view.set_layered(Some(settings(2, probe.effects, true)?));
    }
    if input.has_press_occurrence(5) {
        for mut visual in &mut visuals {
            let wire = if visual.wireframe().is_some() {
                None
            } else {
                Some(WireframeStyle3d::visible(
                    Color::BLACK,
                    LogicalPixels::new(1.0)?,
                )?)
            };
            visual.set_wireframe(wire);
        }
    }
    if input.has_press_occurrence(6) {
        let next = if view.layered().is_some() {
            None
        } else {
            Some(settings(2, probe.effects, true)?)
        };
        view.set_layered(next);
    }
    if input.has_press_occurrence(12) {
        if probe.acceptance && !(probe.rejected && probe.typed && probe.clicked) {
            return Err("acceptance requires a rejection, native text and pointer click".into());
        }
        if probe.acceptance {
            println!(
                "LAYERED_NATIVE_INPUT_PASS current UI after rejection, text and pointer input"
            );
        }
        commands.request_exit()?;
    }
    // Camera motion belongs to this example. Layer changes never alter input coordinates.
    probe.phase += 0.005;
    view.set_pose(Vec3::new(probe.phase.sin() * 1.5, 2.5, 7.0)?, Vec3::ZERO)?;
    for mut label in &mut labels {
        label.set_text(&format!(
            "Frame {} | 3D: {:?} | text: {} | click: {}",
            probe.frames,
            feedback.last_rejection().map(|r| r.reason),
            probe.typed,
            probe.clicked
        ))?;
    }
    Ok(())
}
fn main() -> LogicResult {
    let acceptance = std::env::args().any(|arg| arg == "--acceptance");
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::default()
            .with_frame_limits(FrameLimits::new(
                32,
                128,
                50_000,
                8 * 1024 * 1024,
                16 * 1024 * 1024,
                128,
            ))
            .with_max_screen_texts(1)
            .with_max_screen_text_bytes(8192)
            .with_max_screen_text_glyphs(4096),
    );
    config.set_three_d_render_limits(
        ThreeDRenderLimits::new(2, 24, 2048 * 2048).with_layered_limits(
            LayeredRenderLimits3d::new(24, 24, 72, 128 * 1024 * 1024, 256 * 1024 * 1024),
        ),
    );
    let mut app = Application::<u8>::new(config)?;
    app.register_app_resource(LayeredPresentationFeedback::default())?;
    let mut controls = WindowControls::default();
    controls.set_text_input(true);
    app.register_app_resource(controls)?;
    app.approve_components::<(Detail,)>()?;
    for (key, action) in [
        (PhysicalKeyCode::Space, 1),
        (PhysicalKeyCode::F2, 2),
        (PhysicalKeyCode::F3, 3),
        (PhysicalKeyCode::F4, 4),
        (PhysicalKeyCode::F5, 5),
        (PhysicalKeyCode::F6, 6),
        (PhysicalKeyCode::F12, 12),
    ] {
        app.bind_key(key, action)?;
    }
    app.bind_mouse_button(MouseButton::Left, 9)?;
    let font = app.register_font(
        include_bytes!("../../tests/assets/text/DejaVuSans.ttf").to_vec(),
        TextSettings::new(15.0)?,
    )?;
    let label =
        ScreenTextVisual::new(font, "Layered view", LogicalScreenPosition::new(12.0, 24.0))?;
    let panel = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(0.0, 0.0),
        LogicalScreenVector::new(1024.0, 65.0),
        Color::rgb(0.05, 0.06, 0.08),
    )?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let mut view = View3d::new(Vec3::new(0.0, 2.5, 7.0)?, Vec3::ZERO)?;
    view.set_layered(Some(settings(2, true, true)?));
    view.set_fog(Some(Fog3d::new(Color::rgb(0.15, 0.2, 0.3), 2.0, 0.1)?));
    view.set_surface_policy(ThreeDSurfacePolicy::Native);
    let mut base = CuboidVisual3d::new(
        Vec3::ZERO,
        Vec3::new(3.2, 0.3, 2.0)?,
        Color::rgb(0.2, 0.7, 0.4),
    )?;
    base.set_fog_enabled(true);
    let mut detail = CuboidVisual3d::new(
        Vec3::new(0.2, 0.25, 0.0)?,
        Vec3::new(0.8, 1.3, 0.8)?,
        Color::rgb(0.9, 0.5, 0.1),
    )?;
    detail.set_render_layer(RenderLayer3d::Detail);
    let world = app.register_world("layered-view", move |world| {
        world.spawn(camera)?;
        world.insert_resource(view)?;
        world.insert_resource(Probe {
            acceptance,
            effects: true,
            ..Probe::default()
        })?;
        world.spawn(base)?;
        world.spawn((detail, Detail))?;
        world.spawn(panel)?;
        world.spawn(label.clone())?;
        Ok(())
    })?;
    app.add_fallible_system(Stage::FrameUpdate, update);
    println!(
        "Space: layers; F2: effects; F3: reject; F4: restore; F5: edges; F6: ordinary; F12: exit"
    );
    let report = app.run_desktop(
        world,
        DesktopConfig::new("Sim Logic Layered View", 960.0, 600.0)?,
    )?;
    println!(
        "drawn={} rejected={} lifecycle={:?} renderer={:?}",
        report.drawn_frames(),
        report.layered_rejections(),
        report.layered_statistics(),
        report.renderer_description()
    );
    if acceptance {
        assert!(report.drawn_frames() > 10);
        assert!(report.layered_rejections() > 0);
        assert!(report.layered_statistics().created > 0);
        assert!(report.layered_statistics().reused > 0);
    }
    Ok(())
}
