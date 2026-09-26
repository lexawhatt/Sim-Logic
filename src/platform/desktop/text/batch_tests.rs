//! Public CPU snapshots drive the same grouping used by preflight and drawing.

use super::*;
use crate::{
    desktop::draw_plan::{self, DesktopDraw},
    prelude::*,
};

const FONT: &[u8] = include_bytes!("../../../../tests/assets/text/DejaVuSans.ttf");

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    #[cfg(target_os = "linux")]
    Replace,
}
#[cfg(target_os = "linux")]
#[derive(Resource, Default)]
struct Step(u8);
#[cfg(target_os = "linux")]
#[derive(Component)]
struct First;

fn application() -> LogicResult<Application<Action>> {
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(1)
            .with_max_screen_texts(512)
            .with_max_screen_text_bytes(4096)
            .with_max_screen_text_glyphs(4096),
    );
    Ok(Application::new(config)?)
}

fn label(font: &TextFont, text: &str, order: f32) -> LogicResult<ScreenTextVisual> {
    let mut label =
        ScreenTextVisual::new(font.clone(), text, LogicalScreenPosition::new(3.25, 28.5))?;
    label.set_draw_order_depth(order)?;
    Ok(label)
}

#[test]
fn glyph_groups_preserve_order_boundaries_and_bounded_placement_count() -> LogicResult {
    let mut app = application()?;
    let a = app.register_font(FONT.to_vec(), TextSettings::new(18.0)?)?;
    let b = app.register_font_style(&a, TextSettings::new(20.0)?)?;
    let image = app.register_image_rgba8(1, 1, &[255; 4])?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let labels = (0..257)
        .map(|i| label(&a, "A", i as f32))
        .collect::<LogicResult<Vec<_>>>()?;
    let mut rectangle = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(0.0, 0.0),
        LogicalScreenVector::new(20.0, 20.0),
        Color::WHITE,
    )?;
    rectangle.set_draw_order_depth(257.0)?;
    let mut image = ScreenImageVisual::new(
        image,
        LogicalScreenPosition::new(0.0, 0.0),
        LogicalScreenVector::new(1.0, 1.0),
    )?;
    image.set_draw_order_depth(262.0)?;
    let mut hidden = label(&a, "Hidden", 265.0)?;
    hidden.set_clip(ScreenClip::Empty);
    let tail = [
        label(&a, "A", 258.0)?,
        label(&b, "B", 259.0)?,
        label(&a, "A", 260.0)?,
        label(&a, " ", 261.0)?,
        label(&a, "A", 263.0)?,
        label(&a, "", 264.0)?,
        hidden,
        label(&a, "A", 266.0)?,
    ];
    let world = app.register_world("group-plan", move |world| {
        world.spawn(camera)?;
        for label in &labels {
            world.spawn(label.clone())?;
        }
        world.spawn(rectangle)?;
        world.spawn(image)?;
        for label in &tail {
            world.spawn(label.clone())?;
        }
        Ok(())
    })?;
    let runner = app.build_headless(world)?;
    let snapshot = runner.extracted_frame().unwrap();
    let mut groups = Vec::new();
    let mut expanded = Vec::new();
    for draw in draw_plan::draws(snapshot, true) {
        match draw? {
            DesktopDraw::Single(draw) => expanded.push(draw),
            DesktopDraw::TextBatch(range) => {
                groups.push(range.len());
                expanded.extend(range.map(|index| ScreenDraw::Text { index }));
            }
        }
    }
    assert_eq!(groups, [256, 1, 1, 1, 2, 1, 1]);
    assert_eq!(expanded, snapshot.screen_draws());
    assert!(
        draw_plan::draws(snapshot, false).all(|draw| matches!(draw, Ok(DesktopDraw::Single(_))))
    );
    assert!(DesktopConfig::default().text_batching());
    let mut config = DesktopConfig::default();
    assert!(!config.set_text_batching(false).text_batching());
    Ok(())
}

#[cfg(target_os = "linux")]
fn runner() -> LogicResult<HeadlessRunner<Action>> {
    let mut app = application()?;
    app.bind_key(PhysicalKeyCode::Enter, Action::Replace)?;
    app.add_world_replacement_on_press_system();
    app.approve_component::<First>()?;
    let a = app.register_font(FONT.to_vec(), TextSettings::new(18.0)?)?;
    let b = app.register_font_style(&a, TextSettings::new(20.0)?)?;
    app.add_fallible_frame_system(
        move |mut step: ResMut<Step>,
              mut labels: Query<(&mut ScreenTextVisual, Option<&First>)>|
              -> LogicResult {
            for (mut label, first) in &mut labels {
                if step.0 == 8 {
                    label.set_text("")?;
                }
                if first.is_none() {
                    continue;
                }
                match step.0 {
                    0 => label.set_position(LogicalScreenPosition::new(-2.25, 30.25))?,
                    1 => label.set_tint(Color::rgba(0.5, 0.25, 1.0, 0.5))?,
                    2 => label.set_clip(ScreenClip::Rectangle(sim_engine::ScreenClipRect::new(
                        LogicalScreenPosition::new(0.0, 0.0),
                        LogicalScreenPosition::new(24.0, 48.0),
                    )?)),
                    3 => label.set_text("B")?,
                    4 => label.set_clip(ScreenClip::Empty),
                    5 => label.set_clip(ScreenClip::Unclipped),
                    6 => label.set_text("Longer text")?,
                    7 => label.set_font(b.clone())?,
                    _ => (),
                }
            }
            step.0 += 1;
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let labels = [
        label(&a, "A", 0.0)?,
        label(&a, "A", 1.0)?,
        label(&a, " ", 2.0)?,
    ];
    let replacement = labels[0].clone();
    let target = app.register_world("next-batch-world", move |world| {
        world.spawn(camera)?;
        world.spawn(replacement.clone())?;
        world.insert_resource(Step(9))?;
        Ok(())
    })?;
    let world = app.register_world("batch-update", move |world| {
        world.spawn(camera)?;
        world.spawn((labels[0].clone(), First))?;
        world.spawn(labels[1].clone())?;
        world.spawn(labels[2].clone())?;
        world.insert_resource(Step::default())?;
        world.insert_resource(WorldReplacementOnPress::new(Action::Replace, target))?;
        Ok(())
    })?;
    Ok(app.build_headless(world)?)
}

#[cfg(target_os = "linux")]
#[path = "batch_gpu_tests.rs"]
mod gpu;
