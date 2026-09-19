//! Optional window commands, independent of windowing and rendering crates.

/// How a borderless-fullscreen request selects a monitor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FullscreenMonitor {
    /// Let the window manager choose, without requiring monitor enumeration.
    #[default]
    Automatic,
    /// Require the monitor currently containing the window; no silent fallback.
    Current,
    /// Require the platform's primary monitor; no silent fallback.
    Primary,
}

/// Requested window presentation. Exclusive video-mode changes are not used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WindowMode {
    /// An ordinary resizable window, restoring windowed placement where supported.
    #[default]
    Windowed,
    /// Borderless fullscreen without changing the monitor's video mode.
    BorderlessFullscreen(FullscreenMonitor),
}

/// Native cursor appearance. PointerCapture still controls capture and visibility.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CursorShape {
    /// Platform's ordinary arrow/default cursor.
    #[default]
    Default,
    /// A clickable link or button.
    Pointer,
    /// Text selection/insertion.
    Text,
    /// Precise pointing.
    Crosshair,
    /// An object can be dragged.
    Grab,
    /// An object is being dragged.
    Grabbing,
    /// The requested action is unavailable.
    NotAllowed,
    /// Wait for an operation to finish.
    Wait,
    /// Background progress while interaction remains possible.
    Progress,
    /// Movement in all directions.
    Move,
    /// Horizontal resize.
    ResizeHorizontal,
    /// Vertical resize.
    ResizeVertical,
    /// Northeast/southwest resize.
    ResizeNorthEastSouthWest,
    /// Northwest/southeast resize.
    ResizeNorthWestSouthEast,
}

/// Why a mode could not be submitted. Rejection keeps the old mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowModeFailure {
    /// An explicitly selected current/primary monitor was not available.
    MonitorUnavailable,
    /// This adapter does not implement window mode requests.
    Unsupported,
}

/// Latest mode submission result, not a compositor-confirmed display state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WindowModeStatus {
    /// No native operation attempted (including headless operation).
    #[default]
    NotSubmitted,
    /// Sent to the window manager, which may apply asynchronously or ignore it.
    /// The native API supplies no reliable visual-completion acknowledgement.
    Submitted(WindowMode),
    /// Nothing submitted; inspect the failure and retry on an explicit action.
    Unavailable(WindowModeFailure),
}

/// Optional application-owned window/cursor requests.
///
/// Register `WindowControls::default()` as an Application Resource and mutate
/// through `AppResMut<WindowControls>`. Desktop consumes requests after the
/// logical frame; headless retains them without OS calls. Requests are immediate,
/// not transactional Commands, and survive World replacement. Last request wins
/// before consumption; there is no growing queue or automatic F11 binding.
/// Native cursor themes may substitute the default for an unavailable shape.
#[derive(Debug, Default)]
pub struct WindowControls {
    mode: Option<WindowMode>,
    cursor: Option<CursorShape>,
    mode_status: WindowModeStatus,
    submitted_cursor: Option<CursorShape>,
}

impl WindowControls {
    /// Requests a mode, or explicitly retries a previously ignored request.
    pub fn request_mode(&mut self, mode: WindowMode) {
        self.mode = Some(mode);
    }

    /// Requests a cursor; repeating an acknowledged value does no OS work.
    pub fn request_cursor(&mut self, shape: CursorShape) {
        self.cursor = (self.submitted_cursor != Some(shape)).then_some(shape);
    }

    /// Returns the not-yet-submitted mode request, if any.
    pub const fn pending_mode(&self) -> Option<WindowMode> {
        self.mode
    }
    /// Returns the native submission outcome. Headless never fabricates success.
    pub const fn mode_status(&self) -> WindowModeStatus {
        self.mode_status
    }
    /// Returns the not-yet-submitted cursor request, if any.
    pub const fn pending_cursor(&self) -> Option<CursorShape> {
        self.cursor
    }
    /// Returns the cursor last sent to the backend, not a theme-support guarantee.
    pub const fn submitted_cursor(&self) -> Option<CursorShape> {
        self.submitted_cursor
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn acknowledge_mode(&mut self, status: WindowModeStatus) {
        self.mode = None;
        self.mode_status = status;
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn acknowledge_cursor(&mut self, shape: CursorShape) {
        self.cursor = None;
        self.submitted_cursor = Some(shape);
    }
}
