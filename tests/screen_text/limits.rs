use super::support::*;
use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};
use std::time::Duration;

#[test]
fn text_caps_are_opt_in_and_builders_preserve_other_limits() {
    let initial = RenderLimits::default();
    assert_eq!(initial.max_screen_texts(), 0);
    assert_eq!(initial.max_screen_text_bytes(), 0);
    assert_eq!(initial.max_screen_text_glyphs(), 0);
    let changed = initial
        .with_max_screen_texts(3)
        .with_max_screen_text_bytes(7)
        .with_max_screen_text_glyphs(5);
    assert_eq!(changed.max_screen_texts(), 3);
    assert_eq!(changed.max_screen_text_bytes(), 7);
    assert_eq!(changed.max_screen_text_glyphs(), 5);
    assert_eq!(changed.world_scene_budget(), initial.world_scene_budget());
    assert_eq!(changed.screen_scene_budget(), initial.screen_scene_budget());
    assert_eq!(changed.frame_limits(), initial.frame_limits());
    assert_eq!(changed.max_screen_images(), initial.max_screen_images());
    assert_eq!(changed.three_d(), initial.three_d());
}

#[test]
fn count_utf8_and_shaped_glyph_caps_have_independent_exact_boundaries() -> LogicResult {
    for (count, bytes, glyphs, failure) in [
        (2, 4, 3, None),
        (1, 4, 3, Some("count")),
        (2, 3, 3, Some("bytes")),
        (2, 4, 2, Some("glyphs")),
    ] {
        let mut app = application(limits(count, bytes, glyphs))?;
        let font = font(&mut app)?;
        let first = label(font.clone(), "AB", 0.0)?;
        let second = label(font, "Я", 80.0)?;
        assert_eq!(first.glyph_count() + second.glyph_count(), 3);
        let camera = ActiveCamera2d::centered(1.0)?;
        let initial = app.register_world("exact-text-caps", move |world| {
            world.spawn(camera)?;
            world.spawn(first.clone())?;
            world.spawn(second.clone())?;
            Ok(())
        })?;
        let result = app.build_headless(initial);
        match failure {
            None => assert_eq!(snapshot(&result?)?.resolved_screen_texts().len(), 2),
            Some("count") => assert!(matches!(
                result,
                Err(RunnerBuildError::InitialWorld(
                    CandidateFailure::Extraction(ExtractionError::ScreenTextLimitExceeded {
                        limit: 1
                    })
                ))
            )),
            Some("bytes") => assert!(matches!(
                result,
                Err(RunnerBuildError::InitialWorld(
                    CandidateFailure::Extraction(ExtractionError::ScreenTextBytesLimitExceeded {
                        limit: 3,
                        requested: 4
                    })
                ))
            )),
            Some("glyphs") => assert!(matches!(
                result,
                Err(RunnerBuildError::InitialWorld(
                    CandidateFailure::Extraction(ExtractionError::ScreenTextGlyphLimitExceeded {
                        limit: 2,
                        requested: 3
                    })
                ))
            )),
            _ => unreachable!(),
        }
    }
    Ok(())
}

#[test]
fn empty_text_counts_as_a_source_but_disabled_text_consumes_no_allowance() -> LogicResult {
    for (disabled, count, accepted) in [(false, 0, false), (true, 0, true), (false, 1, true)] {
        let mut app = application(limits(count, 0, 0))?;
        let font = font(&mut app)?;
        let visual = label(font, if disabled { "ignored" } else { "" }, 0.0)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let initial = app.register_world("empty-text", move |world| {
            world.spawn(camera)?;
            if disabled {
                world.spawn((visual.clone(), Disabled))?;
            } else {
                world.spawn(visual.clone())?;
            }
            Ok(())
        })?;
        let result = app.build_headless(initial);
        if accepted {
            assert_eq!(
                snapshot(&result?)?.resolved_screen_texts().len(),
                usize::from(!disabled)
            );
        } else {
            assert!(matches!(
                result,
                Err(RunnerBuildError::InitialWorld(
                    CandidateFailure::Extraction(ExtractionError::ScreenTextLimitExceeded {
                        limit: 0
                    })
                ))
            ));
        }
    }
    Ok(())
}

#[test]
fn shared_text_is_counted_per_placement_and_spaces_consume_glyph_allowance() -> LogicResult {
    let mut app = application(limits(2, 4, 1))?;
    let font = font(&mut app)?;
    let visual = label(font, " ", 0.0)?;
    assert_eq!(visual.glyph_count(), 1);
    let clone = visual.clone();
    assert_eq!(clone.text().as_ptr(), visual.text().as_ptr());
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("shared-text-cap", move |world| {
        world.spawn(camera)?;
        world.spawn(visual.clone())?;
        world.spawn(clone.clone())?;
        Ok(())
    })?;
    assert!(matches!(
        app.build_headless(initial),
        Err(RunnerBuildError::InitialWorld(
            CandidateFailure::Extraction(ExtractionError::ScreenTextGlyphLimitExceeded {
                limit: 1,
                requested: 2
            })
        ))
    ));
    Ok(())
}

#[test]
fn changed_text_over_limit_keeps_complete_snapshot_and_can_be_repaired() -> LogicResult {
    let mut app = application(limits(1, 3, 32))?;
    let font = font(&mut app)?;
    app.bind_key(PhysicalKeyCode::Space, TestAction::Modify)?;
    app.add_fallible_frame_system(
        |input: FrameInput<TestAction>, mut labels: Query<&mut ScreenTextVisual>| -> LogicResult {
            for mut label in &mut labels {
                if input.has_press_occurrence(TestAction::Modify) {
                    label.set_text("too long")?;
                    label.set_position(LogicalScreenPosition::new(90.0, 40.0))?;
                }
                if input.has_release_occurrence(TestAction::Modify) {
                    label.set_text("ok")?;
                }
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let visual = label(font, "old", 0.0)?;
    let initial = app.register_world("repair-text-limit", move |world| {
        world.spawn(camera)?;
        world.spawn(visual.clone())?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let before = snapshot(&runner)?.resolved_screen_texts().to_vec();
    let before_draws = snapshot(&runner)?.screen_draws().to_vec();
    let failed = advance(&mut runner, Duration::ZERO, &[edge(ButtonState::Pressed)])?;
    assert!(matches!(
        failed.failure(),
        Some(FrameFailure::Extraction(
            ExtractionError::ScreenTextBytesLimitExceeded {
                limit: 3,
                requested: 8
            }
        ))
    ));
    assert_eq!(snapshot(&runner)?.resolved_screen_texts(), before);
    assert_eq!(snapshot(&runner)?.screen_draws(), before_draws);
    let repaired = advance(&mut runner, Duration::ZERO, &[edge(ButtonState::Released)])?;
    assert!(repaired.failure().is_none());
    let current = &snapshot(&runner)?.resolved_screen_texts()[0];
    assert_eq!(current.text(), "ok");
    assert_eq!(current.position(), LogicalScreenPosition::new(90.0, 40.0));
    assert_eq!(before[0].text(), "old");
    Ok(())
}
