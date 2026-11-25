//! Real GPU lifecycle oracle through Logic extraction, caches and UI composition.
use super::*;
use crate::desktop::images::{self, DesktopImages};
use crate::desktop::{DesktopLayeredProvenance, LayeredTargetAction};
use sim_engine::{CaptureCorrelation, FrameBudget, RenderTargetBudget, SurfaceEffects3d};
use std::num::NonZeroU32;
const L: ThreeDRenderLimits = ThreeDRenderLimits::new(0, 16, 128 * 128)
    .with_mesh_limits(2, 32 * 1024)
    .with_layered_limits(LayeredRenderLimits3d::new(
        16,
        16,
        48,
        2 * 1024 * 1024,
        4 * 1024 * 1024,
    ));
fn build() -> LogicResult<HeadlessRunner<u8>> {
    let mut config = AppConfig::default();
    config.set_three_d_render_limits(L);
    let mut app = Application::<u8>::new(config)?;
    app.bind_key(PhysicalKeyCode::Space, 1)?;
    app.bind_key(PhysicalKeyCode::Digit2, 2)?;
    app.add_fallible_frame_system(
        |input: FrameInput<u8>,
         mut view: ResMut<View3d>,
         mut meshes: Query<&mut MeshVisual3d>|
         -> LogicResult {
            if input.has_press_occurrence(1) {
                for mut visual in &mut meshes {
                    let next = if visual.render_layer() == RenderLayer3d::Base {
                        RenderLayer3d::Detail
                    } else {
                        RenderLayer3d::Base
                    };
                    visual.set_render_layer(next);
                }
            }
            if input.has_press_occurrence(2) {
                let mut layers = view.layered().ok_or("layered")?.layers();
                layers[0] = layers[0].with_effects(
                    SurfaceEffects3d::new()
                        .with_quantization(8)?
                        .with_dither(180),
                );
                view.set_layered(Some(LayeredView3d::new(layers[0], layers[1])));
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let mut view = View3d::new(Vec3::new(0.0, 0.0, 5.0)?, Vec3::ZERO)?;
    view.set_orthographic(
        WorldLength::new(4.0)?,
        WorldLength::new(0.1)?,
        WorldLength::new(20.0)?,
    )?;
    let base_settings = LayerSettings3d::new(LayerResolution3d::DivideSurface(
        NonZeroU32::new(2).ok_or("divisor")?,
    ))
    .with_surface_policy(ThreeDSurfacePolicy::Native);
    view.set_layered(Some(LayeredView3d::new(
        base_settings,
        LayerSettings3d::default().with_surface_policy(ThreeDSurfacePolicy::Native),
    )));
    let base = MeshVisual3d::new(
        MeshAsset3d::new(Mesh3d::new(
            vec![
                Vec3::new(-1.5, -1.0, 0.0)?,
                Vec3::new(1.5, -1.0, 0.0)?,
                Vec3::new(1.5, 1.0, 0.0)?,
                Vec3::new(-1.5, 1.0, 0.0)?,
            ],
            vec![0, 1, 2, 0, 2, 3],
        )?)?,
        Transform3d::IDENTITY,
        Color::rgb(0.0, 0.8, 0.0),
    )?;
    let mut detail = MeshVisual3d::new(
        MeshAsset3d::new(Mesh3d::new(
            vec![
                Vec3::new(-1.0, -0.8, 0.8)?,
                Vec3::new(1.0, -0.8, -0.8)?,
                Vec3::new(0.0, 1.0, 0.0)?,
            ],
            vec![0, 1, 2],
        )?)?,
        Transform3d::IDENTITY,
        Color::rgb(0.8, 0.0, 0.0),
    )?;
    detail.set_render_layer(RenderLayer3d::Detail);
    let ui = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(0.0, 0.0),
        LogicalScreenVector::new(16.0, 16.0),
        Color::rgb(0.0, 0.0, 1.0),
    )?;
    let world = app.register_world("layered-oracle", move |world| {
        world.spawn(camera)?;
        world.insert_resource(view)?;
        world.spawn(base.clone())?;
        world.spawn(detail.clone())?;
        world.spawn(ui)?;
        Ok(())
    })?;
    Ok(app.build_headless(world)?)
}
fn prepare(
    cache: &mut DesktopThreeD,
    renderer: &mut WgpuRenderer,
    runner: &HeadlessRunner<u8>,
    limits: ThreeDRenderLimits,
    device: u64,
) -> Result<crate::desktop::DesktopLayeredFrame, DesktopThreeDError> {
    cache.begin_world(renderer, runner.world_generation());
    let snapshot = runner
        .extracted_frame()
        .and_then(|frame| frame.three_d())
        .ok_or(DesktopThreeDError::InvalidCache)?;
    cache.prepare_layered(
        renderer,
        snapshot,
        limits,
        DesktopLayeredProvenance {
            frame_index: device,
            world: runner.world_generation(),
            device_generation: device,
            configuration_revision: 0,
        },
    )
}
fn key(runner: &mut HeadlessRunner<u8>, code: PhysicalKeyCode) -> LogicResult {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        let events = [InputEvent::key(code, state)];
        match runner.advance_frame(FrameRequest::new(
            Duration::ZERO,
            &events,
            LogicalViewport::new(64.0, 64.0)?,
        )) {
            FrameOutcome::Advanced(report) if report.failure().is_none() => {}
            outcome => return Err(format!("fixture frame failed: {outcome:?}").into()),
        }
    }
    Ok(())
}
pub(super) fn run(renderer: &mut WgpuRenderer) -> LogicResult {
    let mut runner = build()?;
    let mut cache = DesktopThreeD::new();
    let first = prepare(&mut cache, renderer, &runner, L, 0)?;
    assert_eq!(first.target_action, LayeredTargetAction::Created);
    let second = prepare(&mut cache, renderer, &runner, L, 0)?;
    assert_eq!(second.target_action, LayeredTargetAction::Reused);
    assert_eq!(second.target_bytes, first.target_bytes);
    // Capture the identical complete Logic draw plan, including the later screen UI.
    let output =
        renderer.create_render_target_with_budget(64, 64, RenderTargetBudget::new(65536, 65536))?;
    let mut images = DesktopImages::new();
    let frame = runner.extracted_frame().ok_or("snapshot")?;
    let report = images::present_to_target(
        renderer,
        frame,
        runner.image_assets(),
        &mut images,
        FrameBudget::default(),
        (cache.color_target(), &output),
    )
    .map_err(|error| format!("composition: {error:?}"))?;
    println!("full composed capture report: {report:?}");
    let capture = renderer.capture_render_target(
        &output,
        CaptureCorrelation {
            frame_id: 0,
            texture_revision: 0,
        },
        1024 * 1024,
        Duration::from_secs(5),
    )?;
    let bgra = format!("{:?}", capture.format()).starts_with("Bgra");
    let rgb = |x: usize, y: usize| {
        let p = &capture.pixels()[(y * 64 + x) * 4..][..4];
        if bgra {
            [p[2], p[1], p[0]]
        } else {
            [p[0], p[1], p[2]]
        }
    };
    assert_eq!(
        rgb(4, 4),
        [0, 0, 255],
        "screen UI must be above the 3D output"
    );
    assert!(
        rgb(22, 40)[0] > 150 && rgb(22, 40)[1] < 10,
        "near Detail must occlude Base"
    );
    assert!(
        rgb(42, 40)[1] > 150 && rgb(42, 40)[0] < 10,
        "far Detail must be occluded by Base"
    );
    if let Ok(path) = std::env::var("SIM_LOGIC_LAYERED_CAPTURE") {
        let mut ppm = b"P6\n64 64\n255\n".to_vec();
        for y in 0..64 {
            for x in 0..64 {
                ppm.extend_from_slice(&rgb(x, y));
            }
        }
        std::fs::write(path, ppm)?;
    }
    drop(output);
    images.clear();
    renderer.clear_frame_cache();
    key(&mut runner, PhysicalKeyCode::Digit2)?;
    assert_eq!(
        prepare(&mut cache, renderer, &runner, L, 0)?.target_action,
        LayeredTargetAction::Reused
    );
    key(&mut runner, PhysicalKeyCode::Space)?;
    prepare(&mut cache, renderer, &runner, L, 0)?;
    for scene in &cache.layered.scenes {
        assert_eq!(scene.scene.as_ref().ok_or("scene")?.instances().len(), 1);
    }
    renderer.resize(96, 64)?;
    let short = L.with_layered_limits(LayeredRenderLimits3d::new(
        16,
        16,
        48,
        2 * 1024 * 1024,
        first.target_bytes,
    ));
    for _ in 0..2 {
        assert!(matches!(
            prepare(&mut cache, renderer, &runner, short, 0),
            Err(DesktopThreeDError::LayeredRejected(
                LayeredRejection3d::TargetBudget { .. }
            ))
        ));
        assert!(cache.color_target().is_none());
        assert_eq!(
            cache
                .layered
                .target
                .as_ref()
                .ok_or("old target")?
                .allocation_bytes(),
            first.target_bytes
        );
    }
    let resized = prepare(&mut cache, renderer, &runner, L, 0)?;
    assert_eq!(resized.target_action, LayeredTargetAction::Replaced);
    assert_eq!(
        resized.peak_target_bytes,
        first.target_bytes + resized.target_bytes
    );
    renderer.set_scale_factor(2.0)?;
    assert_eq!(
        prepare(&mut cache, renderer, &runner, L, 0)?.target_action,
        LayeredTargetAction::Replaced
    );
    pollster::block_on(renderer.recover_device_and_surface())?;
    images.clear();
    cache.restore(renderer)?;
    assert!(cache.color_target().is_none());
    let restored = prepare(&mut cache, renderer, &runner, L, 1)?;
    assert_eq!(restored.target_action, LayeredTargetAction::Replaced);
    assert_eq!(restored.peak_target_bytes, restored.target_bytes * 2);
    assert_eq!(
        prepare(&mut cache, renderer, &runner, L, 1)?.target_action,
        LayeredTargetAction::Reused
    );
    println!(
        "LAYERED_GPU_PASS full UI capture, mutual depth, reuse, effect edit, migration, resize, rejected replacement, DPI, actual device restoration; {:?}",
        renderer.diagnostics()
    );
    Ok(())
}
