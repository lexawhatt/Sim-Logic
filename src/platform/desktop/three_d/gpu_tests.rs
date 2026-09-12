//! Manual public-ECS-to-private-desktop-cache acceptance on Linux/X11/Vulkan.
//!
//! Two attributed, lit Blend triangles share topology and a mipmapped texture.
//! Four requested revisions exercise alias detachment then unique capacity reuse.
//! The dev.5 mutation companion and the original revision fixture each perform
//! one device replacement and then an unchanged preparation. This inspects
//! resources/reports after real offscreen draws;
//! it is not pixel readback, confirmed surface presentation, or a performance gate.

use super::*;
use crate::{
    prelude::*,
    screen::{ImageFilter, ImageRegion},
    three_d::{TextureAsset3d, TextureVisual3d},
};
use sim_engine::{
    AmbientLight3d, Fog3d, Lighting3d, Mesh3dAttributes, SurfaceLighting3d, SurfaceStyle3d,
    TextureAddressMode3d, TextureCoordinate2d, TextureUvTransform3d, Vec2,
};
use std::{sync::Arc, time::Duration};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::x11::EventLoopBuilderExtX11,
    window::{Window, WindowId},
};

#[path = "gpu_tests/mutations.rs"]
mod mutations;

const SIZE: u32 = 64;
const LIMITS: ThreeDRenderLimits = ThreeDRenderLimits::new(0, 2, SIZE as u64 * SIZE as u64)
    .with_mesh_limits(2, 16 * 1024)
    .with_texture_limits(128, 168);

#[derive(Component)]
struct ProbeObject(u8);

fn asset(offset: f32) -> LogicResult<MeshAsset3d> {
    let attributes = Mesh3dAttributes::new()
        .with_texture_coordinates(vec![
            TextureCoordinate2d::new(0.0, 0.0)?,
            TextureCoordinate2d::new(1.0, 0.0)?,
            TextureCoordinate2d::new(0.5, 1.0)?,
        ])
        .with_vertex_colors(vec![Color::rgba(0.8, 1.0, 0.9, 0.8); 3])?
        .with_normals(vec![Vec3::Z; 3])?;
    Ok(MeshAsset3d::new(Mesh3d::with_attributes(
        vec![
            Vec3::new(-0.6 + offset, -0.4, 0.0)?,
            Vec3::new(offset, -0.4, 0.0)?,
            Vec3::new(-0.3 + offset, 0.4, 0.0)?,
        ],
        vec![0, 1, 2],
        vec![],
        attributes,
    )?)?)
}

fn textures() -> LogicResult<[TextureVisual3d; 3]> {
    let original = TextureAsset3d::rgba8(4, 4, vec![255; 64], 64)?;
    let region = ImageRegion::new(1, 1, 2, 2)?;
    let first = original.with_region_update(region, 8, &[255, 0, 0, 128].repeat(4), 64)?;
    let second = first.with_region_update(region, 8, &[0, 255, 0, 64].repeat(4), 64)?;
    let uv = TextureUvTransform3d::new(Vec2::new(-2.0, 3.0), Vec2::new(0.25, -0.5))?;
    Ok([original, first, second].map(|asset| {
        TextureVisual3d::new(asset)
            .with_mipmaps(true)
            .with_filter(ImageFilter::Linear)
            .with_address_mode(TextureAddressMode3d::Repeat)
            .with_uv_transform(uv)
    }))
}

struct Game {
    runner: HeadlessRunner<u8>,
    assets: [MeshAsset3d; 3],
    textures: [TextureVisual3d; 3],
    sources: [LogicEntity; 2],
}

