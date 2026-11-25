//! Native text-entry probe. Document contents are never printed.
use sim_logic::{input::EditorEvent, prelude::*};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}
#[derive(Resource)]
struct Editor {
    note: String,
    preedit: String,
    preedit_updates: usize,
    cancellations: usize,
    focus_losses: usize,
    focus_losses_during_preedit: usize,
    preedit_deletions: usize,
    acceptance: bool,
}
fn update(
    input: FrameText,
    mut editor: ResMut<Editor>,
    mut controls: AppResMut<WindowControls>,
    mut labels: Query<&mut ScreenTextVisual>,
    mut commands: Commands,
) -> LogicResult {
    for event in input.ordered() {
        match event {
            EditorEvent::Physical(InputEvent::FocusLost) => {
                editor.focus_losses += 1;
                if !editor.preedit.is_empty() {
                    editor.focus_losses_during_preedit += 1;
                }
            }
            EditorEvent::Physical(InputEvent::Key {
                key: PhysicalKeyCode::F8,
                state: ButtonState::Pressed,
            }) => {
                // Explicit known fixture data, not an implicit OS clipboard read.
                controls.paste_text(" paste".into())?;
            }
            EditorEvent::Physical(InputEvent::Key {
                key: PhysicalKeyCode::F12,
                state: ButtonState::Pressed,
            }) => {
                if editor.acceptance {
                    if editor.note != "aбшc paste"
                        || !editor.preedit.is_empty()
                        || editor.preedit_updates < 2
                        || editor.focus_losses == 0
                        || editor.focus_losses_during_preedit == 0
                        || editor.preedit_deletions == 0
                        || editor.cancellations == 0
                    {
                        return Err(format!("native IME acceptance failed: bytes={}, preedits={}, cancellations={}, focus_losses={}", editor.note.len(), editor.preedit_updates, editor.cancellations, editor.focus_losses).into());
                    }
                    println!(
                        "NATIVE_IME_PASS preedit/edit/commit/cancel/focus/paste; no duplicate commits"
                    );
                }
                commands.request_exit()?;
            }
            EditorEvent::Text(TextEvent::Commit(text) | TextEvent::Paste(text)) => {
                if editor.note.len() + text.len() <= 4096 {
                    editor.note.push_str(text);
                }
                editor.preedit.clear();
            }
            EditorEvent::Text(TextEvent::Preedit { text, .. }) => {
                if !text.is_empty() && text.len() < editor.preedit.len() {
                    editor.preedit_deletions += 1;
                }
                editor.preedit.clone_from(text);
                if !text.is_empty() {
                    editor.preedit_updates += 1;
                }
                if editor.acceptance {
                    println!("IME_PREEDIT bytes={}", text.len());
                }
            }
            EditorEvent::Text(TextEvent::Cancel) => {
                editor.preedit.clear();
                editor.cancellations += 1;
            }
            EditorEvent::Text(TextEvent::Edit {
                command: EditCommand::Backspace,
                ..
            }) => {
                // This probe appends/deletes scalar values; it is not a grapheme editor.
                editor.note.pop();
            }
            _ => {}
        }
    }
    for mut label in &mut labels {
        label.set_text(&format!("{} | {}", editor.note, editor.preedit))?;
    }
    Ok(())
}
fn main() -> LogicResult {
    let acceptance = std::env::args().any(|arg| arg == "--acceptance");
    let layered = std::env::args().any(|arg| arg == "--layered");
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::default()
            .with_frame_limits(FrameLimits::new(
                32,
                64,
                50_000,
                8 * 1024 * 1024,
                8 * 1024 * 1024,
                64,
            ))
            .with_max_screen_texts(1)
            .with_max_screen_text_bytes(32 * 1024)
            .with_max_screen_text_glyphs(8192),
    );
    if layered {
        config.set_three_d_render_limits(
            ThreeDRenderLimits::new(1, 12, 2048 * 2048).with_layered_limits(
                LayeredRenderLimits3d::new(12, 0, 24, 128 * 1024 * 1024, 256 * 1024 * 1024),
            ),
        );
    }
    let mut app = Application::<Action>::new(config)?;
    let mut controls = WindowControls::default();
    controls.set_text_input(true);
    app.register_app_resource(controls)?;
    let font = app.register_font(
        include_bytes!("../../tests/assets/text/DejaVuSans.ttf").to_vec(),
        TextSettings::new(26.0)?,
    )?;
    let label = ScreenTextVisual::new(font, " | ", LogicalScreenPosition::new(24.0, 80.0))?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let mut view = View3d::new(Vec3::new(0.0, 1.0, 4.0)?, Vec3::ZERO)?;
    let settings = LayerSettings3d::default().with_surface_policy(ThreeDSurfacePolicy::Native);
    view.set_layered(Some(LayeredView3d::new(settings, settings)));
    let cuboid = CuboidVisual3d::new(
        Vec3::ZERO,
        Vec3::new(1.0, 1.0, 1.0)?,
        Color::rgb(0.1, 0.3, 0.5),
    )?;
    let world = app.register_world("native-text-entry", move |world| {
        world.spawn(camera)?;
        if layered {
            world.insert_resource(view)?;
            world.spawn(cuboid)?;
        }
        world.spawn(label.clone())?;
        world.insert_resource(Editor {
            note: String::new(),
            preedit: String::new(),
            preedit_updates: 0,
            cancellations: 0,
            focus_losses: 0,
            focus_losses_during_preedit: 0,
            preedit_deletions: 0,
            acceptance,
        })?;
        Ok(())
    })?;
    app.add_fallible_system(Stage::FrameUpdate, update);
    println!("Text entry: type using your IME; F8 inserts fixture paste; F12 exits.");
    app.run_desktop(
        world,
        DesktopConfig::new("Sim Logic Native IME", 800.0, 240.0)?,
    )?;
    Ok(())
}
