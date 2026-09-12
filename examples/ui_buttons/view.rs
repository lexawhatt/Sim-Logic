//! Managed text labels reuse two application fonts and only reshape changed values.

use sim_engine::{Layer, Scene, SceneBudget, ShapeStyle};
use sim_logic::prelude::*;

use super::{app, scene};

pub(super) const FONT: &[u8] = include_bytes!("../../tests/assets/text/DejaVuSans.ttf");
pub(super) const RECTANGLE_COUNT: usize = 7;
pub(super) const TEXT_COUNT: usize = 28;
const MAX_TEXT_BYTES: usize = 2048;
const MAX_TEXT_GLYPHS: usize = 1024;
const INK: Color = Color::rgb(0.75, 0.82, 0.89);
const MUTED: Color = Color::rgb(0.28, 0.36, 0.46);

/// The five displayed values; all format only when the visible value changes.
#[derive(Debug, Clone, Copy)]
pub enum Value {
    /// Successful COUNT clicks across Worlds.
    Clicks,
    /// Accepted gameplay presses across Worlds.
    Background,
    /// Cancelled captured gestures across Worlds.
    Cancellations,
    /// Requested World replacements.
    Replacements,
    /// Whole fixed simulation seconds in the current World.
    Seconds,
}

/// Per-label display cache avoids formatting and shaping unchanged values.
#[derive(Debug, Clone, Copy, Component)]
pub enum Label {
    /// Setup-time content never changed by the frame loop.
    Static,
    /// A persistent counter or World-local time value.
    Value {
        /// Determines the source value and its format.
        kind: Value,
        /// Last displayed value; None initializes on the first FrameUpdate.
        displayed: Option<u64>,
    },
}

/// Green indicates an active state; gray indicates an inactive state.
#[derive(Debug, Clone, Copy, Component)]
pub enum Indicator {
    /// Fixed simulation is running.
    Running,
    /// COUNT remains eligible for new presses and releases.
    CountEnabled,
}

fn rectangle(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
    layer: i32,
) -> LogicResult<ScreenRectangleVisual> {
    let mut visual = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(x, y),
        LogicalScreenVector::new(width, height),
        color,
    )?;
    visual.set_layer(Layer::new(layer));
    Ok(visual)
}

pub(super) fn panel() -> LogicResult<ScreenRectangleVisual> {
    rectangle(0.0, 0.0, 1000.0, 412.0, Color::rgb8(22, 33, 48), 0)
}

pub(super) fn buttons() -> LogicResult<[(scene::Button, ScreenRectangleVisual); 4]> {
    let mut result = Vec::new();
    for (index, button) in [
        scene::Button::Count,
        scene::Button::Pause,
        scene::Button::ToggleCount,
        scene::Button::NextWorld,
    ]
    .into_iter()
    .enumerate()
    {
        result.push((
            button,
            rectangle(
                24.0 + index as f32 * 240.0,
                96.0,
                224.0,
                56.0,
                Color::rgb8(44, 65, 86),
                1,
            )?,
        ));
    }
    result
        .try_into()
        .map_err(|_| "four button roles required".into())
}

fn text(
    font: &TextFont,
    content: &str,
    x: f32,
    baseline: f32,
    tint: Color,
) -> LogicResult<ScreenTextVisual> {
    let mut text = ScreenTextVisual::new(
        font.clone(),
        content,
        LogicalScreenPosition::new(x, baseline),
    )?;
    text.set_tint(tint)?;
    text.set_layer(Layer::new(3));
    Ok(text)
}

