//! Native window commands; the logical API never exposes a winit handle.

use crate::window::{
    CursorShape, FullscreenMonitor, WindowControls, WindowMode, WindowModeFailure, WindowModeStatus,
};
use winit::window::{CursorIcon, Fullscreen, Window};

pub(super) fn apply_mode(window: &Window, mode: WindowMode) -> WindowModeStatus {
    let fullscreen = match mode {
        WindowMode::Windowed => None,
        WindowMode::BorderlessFullscreen(selection) => {
            let monitor = match select_monitor(
                selection,
                || window.current_monitor(),
                || window.primary_monitor(),
            ) {
                Ok(monitor) => monitor,
                Err(error) => return WindowModeStatus::Unavailable(error),
            };
            Some(Fullscreen::Borderless(monitor))
        }
    };
    window.set_fullscreen(fullscreen);
    WindowModeStatus::Submitted(mode)
}

fn select_monitor<M>(
    selection: FullscreenMonitor,
    current: impl FnOnce() -> Option<M>,
    primary: impl FnOnce() -> Option<M>,
) -> Result<Option<M>, WindowModeFailure> {
    match selection {
        FullscreenMonitor::Automatic => Ok(None),
        FullscreenMonitor::Current => current()
            .map(Some)
            .ok_or(WindowModeFailure::MonitorUnavailable),
        FullscreenMonitor::Primary => primary()
            .map(Some)
            .ok_or(WindowModeFailure::MonitorUnavailable),
    }
}

pub(super) fn synchronize(window: &Window, controls: &mut WindowControls) {
    if let Some(mode) = controls.pending_mode() {
        controls.acknowledge_mode(apply_mode(window, mode));
    }
    if let Some(shape) = controls.pending_cursor() {
        window.set_cursor(map_cursor(shape));
        controls.acknowledge_cursor(shape);
    }
}

fn map_cursor(shape: CursorShape) -> CursorIcon {
    match shape {
        CursorShape::Default => CursorIcon::Default,
        CursorShape::Pointer => CursorIcon::Pointer,
        CursorShape::Text => CursorIcon::Text,
        CursorShape::Crosshair => CursorIcon::Crosshair,
        CursorShape::Grab => CursorIcon::Grab,
        CursorShape::Grabbing => CursorIcon::Grabbing,
        CursorShape::NotAllowed => CursorIcon::NotAllowed,
        CursorShape::Wait => CursorIcon::Wait,
        CursorShape::Progress => CursorIcon::Progress,
        CursorShape::Move => CursorIcon::Move,
        CursorShape::ResizeHorizontal => CursorIcon::EwResize,
        CursorShape::ResizeVertical => CursorIcon::NsResize,
        CursorShape::ResizeNorthEastSouthWest => CursorIcon::NeswResize,
        CursorShape::ResizeNorthWestSouthEast => CursorIcon::NwseResize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monitor_selection_never_silently_chooses_another_monitor() {
        assert_eq!(
            select_monitor::<u8>(
                FullscreenMonitor::Automatic,
                || panic!("automatic must not enumerate"),
                || panic!("automatic must not enumerate")
            ),
            Ok(None)
        );
        assert_eq!(
            select_monitor(FullscreenMonitor::Current, || Some(2), || Some(9)),
            Ok(Some(2))
        );
        assert_eq!(
            select_monitor(FullscreenMonitor::Primary, || Some(2), || Some(9)),
            Ok(Some(9))
        );
        assert_eq!(
            select_monitor(FullscreenMonitor::Current, || None, || Some(9)),
            Err(WindowModeFailure::MonitorUnavailable)
        );
        assert_eq!(
            select_monitor(FullscreenMonitor::Primary, || Some(2), || None),
            Err(WindowModeFailure::MonitorUnavailable)
        );
    }
    #[test]
    fn acknowledgement_clears_pending_work_without_claiming_display_confirmation() {
        let mut controls = WindowControls::default();
        controls.request_mode(WindowMode::Windowed);
        controls.acknowledge_mode(WindowModeStatus::Unavailable(
            WindowModeFailure::MonitorUnavailable,
        ));
        assert_eq!(controls.pending_mode(), None);
        controls.request_mode(WindowMode::Windowed);
        assert_eq!(controls.pending_mode(), Some(WindowMode::Windowed));
        controls.acknowledge_mode(WindowModeStatus::Submitted(WindowMode::Windowed));
        controls.request_cursor(CursorShape::Pointer);
        controls.acknowledge_cursor(CursorShape::Pointer);
        controls.request_cursor(CursorShape::Pointer);
        assert_eq!(controls.pending_cursor(), None);
        controls.request_cursor(CursorShape::Text);
        controls.request_cursor(CursorShape::Pointer);
        assert_eq!(controls.pending_cursor(), None);
        assert_eq!(map_cursor(CursorShape::Pointer), CursorIcon::Pointer);
        assert_eq!(map_cursor(CursorShape::Default), CursorIcon::Default);
    }
}
