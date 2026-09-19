use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Menu,
    Dialog,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Start,
    Options,
    Exit,
}

#[test]
fn focus_wraps_and_never_activates_stale_modal_or_disabled_targets() -> LogicResult {
    use Target::*;
    let mut focus = KeyboardFocus::new(3);
    let all = [Start, Options, Exit];
    assert_eq!(
        focus
            .process(Scope::Menu, &all, FocusCommand::Activate)?
            .activated,
        None
    );
    assert_eq!(
        focus
            .process(Scope::Menu, &all, FocusCommand::Previous)?
            .focused,
        Some(Exit)
    );
    assert_eq!(
        focus
            .process(Scope::Menu, &all, FocusCommand::Next)?
            .focused,
        Some(Start)
    );
    assert_eq!(
        focus
            .process(Scope::Menu, &all, FocusCommand::Activate)?
            .activated,
        Some(Start)
    );
    assert_eq!(
        focus
            .process(Scope::Menu, &[Options, Exit], FocusCommand::Activate)?
            .activated,
        None
    );
    focus.set_focused(Scope::Menu, &all, Some(Options))?;
    assert_eq!(
        focus
            .process(Scope::Dialog, &[Options], FocusCommand::Activate)?
            .activated,
        None
    );
    assert_eq!(
        focus
            .process(Scope::Dialog, &[Options], FocusCommand::First)?
            .focused,
        Some(Options)
    );
    assert_eq!(
        focus
            .process(Scope::Dialog, &[], FocusCommand::Next)?
            .focused,
        None
    );
    Ok(())
}

#[test]
fn invalid_focus_orders_preserve_scope_and_focus() -> LogicResult {
    let mut focus = KeyboardFocus::new(2);
    focus.set_focused(Scope::Menu, &[Target::Start], Some(Target::Start))?;
    for (order, error) in [
        (
            &[Target::Options, Target::Options][..],
            FocusError::DuplicateTarget,
        ),
        (
            &[Target::Options, Target::Exit, Target::Start][..],
            FocusError::TargetLimitExceeded,
        ),
    ] {
        assert_eq!(
            focus.process(Scope::Dialog, order, FocusCommand::First),
            Err(error)
        );
        assert_eq!(focus.scope(), Some(Scope::Menu));
        assert_eq!(focus.focused(), Some(Target::Start));
    }
    assert_eq!(
        focus.set_focused(Scope::Dialog, &[Target::Options], Some(Target::Exit)),
        Err(FocusError::IneligibleTarget)
    );
    assert_eq!(focus.scope(), Some(Scope::Menu));
    Ok(())
}