pub(super) fn labels(
    index: usize,
    body: &TextFont,
    title: &TextFont,
) -> LogicResult<Vec<(Label, ScreenTextVisual)>> {
    let mut labels = vec![
        (
            Label::Static,
            text(
                title,
                if index == 0 {
                    "Pointer buttons / Garden"
                } else {
                    "Pointer buttons / Harbor"
                },
                28.0,
                48.0,
                scene::accent(index),
            )?,
        ),
        (
            Label::Static,
            text(body, "Two Worlds / one persistent score", 28.0, 78.0, MUTED)?,
        ),
    ];
    for (index, caption) in ["Count +1", "Pause / resume", "Toggle count", "Next World"]
        .into_iter()
        .enumerate()
    {
        let mut label = text(body, caption, 136.0 + index as f32 * 240.0, 130.0, INK)?;
        label.set_alignment(TextAlignment::Center)?;
        labels.push((Label::Static, label));
    }
    labels.push((Label::Static, text(body, "Running", 44.0, 186.0, MUTED)?));
    labels.push((
        Label::Static,
        text(body, "Count enabled", 236.0, 186.0, MUTED)?,
    ));
    for (index, (caption, kind)) in [
        ("UI clicks", Value::Clicks),
        ("Game presses", Value::Background),
        ("Cancelled", Value::Cancellations),
        ("World changes", Value::Replacements),
        ("Simulation time", Value::Seconds),
    ]
    .into_iter()
    .enumerate()
    {
        let baseline = 234.0 + index as f32 * 34.0;
        labels.push((Label::Static, text(body, caption, 28.0, baseline, INK)?));
        let mut label = text(
            title,
            if matches!(kind, Value::Seconds) {
                "0 s"
            } else {
                "..."
            },
            450.0,
            baseline,
            INK,
        )?;
        label.set_alignment(TextAlignment::Right)?;
        labels.push((
            Label::Value {
                kind,
                displayed: None,
            },
            label,
        ));
    }
    for (index, caption) in [
        "Press then release on the same button.",
        "Drag out or lose focus to cancel.",
        "D toggles Count, even while held.",
        "P pauses / R clears dots / Esc exits.",
        "Green lights mean active.",
        "World change resets dots and time.",
        "The four application counters survive.",
    ]
    .into_iter()
    .enumerate()
    {
        labels.push((
            Label::Static,
            text(body, caption, 550.0, 234.0 + index as f32 * 25.0, MUTED)?,
        ));
    }
    labels.push((
        Label::Static,
        text(
            body,
            "Click the lower area to place dots. Pause freezes the game.",
            28.0,
            460.0,
            INK,
        )?,
    ));
    labels.push((
        Label::Static,
        text(
            body,
            "UI clicks never place dots. Up to 64 dots per World.",
            28.0,
            488.0,
            MUTED,
        )?,
    ));
    labels.push((
        Label::Static,
        text(
            body,
            "Display caps at 999999. Time updates once a second.",
            28.0,
            401.0,
            MUTED,
        )?,
    ));
    assert_eq!(labels.len(), TEXT_COUNT);
    Ok(labels)
}

pub(super) fn indicators() -> LogicResult<[(Indicator, ScreenRectangleVisual); 2]> {
    Ok([
        (
            Indicator::Running,
            rectangle(24.0, 173.0, 12.0, 12.0, scene::accent(0), 2)?,
        ),
        (
            Indicator::CountEnabled,
            rectangle(216.0, 173.0, 12.0, 12.0, scene::accent(0), 2)?,
        ),
    ])
}

fn refresh_labels(
    board: &scene::Board,
    counters: &app::Counters,
    labels: &mut Query<(&mut Label, &mut ScreenTextVisual)>,
) -> LogicResult {
    for (mut label, mut visual) in labels {
        let Label::Value { kind, displayed } = &mut *label else {
            continue;
        };
        let value = match kind {
            Value::Clicks => u64::from(counters.clicks),
            Value::Background => u64::from(counters.background),
            Value::Cancellations => u64::from(counters.cancellations),
            Value::Replacements => u64::from(counters.replacements),
            Value::Seconds => board.elapsed.as_secs(),
        }
        .min(999_999);
        if *displayed == Some(value) {
            continue;
        }
        let content = if matches!(kind, Value::Seconds) {
            format!("{value} s")
        } else {
            value.to_string()
        };
        visual.set_text(&content)?;
        *displayed = Some(value);
    }
    Ok(())
}

