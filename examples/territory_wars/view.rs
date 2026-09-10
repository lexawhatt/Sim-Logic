//! Managed rectangle/image pools. This is presentation, not a new renderer.

mod hud;
mod map;

use sim_logic::prelude::*;

use super::{
    app::Session,
    drawing::{self, Canvas},
    layout::Layout,
};

#[derive(Component)]
pub struct RectSlot(pub usize);
#[derive(Component)]
pub struct GlyphSlot(pub usize);

#[derive(Resource)]
pub struct Presentation {
    canvas: Canvas,
    font: ImageAssetId,
    rectangles: Vec<ScreenRectangleVisual>,
    images: Vec<ScreenImageVisual>,
}

pub fn limits() -> RenderLimits {
    let scene = sim_engine::SceneBudget::new(
        drawing::MAX_RECTS,
        0,
        drawing::MAX_RECTS * 12,
        4 * 1024 * 1024,
        8 * 1024 * 1024,
        16 * 1024 * 1024,
        drawing::MAX_RECTS,
    );
    RenderLimits::new(
        0,
        scene,
        FrameLimits::new(
            drawing::MAX_GLYPHS + 2,
            drawing::MAX_RECTS + drawing::MAX_GLYPHS,
            drawing::MAX_RECTS * 12 + drawing::MAX_GLYPHS * 6,
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            drawing::MAX_RECTS + drawing::MAX_GLYPHS,
        ),
    )
    .with_max_world_rectangles(0)
    .with_max_world_lines(0)
    .with_max_screen_rectangles(drawing::MAX_RECTS)
    .with_screen_scene_budget(scene)
    .with_max_screen_images(drawing::MAX_GLYPHS)
}

pub fn spawn(world: &mut WorldBuilder, font: ImageAssetId) -> Result<(), WorldBuildError> {
    let canvas = Canvas::new().map_err(|e| WorldBuildError::user(e.to_string()))?;
    let mut rectangles = Vec::new();
    let mut images = Vec::new();
    rectangles
        .try_reserve_exact(drawing::MAX_RECTS)
        .map_err(|e| WorldBuildError::user(e.to_string()))?;
    images
        .try_reserve_exact(drawing::MAX_GLYPHS)
        .map_err(|e| WorldBuildError::user(e.to_string()))?;
    world.insert_resource(Presentation {
        canvas,
        font,
        rectangles,
        images,
    })?;
    // Empty slots have no visual at all. In particular, 1,800 invisible glyphs
    // must not become 1,800 image passes on Engine 0.2.
    for index in 0..drawing::MAX_RECTS {
        world.spawn(RectSlot(index))?;
    }
    for index in 0..drawing::MAX_GLYPHS {
        world.spawn(GlyphSlot(index))?;
    }
    Ok(())
}

type Rectangles<'w, 's> = Query<
    'w,
    's,
    (
        LogicEntityRef,
        &'static RectSlot,
        Option<&'static mut ScreenRectangleVisual>,
    ),
>;
type Images<'w, 's> = Query<
    'w,
    's,
    (
        LogicEntityRef,
        &'static GlyphSlot,
        Option<&'static mut ScreenImageVisual>,
    ),
>;

pub fn draw(
    session: AppRes<Session>,
    viewport: FrameViewport,
    mut presentation: ResMut<Presentation>,
    mut rectangles: Rectangles,
    mut images: Images,
    mut commands: Commands,
) -> LogicResult {
    let layout = Layout::new(viewport.logical());
    let Presentation {
        canvas,
        font,
        rectangles: prepared_rectangles,
        images: prepared_images,
    } = &mut *presentation;
    canvas.clear();
    canvas.rect(0.0, 0.0, 1440.0, 900.0, Color::rgb8(10, 15, 22))?;
    map::draw(canvas, &session)?;
    hud::draw(canvas, &session)?;
    if session.debug {
        let previous_rectangles = prepared_rectangles.len();
        let previous_images = prepared_images.len();
        canvas.text(1032.0, 616.0, 1.0, "DRAW RECTS", Color::rgb8(133, 158, 173))?;
        canvas.number(1140.0, 616.0, 1.0, previous_rectangles as u32, Color::WHITE)?;
        canvas.text(
            1220.0,
            616.0,
            1.0,
            "DRAW GLYPHS",
            Color::rgb8(133, 158, 173),
        )?;
        canvas.number(1334.0, 616.0, 1.0, previous_images as u32, Color::WHITE)?;
    }
    // Complete numeric/viewport validation before publishing any component.
    prepared_rectangles.clear();
    prepared_images.clear();
    for (index, rect) in canvas.rects.iter().enumerate() {
        let mut visual = ScreenRectangleVisual::new(
            layout.position(rect.x, rect.y),
            layout.size(rect.width, rect.height),
            rect.color,
        )?;
        visual.set_draw_order_depth(index as f32)?;
        prepared_rectangles.push(visual);
    }
    for (index, glyph) in canvas.glyphs.iter().enumerate() {
        let mut visual = ScreenImageVisual::new(
            *font,
            layout.position(glyph.x, glyph.y),
            layout.size(glyph.width, glyph.height),
        )?;
        visual.set_source_region(Some(glyph.source))?;
        visual.set_tint(glyph.color)?;
        visual.set_draw_order_depth((drawing::MAX_RECTS + index) as f32)?;
        prepared_images.push(visual);
    }
    debug_assert_eq!(prepared_rectangles.len(), canvas.rect_count());
    debug_assert_eq!(prepared_images.len(), canvas.glyph_count());
    for (entity, slot, visual) in &mut rectangles {
        match (prepared_rectangles.get(slot.0), visual) {
            (Some(proposed), Some(mut current)) => {
                if *current != *proposed {
                    *current = *proposed;
                }
            }
            (Some(proposed), None) => commands.insert(entity.handle(), (*proposed,))?,
            (None, Some(_)) => commands.remove::<(ScreenRectangleVisual,)>(entity.handle())?,
            (None, None) => {}
        }
    }
    for (entity, slot, visual) in &mut images {
        match (prepared_images.get(slot.0), visual) {
            (Some(proposed), Some(mut current)) => {
                if *current != *proposed {
                    *current = *proposed;
                }
            }
            (Some(proposed), None) => commands.insert(entity.handle(), (*proposed,))?,
            (None, Some(_)) => commands.remove::<(ScreenImageVisual,)>(entity.handle())?,
            (None, None) => {}
        }
    }
    Ok(())
}

pub(super) fn faction_color(index: usize) -> Color {
    let (r, g, b) = [
        (76, 213, 176),
        (229, 104, 88),
        (88, 160, 224),
        (156, 188, 96),
        (169, 130, 221),
        (236, 192, 96),
        (225, 133, 172),
    ][index % 7];
    Color::rgb8(r, g, b)
}

pub(super) fn shade(color: Color, amount: f32) -> Color {
    Color::rgb(
        color.red() * amount,
        color.green() * amount,
        color.blue() * amount,
    )
}
