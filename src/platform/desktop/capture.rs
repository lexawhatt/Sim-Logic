//! Focus-gated cursor capture with best-effort release on every exit path.

use crate::input::{PointerCapture, PointerCaptureStatus};
use std::sync::Arc;
use winit::window::{CursorGrabMode, Window};

pub(super) struct DesktopCapture {
    window: Option<Arc<Window>>,
    focused: bool,
    active: bool,
}

impl DesktopCapture {
    pub(super) const fn new() -> Self {
        Self {
            window: None,
            focused: false,
            active: false,
        }
    }
    pub(super) fn attach(&mut self, window: Arc<Window>) {
        self.release();
        self.focused = window.has_focus();
        self.window = Some(window);
    }
    pub(super) fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
    pub(super) const fn accepts_motion(&self) -> bool {
        self.focused && self.active
    }
    pub(super) fn synchronize(&mut self, request: &mut PointerCapture) {
        if !self.focused && request.requested() {
            request.release();
        }
        if !request.pending() {
            return;
        }
        let status = match self.window.as_deref() {
            Some(window) => apply_backend(window, request.requested() && self.focused),
            None => PointerCaptureStatus::Unavailable,
        };
        self.active = matches!(
            status,
            PointerCaptureStatus::Locked | PointerCaptureStatus::Confined
        );
        request.acknowledge(status);
    }
    pub(super) fn release(&mut self) -> PointerCaptureStatus {
        self.active = false;
        self.window
            .as_deref()
            .map_or(PointerCaptureStatus::Released, |window| {
                apply_backend(window, false)
            })
    }
}

impl Drop for DesktopCapture {
    fn drop(&mut self) {
        self.release();
    }
}

trait CursorBackend {
    fn grab(&self, mode: CursorGrabMode) -> bool;
    fn visible(&self, visible: bool);
}
impl CursorBackend for Window {
    fn grab(&self, mode: CursorGrabMode) -> bool {
        self.set_cursor_grab(mode).is_ok()
    }
    fn visible(&self, visible: bool) {
        self.set_cursor_visible(visible);
    }
}

fn apply_backend(backend: &impl CursorBackend, requested: bool) -> PointerCaptureStatus {
    let status = if !requested {
        if backend.grab(CursorGrabMode::None) {
            PointerCaptureStatus::Released
        } else {
            PointerCaptureStatus::Unavailable
        }
    } else if backend.grab(CursorGrabMode::Locked) {
        PointerCaptureStatus::Locked
    } else if backend.grab(CursorGrabMode::Confined) {
        PointerCaptureStatus::Confined
    } else {
        // A failed acquire must never leave an intentionally hidden cursor.
        let _ = backend.grab(CursorGrabMode::None);
        PointerCaptureStatus::Unavailable
    };
    backend.visible(!matches!(
        status,
        PointerCaptureStatus::Locked | PointerCaptureStatus::Confined
    ));
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Backend {
        locked: bool,
        confined: bool,
        release: bool,
        calls: RefCell<Vec<String>>,
    }
    impl CursorBackend for Backend {
        fn grab(&self, mode: CursorGrabMode) -> bool {
            self.calls.borrow_mut().push(format!("{mode:?}"));
            match mode {
                CursorGrabMode::None => self.release,
                CursorGrabMode::Locked => self.locked,
                CursorGrabMode::Confined => self.confined,
            }
        }
        fn visible(&self, visible: bool) {
            self.calls.borrow_mut().push(format!("visible:{visible}"));
        }
    }
    fn backend(locked: bool, confined: bool) -> Backend {
        Backend {
            locked,
            confined,
            release: true,
            calls: RefCell::default(),
        }
    }
    #[test]
    fn lock_precedes_confine_and_only_success_hides_cursor() {
        let locked = backend(true, true);
        assert_eq!(apply_backend(&locked, true), PointerCaptureStatus::Locked);
        assert_eq!(*locked.calls.borrow(), ["Locked", "visible:false"]);
        let confined = backend(false, true);
        assert_eq!(
            apply_backend(&confined, true),
            PointerCaptureStatus::Confined
        );
        assert_eq!(
            *confined.calls.borrow(),
            ["Locked", "Confined", "visible:false"]
        );
        let rejected = backend(false, false);
        assert_eq!(
            apply_backend(&rejected, true),
            PointerCaptureStatus::Unavailable
        );
        assert_eq!(
            *rejected.calls.borrow(),
            ["Locked", "Confined", "None", "visible:true"]
        );
    }
    #[test]
    fn even_failed_release_restores_visibility() {
        let mut backend = backend(true, true);
        backend.release = false;
        assert_eq!(
            apply_backend(&backend, false),
            PointerCaptureStatus::Unavailable
        );
        assert_eq!(*backend.calls.borrow(), ["None", "visible:true"]);
    }

    #[test]
    fn acknowledged_capture_and_menu_release_do_not_repeat_backend_work() {
        let mut request = PointerCapture::default();
        request.request_lock();
        assert!(request.pending());
        request.acknowledge(PointerCaptureStatus::Locked);
        request.request_lock();
        assert!(!request.pending());
        request.release();
        assert!(request.pending());
        request.acknowledge(PointerCaptureStatus::Released);
        request.release();
        assert!(!request.pending());
        request.request_lock();
        request.acknowledge(PointerCaptureStatus::Unavailable);
        request.request_lock();
        assert!(
            request.pending(),
            "an explicit new gesture may retry failure"
        );
    }
    #[test]
    fn lost_focus_disables_raw_motion_and_never_reacquires_on_regain() {
        let mut capture = DesktopCapture::new();
        capture.focused = true;
        capture.active = true;
        assert!(capture.accepts_motion());
        capture.set_focused(false);
        capture.release();
        assert!(!capture.accepts_motion());
        capture.set_focused(true);
        assert!(!capture.accepts_motion());
        let mut intent = PointerCapture::default();
        intent.request_lock();
        capture.set_focused(false);
        capture.synchronize(&mut intent);
        assert!(!intent.requested());
        capture.set_focused(true);
        capture.synchronize(&mut intent);
        assert!(!capture.accepts_motion());
        assert!(!intent.pending());
    }
}