pub(super) fn refresh(
    board: &scene::Board,
    counters: &app::Counters,
    paused: bool,
    pointer: Option<PointerSample>,
    buttons: &mut Query<(LogicEntityRef, &scene::Button, &mut ScreenRectangleVisual)>,
    labels: &mut Query<(&mut Label, &mut ScreenTextVisual)>,
    indicators: &mut Query<(&Indicator, &mut ScreenRectangleVisual), Without<scene::Button>>,
) -> LogicResult {
    for (entity, button, mut rectangle) in buttons {
        let color = if !button.eligible(board.count_enabled) {
            Color::rgb8(45, 47, 54)
        } else if board.pointer.captured() == Some(entity.handle()) {
            Color::rgb8(153, 102, 31)
        } else if pointer.is_some_and(|pointer| rectangle.contains_pointer(pointer)) {
            Color::rgb8(59, 90, 119)
        } else {
            Color::rgb8(44, 65, 86)
        };
        rectangle.set_color(color)?;
    }
    for (indicator, mut rectangle) in indicators {
        let active = match indicator {
            Indicator::Running => !paused,
            Indicator::CountEnabled => board.count_enabled,
        };
        rectangle.set_color(if active {
            Color::rgb8(87, 220, 156)
        } else {
            Color::rgb8(69, 76, 86)
        })?;
    }
    refresh_labels(board, counters, labels)
}

pub(super) fn render_limits() -> LogicResult<RenderLimits> {
    let screen_vertices = RECTANGLE_COUNT * 12;
    let screen_bytes = RECTANGLE_COUNT * 1024;
    let screen = SceneBudget::new(
        RECTANGLE_COUNT,
        0,
        screen_vertices,
        screen_bytes,
        screen_bytes,
        screen_bytes,
        RECTANGLE_COUNT,
    );
    // Ask the pinned Engine for this shape's cost once during setup. Vertex
    // count alone does not describe upload bytes, and private vertex layouts
    // should not be guessed by the application.
    let mut probe = Scene::new(Color::TRANSPARENT)?;
    probe.try_circle(Vec2::ZERO, 0.2, ShapeStyle::filled(Color::WHITE))?;
    let statistics = probe.statistics();
    let world_vertices = app::MAX_MARKERS * statistics.estimated_tessellated_vertices();
    let world_bytes = app::MAX_MARKERS
        * statistics
            .estimated_upload_bytes()
            .max(statistics.retained_bytes())
            .max(probe.allocation_bytes());
    let world = SceneBudget::new(
        app::MAX_MARKERS,
        0,
        world_vertices,
        world_bytes,
        world_bytes,
        world_bytes,
        app::MAX_MARKERS,
    );
    let commands = RECTANGLE_COUNT + TEXT_COUNT + app::MAX_MARKERS;
    // Text may split rectangle runs. Two fixed font atlases each permit 4 MiB.
    let frame = FrameLimits::new(
        2 + TEXT_COUNT * 2,
        commands,
        screen_vertices + world_vertices + MAX_TEXT_GLYPHS * 6,
        2 * 1024 * 1024,
        2 * 4 * 1024 * 1024,
        commands,
    );
    Ok(RenderLimits::new(app::MAX_MARKERS, world, frame)
        .with_max_world_rectangles(0)
        .with_max_world_lines(0)
        .with_max_screen_rectangles(RECTANGLE_COUNT)
        .with_screen_scene_budget(screen)
        .with_max_screen_texts(TEXT_COUNT)
        .with_max_screen_text_bytes(MAX_TEXT_BYTES)
        .with_max_screen_text_glyphs(MAX_TEXT_GLYPHS))
}
