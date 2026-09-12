use super::support::*;
use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};
use std::time::Duration;

#[test]
fn foreign_font_rejects_active_sources_but_not_disabled_sources() -> LogicResult {
    let foreign = font(&mut application(limits(2, 128, 64))?)?;
    for disabled in [false, true] {
        let mut app = application(limits(2, 128, 64))?;
        let local = font(&mut app)?;
        assert_ne!(local, foreign);
        let visual = label(foreign.clone(), "Foreign", 0.0)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let initial = app.register_world("foreign-font", move |world| {
            world.spawn(camera)?;
            if disabled {
                world.spawn((visual.clone(), Disabled))?;
            } else {
                world.spawn(visual.clone())?;
            }
            Ok(())
        })?;
        let result = app.build_headless(initial);
        if disabled {
            assert!(snapshot(&result?)?.resolved_screen_texts().is_empty());
        } else {
            assert!(matches!(result, Err(RunnerBuildError::InitialWorld(
                CandidateFailure::Extraction(ExtractionError::UnregisteredTextFont { font, .. })
            )) if font == foreign));
        }
    }
    Ok(())
}

#[test]
fn shared_preparation_survives_frames_and_frame_updates_use_current_values() -> LogicResult {
    let mut app = application(limits(2, 128, 64))?;
    let font = font(&mut app)?;
    let visual = label(font.clone(), "Привет", -30.0)?;
    let shared = visual.clone();
    let text_pointer = visual.text().as_ptr();
    let metrics = visual.metrics();
    app.add_fallible_frame_system(|mut labels: Query<&mut ScreenTextVisual>| -> LogicResult {
        for mut label in &mut labels {
            let p = label.position().to_vec2();
            label.set_position(LogicalScreenPosition::new(p.x() + 10.0, p.y()))?;
            label.set_tint(Color::rgba(0.5, 0.25, 1.0, 0.75))?;
            label.set_alignment(TextAlignment::Center)?;
        }
        Ok(())
    });
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("shared-preparation", move |world| {
        world.spawn(camera)?;
        world.spawn(visual.clone())?;
        world.spawn(shared.clone())?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    assert_eq!(runner.components::<Transform2d>().count(), 0);
    for frame in 0..5 {
        let current = snapshot(&runner)?;
        assert_eq!(current.resolved_screen_texts().len(), 2);
        for row in current.resolved_screen_texts() {
            assert_eq!(row.text().as_ptr(), text_pointer);
            assert_eq!(row.font(), &font);
            assert_eq!(row.metrics(), metrics);
            assert_eq!(row.position().to_vec2().x(), -30.0 + 10.0 * frame as f32);
            if frame > 0 {
                assert_eq!(
                    row.baseline_origin().to_vec2().x(),
                    row.position().to_vec2().x() - metrics.advance() * 0.5
                );
                assert_eq!(row.tint(), Color::rgba(0.5, 0.25, 1.0, 0.75));
            }
        }
        assert!(
            advance(&mut runner, Duration::ZERO, &[])?
                .failure()
                .is_none()
        );
    }
    Ok(())
}

#[derive(Resource)]
struct Target(LogicEntity);

#[test]
fn disable_and_replacement_commands_update_text_at_the_normal_frame_barrier() -> LogicResult {
    let mut app = application(limits(1, 128, 64))?;
    let font = font(&mut app)?;
    let original = label(font.clone(), "Original", 0.0)?;
    let replacement = label(font, "Replacement", 80.0)?;
    app.bind_key(PhysicalKeyCode::Space, TestAction::Modify)?;
    app.add_fallible_frame_system(
        move |input: FrameInput<TestAction>,
              target: Res<Target>,
              mut commands: Commands|
              -> LogicResult {
            if input.has_press_occurrence(TestAction::Modify) {
                commands.disable(target.0)?;
            }
            if input.has_release_occurrence(TestAction::Modify) {
                commands.enable(target.0)?;
                commands.insert(target.0, replacement.clone())?;
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("text-commands", move |world| {
        world.spawn(camera)?;
        let target = world.spawn(original.clone())?;
        world.insert_resource(Target(target))?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let source = snapshot(&runner)?.resolved_screen_texts()[0].source();
    assert!(
        advance(&mut runner, Duration::ZERO, &[edge(ButtonState::Pressed)])?
            .failure()
            .is_none()
    );
    assert!(snapshot(&runner)?.resolved_screen_texts().is_empty());
    assert!(snapshot(&runner)?.screen_draws().is_empty());
    assert!(
        advance(&mut runner, Duration::ZERO, &[edge(ButtonState::Released)])?
            .failure()
            .is_none()
    );
    let row = &snapshot(&runner)?.resolved_screen_texts()[0];
    assert_eq!(row.source(), source);
    assert_eq!(row.text(), "Replacement");
    assert_eq!(row.position().to_vec2().x(), 80.0);
    Ok(())
}

#[test]
fn font_registry_survives_world_replacement_and_foreign_candidates_stay_inactive() -> LogicResult {
    for valid in [false, true] {
        let mut app = application(limits(1, 128, 64))?;
        let local = font(&mut app)?;
        let foreign = font(&mut application(limits(1, 128, 64))?)?;
        app.bind_key(PhysicalKeyCode::Enter, TestAction::Replace)?;
        app.add_world_replacement_on_press_system();
        let replacement = label(
            if valid { local.clone() } else { foreign },
            "New World",
            80.0,
        )?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let target = app.register_world("target", move |world| {
            world.spawn(camera)?;
            world.spawn(replacement.clone())?;
            Ok(())
        })?;
        let original = label(local.clone(), "Old World", 0.0)?;
        let initial = app.register_world("initial", move |world| {
            world.spawn(camera)?;
            world.spawn(original.clone())?;
            world.insert_resource(WorldReplacementOnPress::new(TestAction::Replace, target))?;
            Ok(())
        })?;
        let mut runner = app.build_headless(initial)?;
        let generation = runner.world_generation();
        let before = snapshot(&runner)?.resolved_screen_texts().to_vec();
        advance(
            &mut runner,
            Duration::ZERO,
            &[InputEvent::key(
                PhysicalKeyCode::Enter,
                ButtonState::Pressed,
            )],
        )?;
        let report = advance(&mut runner, STEP * 3, &[])?;
        assert!(report.failure().is_none());
        let row = &snapshot(&runner)?.resolved_screen_texts()[0];
        assert_eq!(row.font(), &local);
        if valid {
            assert!(
                matches!(report.transition(), FrameTransition::Committed { target: actual, .. } if *actual == target)
            );
            assert_ne!(runner.world_generation(), generation);
            assert_eq!(row.text(), "New World");
            assert_eq!(row.source().world_generation(), runner.world_generation());
        } else {
            assert!(matches!(
                report.transition(),
                FrameTransition::PreparationFailed { .. }
            ));
            assert_eq!(runner.world_generation(), generation);
            assert_eq!(snapshot(&runner)?.resolved_screen_texts(), before);
        }
    }
    Ok(())
}

#[test]
fn live_foreign_font_preserves_last_good_snapshot_and_reports_the_source() -> LogicResult {
    let mut app = application(limits(1, 128, 64))?;
    let local = font(&mut app)?;
    let foreign = font(&mut application(limits(1, 128, 64))?)?;
    let bad_font = foreign.clone();
    app.bind_key(PhysicalKeyCode::Space, TestAction::Modify)?;
    app.add_fallible_frame_system(
        move |input: FrameInput<TestAction>,
              mut labels: Query<&mut ScreenTextVisual>|
              -> LogicResult {
            for mut label in &mut labels {
                if input.has_press_occurrence(TestAction::Modify) {
                    label.set_font(bad_font.clone())?;
                }
            }
            Ok(())
        },
    );
    let visual = label(local, "Local", 0.0)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("live-foreign", move |world| {
        world.spawn(camera)?;
        world.spawn(visual.clone())?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let before = snapshot(&runner)?.resolved_screen_texts().to_vec();
    let report = advance(&mut runner, Duration::ZERO, &[edge(ButtonState::Pressed)])?;
    assert!(matches!(report.failure(), Some(FrameFailure::Extraction(
        ExtractionError::UnregisteredTextFont { entity, font }
    )) if *entity == before[0].source() && *font == foreign));
    assert_eq!(snapshot(&runner)?.resolved_screen_texts(), before);
    Ok(())
}