fn game() -> LogicResult<Game> {
    let assets = [asset(0.0)?, asset(0.1)?, asset(0.2)?];
    let textures = textures()?;
    let mut config = AppConfig::default();
    config.set_three_d_render_limits(LIMITS);
    let mut app = Application::<u8>::new(config)?;
    app.approve_components::<(ProbeObject,)>()?;
    for (key, action) in [
        (PhysicalKeyCode::Digit1, 1),
        (PhysicalKeyCode::Digit2, 2),
        (PhysicalKeyCode::Digit3, 3),
        (PhysicalKeyCode::Digit4, 4),
        (PhysicalKeyCode::Digit5, 5),
        (PhysicalKeyCode::Digit6, 6),
        (PhysicalKeyCode::Digit7, 7),
        (PhysicalKeyCode::Digit8, 8),
        (PhysicalKeyCode::Digit9, 9),
        (PhysicalKeyCode::KeyR, 0),
    ] {
        app.bind_key(key, action)?;
    }
    let geometry = assets.clone();
    let images = textures.clone();
    let plain = MeshAsset3d::new(Mesh3d::new(
        assets[1].mesh().vertices().to_vec(),
        vec![0, 1, 2],
    )?)?;
    app.add_fallible_frame_system(
        move |input: FrameInput<u8>,
              mut view: ResMut<View3d>,
              mut objects: Query<(&ProbeObject, &mut MeshVisual3d)>|
              -> LogicResult {
            if input.has_press_occurrence(5) {
                view.set_background(Color::rgba(0.2, 0.3, 0.4, 0.5))?;
            }
            for (marker, mut visual) in &mut objects {
                if marker.0 != 0 {
                    if input.has_press_occurrence(0) {
                        visual.set_texture(None)?;
                    }
                    continue;
                }
                for action in [1, 2, 3, 4, 6, 7, 8, 9] {
                    if input.has_press_occurrence(action) {
                        match action {
                            1 | 2 => visual.set_asset(geometry[action as usize].clone())?,
                            3 | 4 => {
                                visual.set_texture(Some(images[action as usize - 2].clone()))?;
                            }
                            6 => visual.set_texture(None)?,
                            7 => visual.set_texture(Some(images[0].clone()))?,
                            8 => {
                                let unlit =
                                    visual.surface().with_lighting(SurfaceLighting3d::Unlit);
                                visual.set_surface(unlit)?;
                                visual.set_texture(None)?;
                                visual.set_asset(plain.clone())?;
                            }
                            9 => {
                                visual.set_asset(geometry[0].clone())?;
                                let lit =
                                    visual.surface().with_lighting(SurfaceLighting3d::Lambert);
                                visual.set_surface(lit)?;
                                visual.set_texture(Some(images[0].clone()))?;
                            }
                            _ => unreachable!("bounded fixture actions"),
                        }
                    }
                }
            }
            Ok(())
        },
    );
    let surface = SurfaceStyle3d::blend(Color::rgba(1.0, 1.0, 1.0, 0.75))?
        .with_lighting(SurfaceLighting3d::Lambert)
        .with_fog(true);
    let mut visual = MeshVisual3d::with_surface(assets[0].clone(), Transform3d::IDENTITY, surface)?;
    visual.set_texture(Some(textures[0].clone()))?;
    let mut view = View3d::new(Vec3::new(0.0, 0.0, 3.0)?, Vec3::ZERO)?;
    view.set_surface_policy(ThreeDSurfacePolicy::Native);
    view.set_background(Color::TRANSPARENT)?;
    view.set_lighting(Lighting3d::new(AmbientLight3d::new(Color::WHITE, 0.6)?));
    view.set_fog(Some(Fog3d::new(Color::rgb(0.3, 0.4, 0.5), 1.0, 0.1)?));
    let camera = ActiveCamera2d::centered(1.0)?;
    let factory = app.register_world("gpu cache fixture", move |world| {
        world.spawn(camera)?;
        world.spawn((ProbeObject(0), visual.clone()))?;
        world.spawn((ProbeObject(1), visual.clone()))?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let runner = app.build_headless(factory)?;
    let mut sources = [None, None];
    for (entity, marker) in runner.components::<ProbeObject>() {
        sources[usize::from(marker.0)] = Some(entity);
    }
    Ok(Game {
        runner,
        assets,
        textures,
        sources: [
            sources[0].ok_or("source zero")?,
            sources[1].ok_or("source one")?,
        ],
    })
}

fn advance(game: &mut Game, key: PhysicalKeyCode) -> LogicResult {
    let events = [
        InputEvent::key(key, ButtonState::Pressed),
        InputEvent::key(key, ButtonState::Released),
    ];
    let viewport = LogicalViewport::new(SIZE as f32, SIZE as f32)?;
    let outcome = game
        .runner
        .advance_frame(FrameRequest::new(Duration::ZERO, &events, viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err(format!("headless frame rejected: {outcome:?}").into());
    };
    assert!(report.failure().is_none(), "{:?}", report.failure());
    Ok(())
}

fn prepare(game: &Game, cache: &mut DesktopThreeD, renderer: &mut WgpuRenderer) -> LogicResult {
    let snapshot = game
        .runner
        .extracted_frame()
        .ok_or("extracted frame")?
        .three_d()
        .ok_or("3D snapshot")?;
    let report = cache.prepare(renderer, snapshot, LIMITS)?;
    assert!(
        report.draw_call_count() > 0,
        "fixture must execute an offscreen draw"
    );
    Ok(())
}

fn object_id(cache: &DesktopThreeD, source: LogicEntity) -> LogicResult<Object3dId> {
    cache
        .custom
        .object_sources()
        .find_map(|(id, current, _)| (current == source).then_some(id))
        .ok_or_else(|| "managed source lost its Engine object".into())
}

fn scene(cache: &DesktopThreeD) -> LogicResult<&Scene3d> {
    cache
        .scene
        .as_ref()
        .ok_or_else(|| "retained scene missing".into())
}

fn mip_pixels(cache: &DesktopThreeD, id: Object3dId) -> LogicResult<Vec<Vec<u8>>> {
    let texture = scene(cache)?
        .instance(id)?
        .mesh()
        .material()
        .ok_or("texture material missing")?
        .texture();
    Ok((0..texture.mip_level_count())
        .map(|level| {
            texture
                .mip_level_pixels(level)
                .map(<[u8]>::to_vec)
                .ok_or("mip missing")
        })
        .collect::<Result<_, _>>()?)
}

fn assert_resources(game: &Game, cache: &DesktopThreeD, ids: [Object3dId; 2]) -> LogicResult {
    for (index, id) in ids.into_iter().enumerate() {
        assert_eq!(object_id(cache, game.sources[index])?, id);
        let expected = game.runner.component::<MeshVisual3d>(game.sources[index])?;
        let instance = scene(cache)?.instance(id)?;
        assert_eq!(instance.mesh().source(), expected.asset().mesh());
        assert_eq!(instance.style().surface_style(), Some(expected.surface()));
        let material = instance.mesh().material().ok_or("material missing")?;
        let expected = expected.texture().ok_or("CPU texture missing")?;
        assert_eq!(material.texture().pixels(), expected.asset().pixels());
        assert_eq!(material.texture().mip_level_count(), 3);
        assert_eq!(material.uv_transform(), expected.uv_transform());
        assert_eq!(material.address_mode(), expected.address_mode());
        assert!(material.texture().preserves_alpha());
    }
    Ok(())
}

fn run(renderer: &mut WgpuRenderer) -> LogicResult {
    assert_eq!(
        renderer.adapter_backend(),
        "vulkan",
        "Vulkan fixture required"
    );
    println!(
        "Logic dev.4 cache acceptance: {} / {}",
        renderer.adapter_name(),
        renderer.adapter_backend()
    );
    mutations::run(renderer)?;
    let mut game = game()?;
    let mut cache = DesktopThreeD::new();
    prepare(&game, &mut cache, renderer)?;
    let ids = [
        object_id(&cache, game.sources[0])?,
        object_id(&cache, game.sources[1])?,
    ];
    assert_eq!(scene(&cache)?.statistics().mesh_count(), 1);
    assert_eq!(scene(&cache)?.statistics().texture_count(), 1);
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(cache.updates(), DesktopThreeDUpdates::default());
    for (key, index) in [(PhysicalKeyCode::Digit1, 1), (PhysicalKeyCode::Digit2, 2)] {
        advance(&mut game, key)?;
        prepare(&game, &mut cache, renderer)?;
        let updates = cache.updates();
        assert_eq!(updates.mesh_updates, 1);
        assert!(updates.mesh_upload_bytes > 0);
        assert_eq!(updates.mesh_gpu_allocations > 0, index == 1);
        assert_eq!(scene(&cache)?.statistics().mesh_count(), 2);
        assert_eq!(
            scene(&cache)?.instance(ids[0])?.mesh().source(),
            game.assets[index].mesh()
        );
        assert_eq!(
            scene(&cache)?.instance(ids[1])?.mesh().source(),
            game.assets[0].mesh()
        );
        assert_resources(&game, &cache, ids)?;
        println!("mesh revision {index}: {updates:?}");
    }
    for (key, index) in [(PhysicalKeyCode::Digit3, 1), (PhysicalKeyCode::Digit4, 2)] {
        advance(&mut game, key)?;
        prepare(&game, &mut cache, renderer)?;
        let updates = cache.updates();
        assert_eq!(updates.texture_updates, 1);
        assert_eq!(updates.texture_upload_bytes, 16 + 16 + 4);
        assert_eq!(updates.texture_gpu_allocations, usize::from(index == 1));
        assert_eq!(scene(&cache)?.statistics().texture_count(), 2);
        assert_eq!(
            scene(&cache)?
                .instance(ids[1])?
                .mesh()
                .material()
                .ok_or("neighbor material")?
                .texture()
                .pixels(),
            game.textures[0].asset().pixels()
        );
        assert_resources(&game, &cache, ids)?;
        println!("texture revision {index}: {updates:?}");
    }
    let levels = [mip_pixels(&cache, ids[0])?, mip_pixels(&cache, ids[1])?];
    let environment = (
        scene(&cache)?.lighting(),
        scene(&cache)?.fog(),
        scene(&cache)?.background(),
    );
    pollster::block_on(renderer.recover_device_and_surface())?;
    cache.restore(renderer)?;
    assert_resources(&game, &cache, ids)?;
    assert_eq!(
        (
            scene(&cache)?.lighting(),
            scene(&cache)?.fog(),
            scene(&cache)?.background()
        ),
        environment
    );
    assert_eq!(
        [mip_pixels(&cache, ids[0])?, mip_pixels(&cache, ids[1])?],
        levels
    );
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(cache.updates(), DesktopThreeDUpdates::default());
    assert_resources(&game, &cache, ids)?;
    println!(
        "scene-owned cache recovery: IDs, divergent meshes/textures, mip bytes, UV, Repeat, alpha and environment preserved; next offscreen draw succeeded"
    );
    Ok(())
}

#[derive(Default)]
struct Fixture {
    result: Option<LogicResult>,
}

impl ApplicationHandler for Fixture {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.result.is_some() {
            return;
        }
        self.result = Some((|| {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title("Sim;Logic dev.4 cache acceptance")
                        .with_inner_size(PhysicalSize::new(SIZE, SIZE))
                        .with_visible(false),
                )?,
            );
            let mut renderer = pollster::block_on(WgpuRenderer::new(window, SIZE, SIZE))?;
            run(&mut renderer)
        })());
        event_loop.exit();
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

#[test]
#[ignore = "manual Linux/X11/Vulkan cache acceptance with two device replacements"]
fn managed_mesh_texture_revisions_and_recovery() -> LogicResult {
    let mut builder = EventLoop::builder();
    builder.with_x11().with_any_thread(true);
    let mut fixture = Fixture::default();
    builder.build()?.run_app(&mut fixture)?;
    fixture.result.ok_or("renderer never initialized")?
}
