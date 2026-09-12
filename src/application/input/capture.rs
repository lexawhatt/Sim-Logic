//! Explicit application-owned pointer capture intent and backend acknowledgement.

/// Last known outcome of a desktop pointer-capture operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointerCaptureStatus {
    /// No capture has been established; the desktop cursor is visible.
    #[default]
    Released,
    /// The backend locked the cursor and hides it while focused.
    Locked,
    /// Lock was unavailable; confinement succeeded and the cursor is hidden.
    Confined,
    /// The last backend request failed. Cursor visibility is restored and raw
    /// motion is disabled; after release failure physical grab state is unknown.
    Unavailable,
}

/// Optional application-owned pointer capture service, independent of winit.
///
/// Register once with `Application::register_app_resource(Self::default())`.
/// Frame systems use `AppResMut<PointerCapture>` to request lock on an explicit
/// user gesture and release it for menus. Desktop applies requests after the
/// logical frame; requesting capture does not promise backend support.
/// Focus loss clears the request immediately. Focus regain never reissues it.
///
/// Requests are immediate application-resource mutations, not transactional
/// Commands. A headless runner preserves intent but performs no OS operation;
/// deterministic tests may inject relative motion independently of OS status.
#[derive(Debug, Default)]
pub struct PointerCapture {
    requested: bool,
    pending: bool,
    status: PointerCaptureStatus,
}

impl PointerCapture {
    /// Requests capture or explicitly retries an unavailable backend. Call on a
    /// fresh click/key gesture, not unconditionally every update. An already
    /// acknowledged capture needs no repeated backend operation.
    pub fn request_lock(&mut self) {
        if self.requested && self.is_captured() {
            return;
        }
        self.requested = true;
        self.pending = true;
    }
    /// Requests cursor release and visibility, normally when opening a menu.
    /// Repeating an acknowledged release performs no new backend operation.
    pub fn release(&mut self) {
        if !self.requested && self.status == PointerCaptureStatus::Released {
            return;
        }
        self.requested = false;
        self.pending = true;
    }
    /// Returns current application intent, not actual OS capture availability.
    pub const fn requested(&self) -> bool {
        self.requested
    }
    /// Returns the latest acknowledged backend state.
    pub const fn status(&self) -> PointerCaptureStatus {
        self.status
    }
    /// Reports successful locked/confined capture, not merely a request.
    pub const fn is_captured(&self) -> bool {
        matches!(
            self.status,
            PointerCaptureStatus::Locked | PointerCaptureStatus::Confined
        )
    }
    #[cfg(feature = "desktop")]
    pub(crate) const fn pending(&self) -> bool {
        self.pending
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn acknowledge(&mut self, status: PointerCaptureStatus) {
        self.status = status;
        self.pending = false;
    }
}
