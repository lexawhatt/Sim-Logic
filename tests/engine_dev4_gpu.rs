//! Public-API regression for Engine revision 1afbc7c5a71a7aabafe41d16ac2502bef7424e2f.
//!
//! This manual Linux/X11 test creates one hidden window, four one-triangle
//! resources and one replacement logical device. It does not present, inspect
//! desktop input, modify Engine or claim GPU pixel correctness. `restore_scene3d`
//! is the passing control; standalone `restore_mesh3d` must obey the same material
//! contract. On the pinned dev.4 source the latter rejects alpha and resets UV,
//! addressing and material alpha policy, so this ignored regression must fail.
//! A future corrected Engine pin should make it pass without weakening assertions.
//!
//! Run explicitly on an available X11/XWayland display and Vulkan adapter:
//! `timeout 60s cargo test --offline --features desktop --test engine_dev4_gpu -- --ignored --nocapture --test-threads=1`

#![cfg(all(feature = "desktop", target_os = "linux"))]

use std::{error::Error, sync::Arc};

use sim_engine::{
    Color, ImageBudget, ImageSampling, Mesh3d, MeshStyle3d, RetainedMesh3d, Scene3d,
    SurfaceStyle3d, Texture3d, TextureAddressMode3d, TextureCoordinate2d, TextureMaterial3d,
    TextureUvTransform3d, Transform3d, Vec2, Vec3, WgpuRenderer,
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::x11::EventLoopBuilderExtX11,
    window::{Window, WindowId},
};

type ProbeResult<T = ()> = Result<T, Box<dyn Error>>;

struct Case {
    name: &'static str,
    mesh: RetainedMesh3d,
}

fn source_triangle() -> ProbeResult<Mesh3d> {
    Ok(Mesh3d::textured(
        vec![
            Vec3::new(-0.5, -0.5, 0.0)?,
            Vec3::new(0.5, -0.5, 0.0)?,
            Vec3::new(0.0, 0.5, 0.0)?,
        ],
        vec![
            TextureCoordinate2d::new(0.0, 0.0)?,
            TextureCoordinate2d::new(1.0, 0.0)?,
            TextureCoordinate2d::new(0.5, 1.0)?,
        ],
        vec![0, 1, 2],
        Vec::new(),
    )?)
}

fn cases(renderer: &WgpuRenderer) -> ProbeResult<[Case; 4]> {
    let topology = renderer.create_mesh3d(source_triangle()?)?;
    let budget = ImageBudget::new(1, 1, 4)?;
    let opaque = renderer.create_texture3d_rgba8_with_alpha(1, 1, vec![255; 4], budget)?;
    let translucent =
        renderer.create_texture3d_rgba8_with_alpha(1, 1, vec![255, 0, 0, 128], budget)?;
    let uv = TextureUvTransform3d::new(Vec2::new(-2.0, 3.0), Vec2::new(0.25, -0.5))?;
    let make = |name, texture: &Texture3d, tint, transform, address| -> ProbeResult<Case> {
        let material = TextureMaterial3d::with_alpha(texture, ImageSampling::Nearest, tint)?
            .with_uv_transform(transform)
            .with_address_mode(address);
        Ok(Case {
            name,
            mesh: renderer.with_mesh3d_material(&topology, &material)?,
        })
    };
    Ok([
        make(
            "transparent texel",
            &translucent,
            Color::WHITE,
            TextureUvTransform3d::IDENTITY,
            TextureAddressMode3d::Clamp,
        )?,
        make(
            "transparent tint",
            &opaque,
            Color::rgba(1.0, 1.0, 1.0, 0.5),
            TextureUvTransform3d::IDENTITY,
            TextureAddressMode3d::Clamp,
        )?,
        make(
            "signed UV and repeat",
            &opaque,
            Color::WHITE,
            uv,
            TextureAddressMode3d::Repeat,
        )?,
        make(
            "opaque pixels with alpha-capable material",
            &opaque,
            Color::WHITE,
            TextureUvTransform3d::IDENTITY,
            TextureAddressMode3d::Clamp,
        )?,
    ])
}

