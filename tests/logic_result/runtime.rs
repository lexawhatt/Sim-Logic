use std::{fmt, rc::Rc, time::Duration};

use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Debug)]
struct LocalOnlyError(Rc<()>);

impl fmt::Display for LocalOnlyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "local-only-error({})", Rc::strong_count(&self.0))
    }
}

impl std::error::Error for LocalOnlyError {}

fn root_generic_result() -> sim_logic::LogicResult<u32> {
    Ok(7)
}

fn prelude_generic_result() -> LogicResult<u32> {
    Ok(11)
}

fn local_only_result() -> LogicResult {
    Err(LocalOnlyError(Rc::new(())).into())
}

fn build_unrelated_values() -> LogicResult<(TimeConfig, Transform2d)> {
    let time = TimeConfig::new(Duration::from_millis(10), 2)?;
    let transform = Transform2d::from_xy(3.0, -4.0)?;
    Ok((time, transform))
}

fn fail_with_erased_application_error() -> LogicResult {
    Err(WorldBuildError::user("logic-result-system-failure").into())
}

fn viewport() -> LogicResult<LogicalViewport> {
    Ok(LogicalViewport::new(800.0, 600.0)?)
}

#[test]
fn root_prelude_default_and_generic_aliases_compose_typed_errors() -> LogicResult {
    assert_eq!(root_generic_result()?, 7);
    assert_eq!(prelude_generic_result()?, 11);
    assert_eq!(
        local_only_result()
            .expect_err("local-only result should retain its error")
            .to_string(),
        "local-only-error(1)"
    );

    let (time, transform) = build_unrelated_values()?;
    assert_eq!(time.fixed_step(), Duration::from_millis(10));
    assert_eq!(time.max_catch_up_ticks(), 2);
    assert_eq!(transform.translation(), Vec2::new(3.0, -4.0));
    Ok(())
}

#[test]
fn logic_result_system_keeps_the_existing_failure_diagnostic() -> LogicResult {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.add_fallible_fixed_system(fail_with_erased_application_error);
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("logic-result", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded logic-result frame was rejected".into());
    };
    let Some(FrameFailure::System { stage, error }) = report.failure() else {
        return Err("fallible LogicResult System should fail its fixed stage".into());
    };
    assert_eq!(*stage, Stage::FixedUpdate);
    assert_eq!(
        error.reason(),
        "returned an error: logic-result-system-failure"
    );
    Ok(())
}
