use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Debug, Clone, Copy)]
struct RegistrationEvent(u8);

#[derive(Resource, Default)]
struct Trace(Vec<&'static str>);

#[derive(Default)]
struct GlobalRuns(usize);

fn fixed_alias(
    mut trace: ResMut<Trace>,
    mut events: EventWriter<RegistrationEvent>,
    mut global: AppResMut<GlobalRuns>,
) {
    trace.0.push("fixed-alias");
    global.0 += 1;
    assert!(events.send(RegistrationEvent(7)).is_ok());
}

fn fixed_generic(events: EventReader<RegistrationEvent>, mut trace: ResMut<Trace>) {
    assert_eq!(events.iter().map(|event| event.0).collect::<Vec<_>>(), [7]);
    trace.0.push("fixed-generic");
}

fn fixed_fallible(mut trace: ResMut<Trace>) -> Result<(), &'static str> {
    trace.0.push("fixed-fallible-alias");
    Ok(())
}

fn frame_generic(mut trace: ResMut<Trace>) {
    trace.0.push("frame-generic");
}

fn frame_alias(mut trace: ResMut<Trace>, mut global: AppResMut<GlobalRuns>) {
    trace.0.push("frame-alias");
    global.0 += 1;
}

fn frame_fallible(mut trace: ResMut<Trace>) -> Result<(), &'static str> {
    trace.0.push("frame-fallible-alias");
    Ok(())
}

fn viewport() -> Result<LogicalViewport, Box<dyn Error>> {
    Ok(LogicalViewport::new(800.0, 600.0)?)
}

fn register_camera_world(
    application: &mut Application<TestAction>,
    name: &'static str,
) -> Result<WorldFactoryId, Box<dyn Error>> {
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    Ok(application.register_world(name, move |world| {
        world.spawn(camera)?;
        world.insert_resource(Trace::default())?;
        Ok(())
    })?)
}

#[test]
fn short_and_generic_registration_share_order_and_discovery() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.register_app_resource(GlobalRuns::default())?;
    application.add_fixed_system(fixed_alias);
    application.add_system(Stage::FixedUpdate, fixed_generic);
    application.add_fallible_fixed_system(fixed_fallible);
    application.add_system(Stage::FrameUpdate, frame_generic);
    application.add_frame_system(frame_alias);
    application.add_fallible_frame_system(frame_fallible);
    let world = register_camera_world(&mut application, "registration-order")?;

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded registration test frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert_eq!(
        runner.resource::<Trace>().ok_or("trace disappeared")?.0,
        [
            "fixed-alias",
            "fixed-generic",
            "fixed-fallible-alias",
            "frame-generic",
            "frame-alias",
            "frame-fallible-alias",
        ]
    );
    assert_eq!(
        runner
            .app_resource::<GlobalRuns>()
            .ok_or("Application Resource disappeared")?
            .0,
        2
    );
    Ok(())
}

#[test]
fn fallible_short_forms_report_their_exact_stage() -> Result<(), Box<dyn Error>> {
    let mut fixed_application = Application::<TestAction>::new(AppConfig::default())?;
    fixed_application
        .add_fallible_fixed_system(|| -> Result<(), &'static str> { Err("fixed alias failure") });
    let fixed_world = register_camera_world(&mut fixed_application, "fixed-failure")?;
    let mut fixed_runner = fixed_application.build_headless(fixed_world)?;
    let fixed = fixed_runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    assert!(matches!(
        fixed,
        FrameOutcome::Advanced(ref report)
            if matches!(
                report.failure(),
                Some(FrameFailure::System { stage: Stage::FixedUpdate, .. })
            )
    ));

    let mut frame_application = Application::<TestAction>::new(AppConfig::default())?;
    frame_application
        .add_fallible_frame_system(|| -> Result<(), &'static str> { Err("frame alias failure") });
    let frame_world = register_camera_world(&mut frame_application, "frame-failure")?;
    let mut frame_runner = frame_application.build_headless(frame_world)?;
    let frame = frame_runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        frame,
        FrameOutcome::Advanced(ref report)
            if matches!(
                report.failure(),
                Some(FrameFailure::System { stage: Stage::FrameUpdate, .. })
            )
    ));
    Ok(())
}