fn check_material(
    expected: &TextureMaterial3d,
    actual: &TextureMaterial3d,
    alpha_replacement: &Texture3d,
) -> ProbeResult {
    let mut failures = Vec::new();
    if actual.sampling() != expected.sampling() || actual.tint() != expected.tint() {
        failures.push("sampling or tint changed".to_owned());
    }
    if actual.uv_transform() != expected.uv_transform() {
        failures.push("UV scale/offset changed".to_owned());
    }
    if actual.address_mode() != expected.address_mode() {
        failures.push("texture address mode changed".to_owned());
    }
    if actual.texture().options() != expected.texture().options()
        || actual.texture().pixels() != expected.texture().pixels()
    {
        failures.push("texture pixels or alpha/mip policy changed".to_owned());
    }
    // There is deliberately no public require_opaque_texture getter. The
    // supported rebinding operation observes its policy without implementation
    // access or texture mutation. All four source materials permit this edit.
    expected.with_texture(alpha_replacement)?;
    if let Err(error) = actual.with_texture(alpha_replacement) {
        failures.push(format!("alpha-capable material policy changed: {error:?}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; ").into())
    }
}

fn probe(renderer: &mut WgpuRenderer) -> ProbeResult<Vec<String>> {
    if renderer.adapter_backend() != "vulkan" {
        return Err("this manual fixture requires Vulkan".into());
    }
    println!(
        "Engine dev.4 restoration: adapter={} backend={}",
        renderer.adapter_name(),
        renderer.adapter_backend()
    );
    let cases = cases(renderer)?;
    let mut scene = Scene3d::with_alpha_background(Color::TRANSPARENT)?;
    let style = MeshStyle3d::surface(SurfaceStyle3d::blend(Color::WHITE)?);
    let mut ids = Vec::with_capacity(cases.len());
    for case in &cases {
        ids.push(scene.try_push(&case.mesh, Transform3d::IDENTITY, style)?);
    }

    // Exercise actual public device replacement, not a same-device restore no-op.
    pollster::block_on(renderer.recover_device_and_surface())?;
    let alpha_replacement = renderer.create_texture3d_rgba8_with_alpha(
        1,
        1,
        vec![0, 255, 0, 64],
        ImageBudget::new(1, 1, 4)?,
    )?;
    let report = renderer.restore_scene3d(&mut scene)?;
    assert_eq!(report.object_count(), cases.len());
    assert_eq!(report.migrated_object_count(), cases.len());
    assert_eq!(
        report.restored_mesh_count(),
        1,
        "shared topology is restored once"
    );
    assert_eq!(report.restored_texture_count(), 2);
    for (case, id) in cases.iter().zip(ids) {
        let instance = scene.instance(id)?;
        assert_eq!(instance.id(), id);
        assert_eq!(instance.transform(), Transform3d::IDENTITY);
        assert_eq!(instance.style(), style);
        check_material(
            case.mesh.material().ok_or("source material missing")?,
            instance
                .mesh()
                .material()
                .ok_or("scene control material missing")?,
            &alpha_replacement,
        )?;
        println!("scene restore control: {} preserved", case.name);
    }

    let mut failures = Vec::with_capacity(cases.len());
    for case in &cases {
        let result = renderer.restore_mesh3d(&case.mesh);
        let issue = match result {
            Err(error) => Some(format!("restore returned {error:?}")),
            Ok(restored) => check_material(
                case.mesh.material().ok_or("source material missing")?,
                restored.material().ok_or("standalone material missing")?,
                &alpha_replacement,
            )
            .err()
            .map(|error| error.to_string()),
        };
        if let Some(issue) = issue {
            let message = format!("{}: {issue}", case.name);
            println!("standalone restore violation: {message}");
            failures.push(message);
        } else {
            println!("standalone restore: {} preserved", case.name);
        }
    }
    Ok(failures)
}

#[derive(Default)]
struct Fixture {
    result: Option<ProbeResult<Vec<String>>>,
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
                        .with_title("Sim;Logic Engine dev.4 restoration regression")
                        .with_inner_size(PhysicalSize::new(64, 64))
                        .with_visible(false),
                )?,
            );
            let mut renderer = pollster::block_on(WgpuRenderer::new(window, 64, 64))?;
            probe(&mut renderer)
        })());
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

#[test]
#[ignore = "known upstream dev.4 standalone material recovery bug; manual Linux/X11/Vulkan"]
fn standalone_mesh_restore_preserves_material_like_scene_restore() -> ProbeResult {
    let mut builder = EventLoop::builder();
    builder.with_x11().with_any_thread(true);
    let mut fixture = Fixture::default();
    builder.build()?.run_app(&mut fixture)?;
    let failures = fixture.result.ok_or("renderer never initialized")??;
    assert!(
        failures.is_empty(),
        "standalone material restoration violated its contract:\n{}",
        failures.join("\n")
    );
    Ok(())
}
