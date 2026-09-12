//! One-window desktop adapter for the shared Sim;Logic frame driver.
//!
//! Run the adapter on the process main thread. The underlying platform may
//! permit only one event-loop creation for the lifetime of the process.
//! Focus loss cancels held controls through the shared input core. Synthetic
//! keyboard events and repeated presses are ignored; returning to the window
//! requires a fresh physical press before a key becomes held again.

#[path = "desktop/capture.rs"]
mod capture;
#[path = "desktop/images.rs"]
mod images;
#[path = "desktop/pointer.rs"]
mod pointer;
#[cfg(feature = "text")]
#[path = "desktop/text.rs"]
mod text;
#[path = "desktop/three_d.rs"]
mod three_d;
#[path = "desktop/timing.rs"]
mod timing;

pub use images::DesktopImageError;
pub use pointer::DesktopPointerError;
pub use sim_engine::FrameCacheBudget;
#[cfg(feature = "text")]
pub use text::DesktopTextError;
pub use three_d::{DesktopThreeDError, DesktopThreeDUpdates};
pub use timing::{DesktopGpuTimingSample, DesktopGpuTimings};

use std::{error::Error, fmt, sync::Arc, time::Instant};

use sim_engine::{
    FrameBudget, FrameComposerError, FramePassOptions, FrameReport, GpuTimingSource,
    Mesh3dRenderReport, RenderStatus, RendererConfigurationError, RendererFrameError,
    RendererInitError, RendererPresentMode, RendererSurfaceStatus, WgpuRenderer,
    WgpuRendererOptions,
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    error::{EventLoopError, OsError},
    event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

use crate::{
    app::Application,
    headless::{
        BeginFrameRejection, FrameOutcome, FrameRequest, FrameTransition, HeadlessRunner,
        LogicFrameReport, RunnerBuildError,
    },
    identity::{WorldFactoryId, WorldGeneration},
    input::{
        Action, ButtonState, InputEvent, PhysicalKeyCode, PointerCapture, RelativePointerMotion,
        RelativePointerMotionError, SUPPORTED_PHYSICAL_KEY_COUNT, physical_key_index,
    },
    render::FrameLimits,
    three_d::ThreeDRenderLimits,
};

use capture::DesktopCapture;
use images::{DesktopImages, ScreenPresentationError};
use pointer::DesktopPointerGeometry;
use three_d::DesktopThreeD;

const DEFAULT_TITLE: &str = "Sim;Logic";
const DEFAULT_WIDTH: f64 = 1_280.0;
const DEFAULT_HEIGHT: f64 = 720.0;
const MAX_TITLE_BYTES: usize = 256;

/// Window and presentation settings fixed before the desktop loop starts.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopConfig {
    title: String,
    logical_width: f64,
    logical_height: f64,
    present_mode: RendererPresentMode,
    frame_cache: FrameCacheBudget,
    gpu_timing: bool,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            title: DEFAULT_TITLE.to_owned(),
            logical_width: DEFAULT_WIDTH,
            logical_height: DEFAULT_HEIGHT,
            present_mode: RendererPresentMode::Vsync,
            frame_cache: FrameCacheBudget::default(),
            gpu_timing: false,
        }
    }
}

impl DesktopConfig {
    /// Creates validated settings for one logical-size window.
    pub fn new(
        title: impl Into<String>,
        logical_width: f64,
        logical_height: f64,
    ) -> Result<Self, DesktopConfigError> {
        let title = title.into();
        validate_title(&title)?;
        validate_logical_size(logical_width, logical_height)?;
        Ok(Self {
            title,
            logical_width,
            logical_height,
            present_mode: RendererPresentMode::Vsync,
            frame_cache: FrameCacheBudget::default(),
            gpu_timing: false,
        })
    }

    /// Selects synchronized or fastest-available presentation.
    pub fn set_present_mode(&mut self, present_mode: RendererPresentMode) -> &mut Self {
        self.present_mode = present_mode;
        self
    }

    /// Sets Engine's idle composition-cache limits independently of active frame limits.
    ///
    /// Zero cache limits disable retention, not drawing. This does not bound
    /// image/font assets, 3D scene resources, driver memory or in-flight work.
    /// Defaults to Engine's finite cache budget, including packed uniform uploads.
    pub fn set_frame_cache_budget(&mut self, budget: FrameCacheBudget) -> &mut Self {
        self.frame_cache = budget;
        self
    }

    /// Returns the separate idle composition-cache budget.
    pub const fn frame_cache_budget(&self) -> FrameCacheBudget {
        self.frame_cache
    }

    /// Requests bounded asynchronous GPU pass timestamps, disabled by default.
    ///
    /// Unsupported adapters still start and report Engine's `Unavailable` status.
    /// Collection never waits for the GPU; query/readback resources and polling
    /// add explicitly reported diagnostic overhead. CPU and presentation time
    /// are not substituted for missing measurements.
    pub fn set_gpu_timing(&mut self, enabled: bool) -> &mut Self {
        self.gpu_timing = enabled;
        self
    }

    /// Returns whether optional GPU diagnostics were requested, not availability.
    pub const fn gpu_timing(&self) -> bool {
        self.gpu_timing
    }

    /// Returns the configured title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the initial logical width.
    pub const fn logical_width(&self) -> f64 {
        self.logical_width
    }

    /// Returns the initial logical height.
    pub const fn logical_height(&self) -> f64 {
        self.logical_height
    }

    /// Returns the requested presentation behavior.
    pub const fn present_mode(&self) -> RendererPresentMode {
        self.present_mode
    }
}

/// Invalid desktop configuration.
#[derive(Debug, Clone, PartialEq)]
pub enum DesktopConfigError {
    /// The title contains no non-whitespace character.
    EmptyTitle,
    /// The UTF-8 title exceeds the finite adapter limit.
    TitleTooLong {
        /// Maximum accepted UTF-8 byte count.
        limit: usize,
    },
    /// Logical dimensions must be finite and strictly positive.
    InvalidLogicalSize {
        /// Rejected logical width.
        width: f64,
        /// Rejected logical height.
        height: f64,
    },
}

impl fmt::Display for DesktopConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTitle => formatter.write_str("desktop window title must not be empty"),
            Self::TitleTooLong { limit } => {
                write!(
                    formatter,
                    "desktop window title exceeds {limit} UTF-8 bytes"
                )
            }
            Self::InvalidLogicalSize { width, height } => write!(
                formatter,
                "desktop logical size must be finite and positive, got {width}x{height}"
            ),
        }
    }
}

impl Error for DesktopConfigError {}

/// Successful reason the desktop event loop ended.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DesktopExitReason {
    /// The platform window was closed or destroyed.
    #[default]
    WindowClosed,
    /// A committed [`crate::commands::LogicCommands::request_exit`] command
    /// ended the loop after its logical frame.
    ApplicationRequested,
}

/// Frame counts and final detailed reports retained after a successful exit.
#[derive(Debug, Default)]
pub struct DesktopRunReport {
    logic_frames: u64,
    drawn_frames: u64,
    skipped_frames: u64,
    committed_transitions: u64,
    device_recoveries: u64,
    exit_reason: DesktopExitReason,
    last_logic_frame: Option<LogicFrameReport>,
    last_render_frame: Option<FrameReport>,
    last_three_d_frame: Option<Mesh3dRenderReport>,
    last_three_d_updates: Option<DesktopThreeDUpdates>,
    three_d_updates: DesktopThreeDUpdates,
    gpu_timings: DesktopGpuTimings,
}

impl DesktopRunReport {
    /// Returns application frames accepted by the shared headless core.
    pub const fn logic_frames(&self) -> u64 {
        self.logic_frames
    }

    /// Returns frames submitted and presented by Sim;Engine.
    pub const fn drawn_frames(&self) -> u64 {
        self.drawn_frames
    }

    /// Returns temporary surface skips reported by Sim;Engine.
    pub const fn skipped_frames(&self) -> u64 {
        self.skipped_frames
    }

    /// Returns successful logical World replacements.
    pub const fn committed_transitions(&self) -> u64 {
        self.committed_transitions
    }

    /// Returns successful explicit device-and-surface recoveries.
    pub const fn device_recoveries(&self) -> u64 {
        self.device_recoveries
    }

    /// Returns why the desktop loop ended successfully.
    pub const fn exit_reason(&self) -> DesktopExitReason {
        self.exit_reason
    }

    /// Returns the last successfully completed logical frame, if any.
    pub const fn last_logic_frame(&self) -> Option<&LogicFrameReport> {
        self.last_logic_frame.as_ref()
    }

    /// Returns the last Sim;Engine presentation report, including a temporary
    /// surface skip when that was the most recent attempt.
    pub const fn last_render_frame(&self) -> Option<FrameReport> {
        self.last_render_frame
    }

    /// Returns the latest frame's separate successful 3D depth-prepass report.
    ///
    /// This GPU work is outside FrameReport's composition budget and timings.
    /// It may have been submitted even if the later surface frame was skipped.
    /// A frame without an enabled 3D view resets this value to None.
    pub const fn last_three_d_frame(&self) -> Option<Mesh3dRenderReport> {
        self.last_three_d_frame
    }

    /// Returns resource-update work from the latest successfully prepared 3D frame.
    /// A presentation attempt without a 3D view resets this value to `None`.
    /// Updates can have occurred even if subsequent surface presentation skipped.
    pub const fn last_three_d_updates(&self) -> Option<DesktopThreeDUpdates> {
        self.last_three_d_updates
    }

    /// Returns saturating totals across all successful 3D preparations in this run.
    /// Device recovery does not reset these host totals. They exclude work from a
    /// failed preparation and are not a measurement of opaque driver allocations.
    pub const fn three_d_updates(&self) -> DesktopThreeDUpdates {
        self.three_d_updates
    }

    /// Returns current-device hardware timing status, losses and bounded samples.
    /// Samples are associated by exact submission ID and source, not arrival order.
    /// Final collection is nonblocking, so pending work need not appear here.
    pub const fn gpu_timings(&self) -> &DesktopGpuTimings {
        &self.gpu_timings
    }

    fn record_application_exit(&mut self, report: LogicFrameReport) {
        self.exit_reason = DesktopExitReason::ApplicationRequested;
        self.last_logic_frame = Some(report);
    }
}

/// Fatal reason the one-window desktop loop stopped.
#[derive(Debug)]
pub enum DesktopRunError {
    /// The shared logical runner could not be built.
    Runner(RunnerBuildError),
    /// Winit could not create or drive its event loop.
    EventLoop(EventLoopError),
    /// Winit could not create the application window.
    Window(OsError),
    /// Sim;Engine rejected display or resize configuration.
    RendererConfiguration(RendererConfigurationError),
    /// Surface reconfiguration failed after one logical frame had already
    /// completed successfully.
    RendererConfigurationAfterFrame {
        /// Renderer resize or scale configuration failure.
        error: RendererConfigurationError,
        /// Canonical logical outcome that preceded reconfiguration.
        logic_frame: Box<LogicFrameReport>,
    },
    /// Sim;Engine could not derive a positive finite logical viewport.
    LogicalViewport(sim_engine::LogicalViewportError),
    /// Delivered desktop pointer geometry could not form a valid logical sample.
    Pointer(DesktopPointerError),
    /// Sim;Engine could not create its renderer.
    RendererInitialization(RendererInitError),
    /// The bounded desktop collector received too many mapped input events.
    InputEventLimitExceeded {
        /// Frozen maximum mapped input events between logical frames.
        limit: usize,
    },
    /// The desktop collector could not reserve bounded input storage.
    InputBufferAllocationFailed,
    /// The shared core rejected BeginFrame atomically.
    BeginFrame(BeginFrameRejection),
    /// A private desktop/core ownership invariant was violated.
    RuntimeInvariant,
    /// A logical frame began but ended with a typed stage or extraction
    /// failure. The adapter attempted one clear-only diagnostic presentation
    /// without reusing the last World snapshot.
    LogicFrame {
        /// Complete logical report for the failed frame.
        report: Box<LogicFrameReport>,
        /// Result of the separate diagnostic-only presentation attempt.
        diagnostic_presentation: Box<Result<FrameReport, FrameComposerError>>,
    },
    /// Sim;Engine rejected frame composition or presentation after the
    /// logical frame had already completed.
    Presentation {
        /// Renderer or frame-composer failure.
        error: FrameComposerError,
        /// Canonical logical outcome that preceded the presentation failure.
        logic_frame: Box<LogicFrameReport>,
    },
    /// Preparing an uncached screen image failed after the logical frame
    /// completed. Previously uploaded immutable assets can remain cached;
    /// the adapter does not present a partially composed frame.
    ImagePreparation {
        /// Source-attributed image or cache preparation failure.
        error: DesktopImageError,
        /// Canonical logical outcome that preceded image preparation.
        logic_frame: Box<LogicFrameReport>,
    },
    /// Preparing managed text failed after CPU publication. Earlier cache
    /// updates may remain, but no partial surface frame is presented.
    #[cfg(feature = "text")]
    TextPreparation {
        /// Source-attributed font atlas, glyph run, or cache failure.
        error: DesktopTextError,
        /// Canonical logical outcome preceding the failure.
        logic_frame: Box<LogicFrameReport>,
    },
    /// Preparing or submitting the separate 3D depth prepass failed after the
    /// logical frame completed. Retained resources can remain warmed; no
    /// partial surface frame is presented and canonical state is not rolled back.
    ThreeDPreparation {
        /// Concrete bounded preparation or whole-scene renderer error.
        error: DesktopThreeDError,
        /// Canonical logical outcome preceding the failed 3D preparation.
        logic_frame: Box<LogicFrameReport>,
    },
    /// The one permitted recovery attempt for a lost surface failed after the
    /// logical frame had already completed.
    Recovery {
        /// Renderer recovery failure.
        error: RendererInitError,
        /// Canonical logical outcome that preceded the recovery attempt.
        logic_frame: Box<LogicFrameReport>,
    },
}

impl fmt::Display for DesktopRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runner(error) => write!(formatter, "logical runner failed: {error}"),
            Self::EventLoop(error) => write!(formatter, "desktop event loop failed: {error}"),
            Self::Window(error) => write!(formatter, "desktop window failed: {error}"),
            Self::RendererConfiguration(error) => {
                write!(formatter, "renderer configuration failed: {error}")
            }
            Self::RendererConfigurationAfterFrame { error, .. } => {
                write!(
                    formatter,
                    "post-frame renderer configuration failed: {error}"
                )
            }
            Self::LogicalViewport(error) => write!(formatter, "logical viewport failed: {error}"),
            Self::Pointer(error) => write!(formatter, "desktop pointer conversion failed: {error}"),
            Self::RendererInitialization(error) => {
                write!(formatter, "renderer initialization failed: {error}")
            }
            Self::InputEventLimitExceeded { limit } => write!(
                formatter,
                "desktop input exceeded its limit of {limit} mapped events"
            ),
            Self::InputBufferAllocationFailed => {
                formatter.write_str("desktop input buffer allocation failed")
            }
            Self::BeginFrame(error) => write!(formatter, "logical frame was rejected: {error}"),
            Self::RuntimeInvariant => {
                formatter.write_str("desktop adapter lost required runtime state")
            }
            Self::LogicFrame { report, .. } => write!(
                formatter,
                "logical frame {} failed after it began",
                report.frame_index()
            ),
            Self::Presentation { error, .. } => {
                write!(formatter, "frame presentation failed: {error}")
            }
            Self::ImagePreparation { error, .. } => {
                write!(formatter, "desktop image preparation failed: {error}")
            }
            #[cfg(feature = "text")]
            Self::TextPreparation { error, .. } => {
                write!(formatter, "desktop text preparation failed: {error}")
            }
            Self::ThreeDPreparation { error, .. } => {
                write!(formatter, "desktop 3D preparation failed: {error}")
            }
            Self::Recovery { error, .. } => write!(formatter, "renderer recovery failed: {error}"),
        }
    }
}

impl Error for DesktopRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Runner(error) => Some(error),
            Self::EventLoop(error) => Some(error),
            Self::Window(error) => Some(error),
            Self::RendererConfiguration(error) => Some(error),
            Self::RendererConfigurationAfterFrame { error, .. } => Some(error),
            Self::LogicalViewport(error) => Some(error),
            Self::Pointer(error) => Some(error),
            Self::RendererInitialization(error) => Some(error),
            Self::BeginFrame(error) => Some(error),
            Self::Presentation { error, .. } => Some(error),
            Self::ImagePreparation { error, .. } => Some(error),
            #[cfg(feature = "text")]
            Self::TextPreparation { error, .. } => Some(error),
            Self::ThreeDPreparation { error, .. } => Some(error),
            Self::Recovery { error, .. } => Some(error),
            Self::InputEventLimitExceeded { .. }
            | Self::InputBufferAllocationFailed
            | Self::RuntimeInvariant => None,
            Self::LogicFrame { report, .. } => report
                .failure()
                .map(|error| error as &(dyn Error + 'static)),
        }
    }
}

pub(crate) fn run<A: Action>(
    application: Application<A>,
    initial: WorldFactoryId,
    config: DesktopConfig,
) -> Result<DesktopRunReport, DesktopRunError> {
    let input_event_limit = application.config.input_event_limit();
    let frame_budget = frame_budget(application.config.render().frame_limits());
    let three_d_limits = application.config.render().three_d();
    let runner = application
        .build_headless(initial)
        .map_err(DesktopRunError::Runner)?;
    let event_loop = EventLoop::new().map_err(DesktopRunError::EventLoop)?;
    let mut desktop = DesktopHost::new(
        runner,
        config,
        input_event_limit,
        frame_budget,
        three_d_limits,
    );
    event_loop
        .run_app(&mut desktop)
        .map_err(DesktopRunError::EventLoop)?;
    desktop.collect_gpu_timings();
    match desktop.fatal {
        Some(error) => Err(error),
        None => Ok(desktop.report),
    }
}

struct DesktopHost<A: Action> {
    runner: HeadlessRunner<A>,
    config: DesktopConfig,
    input_event_limit: usize,
    frame_budget: FrameBudget,
    window: Option<Arc<Window>>,
    renderer: Option<WgpuRenderer>,
    images: DesktopImages,
    three_d: DesktopThreeD,
    three_d_limits: ThreeDRenderLimits,
    pending_events: Vec<InputEvent>,
    input_failure: Option<InputBufferFailure>,
    held_keys: [bool; SUPPORTED_PHYSICAL_KEY_COUNT],
    pointer_geometry: DesktopPointerGeometry,
    capture: DesktopCapture,
    window_occluded: bool,
    surface_waiting: bool,
    last_frame: Instant,
    report: DesktopRunReport,
    fatal: Option<DesktopRunError>,
}

impl<A: Action> DesktopHost<A> {
    fn new(
        runner: HeadlessRunner<A>,
        config: DesktopConfig,
        input_event_limit: usize,
        frame_budget: FrameBudget,
        three_d_limits: ThreeDRenderLimits,
    ) -> Self {
        Self {
            runner,
            config,
            input_event_limit,
            frame_budget,
            window: None,
            renderer: None,
            images: DesktopImages::new(),
            three_d: DesktopThreeD::new(),
            three_d_limits,
            pending_events: Vec::new(),
            input_failure: None,
            held_keys: [false; SUPPORTED_PHYSICAL_KEY_COUNT],
            pointer_geometry: DesktopPointerGeometry::empty(),
            capture: DesktopCapture::new(),
            window_occluded: false,
            surface_waiting: false,
            last_frame: Instant::now(),
            report: DesktopRunReport::default(),
            fatal: None,
        }
    }

    fn stop(&mut self, event_loop: &ActiveEventLoop, error: DesktopRunError) {
        self.release_capture();
        if self.fatal.is_none() {
            self.fatal = Some(error);
        }
        event_loop.exit();
    }

    fn release_capture(&mut self) {
        let status = self.capture.release();
        self.runner.with_pointer_capture(|capture| {
            capture.release();
            capture.acknowledge(status);
        });
    }

    fn synchronize_capture(&mut self) {
        if self
            .runner
            .app_resource::<PointerCapture>()
            .is_some_and(PointerCapture::pending)
        {
            let was_captured = self.capture.accepts_motion();
            let backend = &mut self.capture;
            self.runner
                .with_pointer_capture(|capture| backend.synchronize(capture));
            self.collect_capture_change(was_captured, self.capture.accepts_motion());
        }
    }

    fn collect_capture_change(&mut self, was_captured: bool, captured: bool) {
        // Captured CursorMoved events were deliberately not sent to UI. After
        // changing ownership, the old absolute sample cannot describe the newly
        // visible pointer. Queue the boundary before any later click and wait
        // for fresh CursorMoved; also cancel an unfinished mouse gesture.
        // synchronize_capture runs after the preceding frame queue was drained.
        // Focus loss has its own single FocusLost boundary instead.
        if was_captured != captured && self.collect_input(InputEvent::PointerLeft) {
            self.pointer_geometry.cursor_left();
        }
    }

    fn collect_relative_motion(&mut self, delta: (f64, f64)) -> Result<(), DesktopPointerError> {
        if !self.capture.accepts_motion() || self.input_failure.is_some() {
            return Ok(());
        }
        let motion = RelativePointerMotion::new(delta.0, delta.1)
            .map_err(DesktopPointerError::RelativeMotion)?;
        self.collect_input(InputEvent::relative_pointer_motion(motion));
        Ok(())
    }

    fn collect_mouse_wheel(&mut self, delta: MouseScrollDelta) -> Result<(), DesktopPointerError> {
        if self.input_failure.is_some() {
            return Ok(());
        }
        // Do not merge wheel events: a line and a pixel have different units,
        // and each event's pointer must stay ordered relative to clicks.
        let event = self.pointer_geometry.mouse_wheel(delta)?;
        self.collect_input(event);
        Ok(())
    }

    fn collect_gpu_timings(&mut self) {
        if let Some(renderer) = self.renderer.as_mut() {
            self.report.gpu_timings.collect(renderer);
        }
    }

    fn collect_key(
        &mut self,
        physical_key: PhysicalKey,
        state: ElementState,
        is_synthetic: bool,
        repeat: bool,
    ) {
        // X11 can synthesize releases before Focused(false), and restore
        // presses on focus gain. Neither is a new physical user action.
        // Repeats must not reactivate a key cancelled by a focus boundary;
        // returning to the window requires a fresh non-repeat press.
        if is_synthetic || (repeat && state == ElementState::Pressed) {
            return;
        }
        let Some(key) = map_key(physical_key) else {
            return;
        };
        self.collect_physical_key(key, map_button_state(state));
    }

    fn collect_physical_key(&mut self, key: PhysicalKeyCode, state: ButtonState) {
        if self.collect_input(InputEvent::key(key, state)) {
            self.held_keys[physical_key_index(key)] = state == ButtonState::Pressed;
        }
    }

    fn collect_input(&mut self, event: InputEvent) -> bool {
        if self.input_failure.is_some() {
            return false;
        }
        if let InputEvent::RelativePointerMotion { motion } = event
            && let Some(InputEvent::RelativePointerMotion { motion: previous }) =
                self.pending_events.last_mut()
        {
            match previous.checked_add(motion) {
                Ok(sum) => *previous = sum,
                Err(error) => {
                    self.input_failure = Some(InputBufferFailure::RelativeMotion(error));
                    return false;
                }
            }
            return true;
        }
        // Only the trailing continuous sample is replaceable. A key, button,
        // or leave event keeps all preceding pointer geometry causal.
        if matches!(event, InputEvent::PointerMoved { .. })
            && let Some(last @ InputEvent::PointerMoved { .. }) = self.pending_events.last_mut()
        {
            *last = event;
            return true;
        }
        if self.pending_events.len() >= self.input_event_limit {
            self.input_failure = Some(InputBufferFailure::Limit);
            return false;
        }
        if self.pending_events.try_reserve(1).is_err() {
            self.input_failure = Some(InputBufferFailure::Allocation);
            return false;
        }
        self.pending_events.push(event);
        true
    }

    fn collect_cursor(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<(), DesktopPointerError> {
        if self.input_failure.is_some() || self.capture.accepts_motion() {
            return Ok(());
        }
        let mut geometry = self.pointer_geometry;
        let event = geometry.cursor_moved(position)?;
        if self.collect_input(event) {
            self.pointer_geometry = geometry;
        }
        Ok(())
    }

    fn collect_mouse_button(&mut self, button: winit::event::MouseButton, state: ElementState) {
        if let Some(button) = pointer::map_mouse_button(button) {
            self.collect_input(InputEvent::mouse_button(button, map_button_state(state)));
        }
    }

    fn collect_pointer_left(&mut self) {
        if self.capture.accepts_motion() {
            return;
        }
        if self.collect_input(InputEvent::PointerLeft) {
            self.pointer_geometry.cursor_left();
        }
    }

    fn collect_resize(&mut self, size: PhysicalSize<u32>) -> Result<(), DesktopPointerError> {
        if self.input_failure.is_some() {
            return Ok(());
        }
        let mut geometry = self.pointer_geometry;
        if geometry
            .resized(size)?
            .is_none_or(|event| self.collect_input(event))
        {
            self.pointer_geometry = geometry;
        }
        Ok(())
    }

    fn collect_scale_factor(&mut self, scale_factor: f64) -> Result<(), DesktopPointerError> {
        if self.input_failure.is_some() {
            return Ok(());
        }
        let mut geometry = self.pointer_geometry;
        if geometry
            .scale_factor_changed(scale_factor)?
            .is_none_or(|event| self.collect_input(event))
        {
            self.pointer_geometry = geometry;
        }
        Ok(())
    }

    fn collect_focus_loss(&mut self) {
        self.capture.set_focused(false);
        self.release_capture();
        // The shared core owns cancellation expansion and its exact ordered
        // edge preflight. Queue one boundary, not a partially accepted batch
        // of ordinary releases that could accidentally complete user actions.
        if self.collect_input(InputEvent::FocusLost) {
            self.held_keys.fill(false);
            self.pointer_geometry.cursor_left();
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        if !callback_may_run(self.fatal.is_some(), event_loop.exiting()) {
            return;
        }
        // Service ready timestamps even when input, occlusion or an application
        // exit prevents this redraw from submitting another frame.
        self.collect_gpu_timings();
        if let Some(failure) = self.input_failure {
            let error = match failure {
                InputBufferFailure::Limit => DesktopRunError::InputEventLimitExceeded {
                    limit: self.input_event_limit,
                },
                InputBufferFailure::Allocation => DesktopRunError::InputBufferAllocationFailed,
                InputBufferFailure::RelativeMotion(error) => {
                    DesktopRunError::Pointer(DesktopPointerError::RelativeMotion(error))
                }
            };
            self.stop(event_loop, error);
            return;
        }

        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };
        let viewport = match renderer.logical_viewport() {
            Ok(viewport) => viewport,
            Err(error) => {
                self.stop(event_loop, DesktopRunError::LogicalViewport(error));
                return;
            }
        };
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_frame);
        self.last_frame = now;
        let outcome = advance_shared_core(
            &mut self.runner,
            FrameRequest::new(elapsed, &self.pending_events, viewport),
        );
        self.pending_events.clear();

        let report = match outcome {
            FrameOutcome::Rejected(error) => {
                self.stop(event_loop, DesktopRunError::BeginFrame(error));
                return;
            }
            FrameOutcome::Advanced(report) => report,
        };
        self.synchronize_capture();
        self.report.logic_frames = self.report.logic_frames.saturating_add(1);
        if matches!(report.transition(), FrameTransition::Committed { .. }) {
            self.report.committed_transitions = self.report.committed_transitions.saturating_add(1);
        }
        if transition_resets_wall_clock(report.transition()) {
            // Candidate preparation is synchronous adapter work, not elapsed
            // simulation time for the next frame.
            self.last_frame = Instant::now();
        }
        let active_generation = self.runner.world_generation();
        let snapshot_generation = self
            .runner
            .extracted_frame()
            .map(crate::ExtractedFrame::world_generation);
        match presentation_decision(
            &report,
            snapshot_generation,
            active_generation,
            self.window_occluded,
        ) {
            PresentationDecision::DiagnosticsOnly => {
                let diagnostic_presentation = match self.renderer.as_mut() {
                    Some(renderer) => present_diagnostics_only(renderer, self.frame_budget),
                    None => {
                        self.stop(event_loop, DesktopRunError::RuntimeInvariant);
                        return;
                    }
                };
                self.stop(
                    event_loop,
                    DesktopRunError::LogicFrame {
                        report: Box::new(report),
                        diagnostic_presentation: Box::new(diagnostic_presentation),
                    },
                );
                return;
            }
            PresentationDecision::ApplicationExit => {
                self.release_capture();
                self.report.record_application_exit(report);
                event_loop.exit();
                return;
            }
            PresentationDecision::SkipStaleSnapshot | PresentationDecision::SkipOccludedWindow => {
                self.report.last_logic_frame = Some(report);
                return;
            }
            PresentationDecision::PresentWorld => {}
        }

        let Some(extracted) = self.runner.extracted_frame() else {
            self.stop(event_loop, DesktopRunError::RuntimeInvariant);
            return;
        };
        let Some(renderer) = self.renderer.as_mut() else {
            self.stop(event_loop, DesktopRunError::RuntimeInvariant);
            return;
        };
        self.report.last_three_d_frame = None;
        self.report.last_three_d_updates = None;
        if extracted.three_d().is_none() {
            // A World with no active 3D view must not keep a retired World's
            // chunk revisions alive through renderer caches.
            if self.three_d.color_target().is_some() {
                renderer.clear_frame_cache();
            }
            self.three_d.clear();
        }
        let presentation = if let Some(snapshot) = extracted.three_d() {
            (|| {
                self.images.preflight_three_d(
                    extracted,
                    self.runner.image_assets(),
                    self.frame_budget,
                    three_d::color_target_bytes(renderer)?,
                )?;
                let three_d_report =
                    self.three_d
                        .prepare(renderer, snapshot, self.three_d_limits)?;
                self.report.gpu_timings.submitted(
                    report.frame_index(),
                    GpuTimingSource::Scene3d,
                    three_d_report.gpu_timing_id(),
                );
                self.report.last_three_d_frame = Some(three_d_report);
                let updates = self.three_d.updates();
                self.report.last_three_d_updates = Some(updates);
                self.report.three_d_updates.accumulate(updates);
                images::present(
                    renderer,
                    extracted,
                    self.runner.image_assets(),
                    &mut self.images,
                    self.frame_budget,
                    self.three_d.color_target(),
                )
            })()
        } else if !images::needs_managed_presentation(extracted, &self.images) {
            present_extracted(renderer, extracted, self.frame_budget)
                .map_err(ScreenPresentationError::Composition)
        } else {
            images::present(
                renderer,
                extracted,
                self.runner.image_assets(),
                &mut self.images,
                self.frame_budget,
                None,
            )
        };
        let presentation = match presentation {
            Ok(report) => Ok(report),
            Err(ScreenPresentationError::Composition(error)) => Err(error),
            Err(ScreenPresentationError::Images(error)) => {
                self.stop(
                    event_loop,
                    DesktopRunError::ImagePreparation {
                        error,
                        logic_frame: Box::new(report),
                    },
                );
                return;
            }
            #[cfg(feature = "text")]
            Err(ScreenPresentationError::Text(error)) => {
                self.stop(
                    event_loop,
                    DesktopRunError::TextPreparation {
                        error,
                        logic_frame: Box::new(report),
                    },
                );
                return;
            }
            Err(ScreenPresentationError::ThreeD(error)) => {
                self.stop(
                    event_loop,
                    DesktopRunError::ThreeDPreparation {
                        error,
                        logic_frame: Box::new(report),
                    },
                );
                return;
            }
        };
        match presentation {
            Ok(render_report) => {
                self.report.gpu_timings.submitted(
                    report.frame_index(),
                    GpuTimingSource::FrameComposer,
                    render_report.gpu_timing_id(),
                );
                self.report.last_render_frame = Some(render_report);
                match render_report.status() {
                    RenderStatus::Drawn => {
                        self.report.drawn_frames = self.report.drawn_frames.saturating_add(1);
                        self.surface_waiting = false;
                    }
                    RenderStatus::Skipped(status) => {
                        self.report.skipped_frames = self.report.skipped_frames.saturating_add(1);
                        if let Err(error) = self.handle_skip(status) {
                            let error = match error {
                                SkipHandlingError::RendererConfiguration(error) => {
                                    DesktopRunError::RendererConfigurationAfterFrame {
                                        error,
                                        logic_frame: Box::new(report),
                                    }
                                }
                                SkipHandlingError::Presentation(error) => {
                                    DesktopRunError::Presentation {
                                        error,
                                        logic_frame: Box::new(report),
                                    }
                                }
                            };
                            self.stop(event_loop, error);
                            return;
                        }
                    }
                }
            }
            Err(error) => match renderer_failure_action(&error) {
                RendererFailureAction::RecoverDeviceAndSurface {
                    reset_wall_clock_on_success,
                } => {
                    let Some(renderer) = self.renderer.as_mut() else {
                        return;
                    };
                    match pollster::block_on(renderer.recover_device_and_surface()) {
                        Ok(()) => {
                            self.images.clear();
                            self.report.device_recoveries =
                                self.report.device_recoveries.saturating_add(1);
                            self.report.gpu_timings.reset(
                                self.report.device_recoveries,
                                renderer.gpu_timing_statistics(),
                            );
                            self.report.last_render_frame = None;
                            self.report.last_three_d_frame = None;
                            self.report.last_three_d_updates = None;
                            if let Err(error) = self.three_d.restore(renderer) {
                                self.stop(
                                    event_loop,
                                    DesktopRunError::ThreeDPreparation {
                                        error,
                                        logic_frame: Box::new(report),
                                    },
                                );
                                return;
                            }
                            if reset_wall_clock_on_success {
                                self.last_frame = Instant::now();
                            }
                            self.surface_waiting = false;
                        }
                        Err(error) => {
                            self.stop(
                                event_loop,
                                DesktopRunError::Recovery {
                                    error,
                                    logic_frame: Box::new(report),
                                },
                            );
                            return;
                        }
                    }
                }
                RendererFailureAction::Fail => {
                    self.stop(
                        event_loop,
                        DesktopRunError::Presentation {
                            error,
                            logic_frame: Box::new(report),
                        },
                    );
                    return;
                }
            },
        }
        self.report.last_logic_frame = Some(report);
    }

    fn handle_skip(&mut self, status: RendererSurfaceStatus) -> Result<(), SkipHandlingError> {
        match skipped_surface_action(status) {
            SkippedSurfaceAction::RetryLater => self.surface_waiting = false,
            SkippedSurfaceAction::WaitForWindowEvent => self.surface_waiting = true,
            SkippedSurfaceAction::ReconfigureThenRetry => {
                self.surface_waiting = false;
                self.resize_renderer()
                    .map_err(SkipHandlingError::RendererConfiguration)?;
            }
            SkippedSurfaceAction::Fail => {
                return Err(SkipHandlingError::Presentation(FrameComposerError::Frame(
                    RendererFrameError::Surface(status),
                )));
            }
        }
        Ok(())
    }

    fn resize_renderer(&mut self) -> Result<(), RendererConfigurationError> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        let size = self.pointer_geometry.physical_size();
        renderer.resize_with_scale_factor(
            size.width,
            size.height,
            self.pointer_geometry.scale_factor(),
        )
    }
}

impl<A: Action> ApplicationHandler for DesktopHost<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !callback_may_run(self.fatal.is_some(), event_loop.exiting()) || self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(LogicalSize::new(
                self.config.logical_width,
                self.config.logical_height,
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.stop(event_loop, DesktopRunError::Window(error));
                return;
            }
        };
        let size = window.inner_size();
        let scale_factor = window.scale_factor();
        self.pointer_geometry = match DesktopPointerGeometry::new(size, scale_factor) {
            Ok(geometry) => geometry,
            Err(error) => {
                self.stop(event_loop, DesktopRunError::Pointer(error));
                return;
            }
        };
        let options = match WgpuRendererOptions::new(self.config.present_mode, scale_factor) {
            Ok(options) => options.with_gpu_timing(self.config.gpu_timing),
            Err(error) => {
                self.stop(event_loop, DesktopRunError::RendererConfiguration(error));
                return;
            }
        };
        let mut renderer = match pollster::block_on(WgpuRenderer::new_with_options(
            Arc::clone(&window),
            size.width.max(1),
            size.height.max(1),
            options,
        )) {
            Ok(renderer) => renderer,
            Err(error) => {
                self.stop(event_loop, DesktopRunError::RendererInitialization(error));
                return;
            }
        };
        let notify_window = Arc::clone(&window);
        renderer.set_frame_cache_budget(self.config.frame_cache);
        renderer.set_pre_present_notify(move || notify_window.pre_present_notify());
        self.report
            .gpu_timings
            .reset(0, renderer.gpu_timing_statistics());
        self.window_occluded = extent_is_occluded(size.width, size.height);
        self.last_frame = Instant::now();
        self.capture.attach(Arc::clone(&window));
        self.window = Some(window);
        self.images.clear();
        self.three_d.clear();
        self.renderer = Some(renderer);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Winit may deliver already queued input/redraw after exit(). Neither
        // closing nor a failure permits another logical frame or cursor grab.
        if !callback_may_run(self.fatal.is_some(), event_loop.exiting()) {
            return;
        }
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.release_capture();
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => self.collect_key(event.physical_key, event.state, is_synthetic, event.repeat),
            WindowEvent::CursorMoved { position, .. } => {
                if let Err(error) = self.collect_cursor(position) {
                    self.stop(event_loop, DesktopRunError::Pointer(error));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.collect_mouse_button(button, state)
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Err(error) = self.collect_mouse_wheel(delta) {
                    self.stop(event_loop, DesktopRunError::Pointer(error));
                }
            }
            WindowEvent::CursorLeft { .. } => self.collect_pointer_left(),
            WindowEvent::Focused(false) => self.collect_focus_loss(),
            WindowEvent::Focused(true) => {
                self.capture.set_focused(true);
                self.surface_waiting = false;
            }
            WindowEvent::Resized(size) => {
                if let Err(error) = self.collect_resize(size) {
                    self.stop(event_loop, DesktopRunError::Pointer(error));
                    return;
                }
                self.window_occluded = extent_is_occluded(size.width, size.height);
                if !self.window_occluded {
                    self.surface_waiting = false;
                }
                if let Err(error) = self.resize_renderer() {
                    self.stop(event_loop, DesktopRunError::RendererConfiguration(error));
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Err(error) = self.collect_scale_factor(scale_factor) {
                    self.stop(event_loop, DesktopRunError::Pointer(error));
                    return;
                }
                if let Some(renderer) = self.renderer.as_mut()
                    && let Err(error) = renderer.set_scale_factor(scale_factor)
                {
                    self.stop(event_loop, DesktopRunError::RendererConfiguration(error));
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.window_occluded = occluded;
                if !occluded {
                    self.surface_waiting = false;
                }
            }
            WindowEvent::RedrawRequested => {
                // An explicit host redraw is one new opportunity even after a
                // previous surface-level Occluded result.
                self.surface_waiting = false;
                self.redraw(event_loop);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if callback_may_run(self.fatal.is_some(), event_loop.exiting())
            && !self.window_occluded
            && !self.surface_waiting
            && let Some(window) = self.window.as_ref()
        {
            window.request_redraw();
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if !callback_may_run(self.fatal.is_some(), event_loop.exiting()) {
            return;
        }
        if let DeviceEvent::MouseMotion { delta } = event
            && let Err(error) = self.collect_relative_motion(delta)
        {
            self.stop(event_loop, DesktopRunError::Pointer(error));
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        if callback_may_run(self.fatal.is_some(), event_loop.exiting()) {
            self.collect_focus_loss();
        }
    }
}

/// All host callbacks share the same terminal guard. An OS close and an
/// application-requested exit are terminal even when no fatal error was stored.
const fn callback_may_run(failed: bool, exiting: bool) -> bool {
    !failed && !exiting
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationDecision {
    DiagnosticsOnly,
    ApplicationExit,
    SkipStaleSnapshot,
    SkipOccludedWindow,
    PresentWorld,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkippedSurfaceAction {
    RetryLater,
    WaitForWindowEvent,
    ReconfigureThenRetry,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RendererFailureAction {
    RecoverDeviceAndSurface { reset_wall_clock_on_success: bool },
    Fail,
}

enum SkipHandlingError {
    RendererConfiguration(RendererConfigurationError),
    Presentation(FrameComposerError),
}

fn advance_shared_core<A: Action>(
    runner: &mut HeadlessRunner<A>,
    request: FrameRequest<'_>,
) -> FrameOutcome {
    runner.advance_frame(request)
}

fn presentation_decision(
    report: &LogicFrameReport,
    snapshot: Option<WorldGeneration>,
    active: WorldGeneration,
    window_occluded: bool,
) -> PresentationDecision {
    if report.failure().is_some() {
        return PresentationDecision::DiagnosticsOnly;
    }
    if report.exit_requested() {
        return PresentationDecision::ApplicationExit;
    }
    if !snapshot_is_fresh(report.extracted_generation(), snapshot, active) {
        return PresentationDecision::SkipStaleSnapshot;
    }
    if window_occluded {
        return PresentationDecision::SkipOccludedWindow;
    }
    PresentationDecision::PresentWorld
}

const fn skipped_surface_action(status: RendererSurfaceStatus) -> SkippedSurfaceAction {
    match status {
        RendererSurfaceStatus::Timeout => SkippedSurfaceAction::RetryLater,
        RendererSurfaceStatus::Occluded => SkippedSurfaceAction::WaitForWindowEvent,
        RendererSurfaceStatus::Outdated => SkippedSurfaceAction::ReconfigureThenRetry,
        RendererSurfaceStatus::Lost | RendererSurfaceStatus::Validation => {
            SkippedSurfaceAction::Fail
        }
    }
}

const fn renderer_failure_action(error: &FrameComposerError) -> RendererFailureAction {
    if matches!(
        error,
        FrameComposerError::Frame(RendererFrameError::Surface(RendererSurfaceStatus::Lost))
    ) {
        RendererFailureAction::RecoverDeviceAndSurface {
            reset_wall_clock_on_success: true,
        }
    } else {
        RendererFailureAction::Fail
    }
}

const fn transition_resets_wall_clock(transition: &FrameTransition) -> bool {
    matches!(
        transition,
        FrameTransition::Committed { .. } | FrameTransition::PreparationFailed { .. }
    )
}

const fn extent_is_occluded(width: u32, height: u32) -> bool {
    width == 0 || height == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputBufferFailure {
    Limit,
    Allocation,
    RelativeMotion(RelativePointerMotionError),
}

fn present_extracted(
    renderer: &mut WgpuRenderer,
    extracted: &crate::ExtractedFrame,
    budget: FrameBudget,
) -> Result<FrameReport, FrameComposerError> {
    let mut frame = renderer.begin_frame(extracted.background(), budget)?;
    frame.draw_scene(
        extracted.world_scene(),
        extracted.camera(),
        FramePassOptions::new(0),
    )?;
    if let Some(options) = screen_pass_options(extracted.screen_scene().command_count()) {
        frame.draw_screen_scene(extracted.screen_scene(), options)?;
    }
    frame.present()
}

fn screen_pass_options(command_count: usize) -> Option<FramePassOptions> {
    (command_count != 0).then(|| FramePassOptions::new(1))
}

fn present_diagnostics_only(
    renderer: &mut WgpuRenderer,
    budget: FrameBudget,
) -> Result<FrameReport, FrameComposerError> {
    renderer
        .begin_frame(sim_engine::Color::BLACK, budget)?
        .present()
}

fn map_key(key: PhysicalKey) -> Option<PhysicalKeyCode> {
    match key {
        PhysicalKey::Code(KeyCode::KeyW) => Some(PhysicalKeyCode::KeyW),
        PhysicalKey::Code(KeyCode::KeyA) => Some(PhysicalKeyCode::KeyA),
        PhysicalKey::Code(KeyCode::KeyS) => Some(PhysicalKeyCode::KeyS),
        PhysicalKey::Code(KeyCode::KeyD) => Some(PhysicalKeyCode::KeyD),
        PhysicalKey::Code(KeyCode::Enter) => Some(PhysicalKeyCode::Enter),
        PhysicalKey::Code(KeyCode::Space) => Some(PhysicalKeyCode::Space),
        PhysicalKey::Code(KeyCode::ArrowLeft) => Some(PhysicalKeyCode::ArrowLeft),
        PhysicalKey::Code(KeyCode::ArrowRight) => Some(PhysicalKeyCode::ArrowRight),
        PhysicalKey::Code(KeyCode::ArrowDown) => Some(PhysicalKeyCode::ArrowDown),
        PhysicalKey::Code(KeyCode::ArrowUp) => Some(PhysicalKeyCode::ArrowUp),
        PhysicalKey::Code(KeyCode::Escape) => Some(PhysicalKeyCode::Escape),
        PhysicalKey::Code(KeyCode::KeyP) => Some(PhysicalKeyCode::KeyP),
        PhysicalKey::Code(KeyCode::KeyR) => Some(PhysicalKeyCode::KeyR),
        PhysicalKey::Code(KeyCode::KeyN) => Some(PhysicalKeyCode::KeyN),
        PhysicalKey::Code(KeyCode::Digit1) => Some(PhysicalKeyCode::Digit1),
        PhysicalKey::Code(KeyCode::Digit2) => Some(PhysicalKeyCode::Digit2),
        PhysicalKey::Code(KeyCode::Digit3) => Some(PhysicalKeyCode::Digit3),
        PhysicalKey::Code(KeyCode::Digit4) => Some(PhysicalKeyCode::Digit4),
        PhysicalKey::Code(KeyCode::Digit5) => Some(PhysicalKeyCode::Digit5),
        PhysicalKey::Code(KeyCode::F3) => Some(PhysicalKeyCode::F3),
        PhysicalKey::Code(KeyCode::F4) => Some(PhysicalKeyCode::F4),
        PhysicalKey::Code(KeyCode::F5) => Some(PhysicalKeyCode::F5),
        PhysicalKey::Code(KeyCode::F6) => Some(PhysicalKeyCode::F6),
        PhysicalKey::Code(KeyCode::F8) => Some(PhysicalKeyCode::F8),
        PhysicalKey::Code(KeyCode::F9) => Some(PhysicalKeyCode::F9),
        PhysicalKey::Code(KeyCode::KeyL) => Some(PhysicalKeyCode::KeyL),
        PhysicalKey::Code(KeyCode::KeyF) => Some(PhysicalKeyCode::KeyF),
        PhysicalKey::Code(KeyCode::KeyM) => Some(PhysicalKeyCode::KeyM),
        PhysicalKey::Code(KeyCode::KeyT) => Some(PhysicalKeyCode::KeyT),
        PhysicalKey::Code(KeyCode::KeyV) => Some(PhysicalKeyCode::KeyV),
        PhysicalKey::Code(KeyCode::Digit6) => Some(PhysicalKeyCode::Digit6),
        PhysicalKey::Code(KeyCode::Digit7) => Some(PhysicalKeyCode::Digit7),
        PhysicalKey::Code(KeyCode::Digit8) => Some(PhysicalKeyCode::Digit8),
        PhysicalKey::Code(KeyCode::Digit9) => Some(PhysicalKeyCode::Digit9),
        PhysicalKey::Code(KeyCode::F7) => Some(PhysicalKeyCode::F7),
        PhysicalKey::Code(KeyCode::KeyE) => Some(PhysicalKeyCode::KeyE),
        PhysicalKey::Code(KeyCode::ShiftLeft) => Some(PhysicalKeyCode::ShiftLeft),
        PhysicalKey::Code(_) | PhysicalKey::Unidentified(_) => None,
    }
}

const fn map_button_state(state: ElementState) -> ButtonState {
    match state {
        ElementState::Pressed => ButtonState::Pressed,
        ElementState::Released => ButtonState::Released,
    }
}

fn snapshot_is_fresh(
    report: Option<crate::identity::WorldGeneration>,
    snapshot: Option<crate::identity::WorldGeneration>,
    active: crate::identity::WorldGeneration,
) -> bool {
    matches!((report, snapshot), (Some(report), Some(snapshot)) if report == active && snapshot == active)
}

const fn frame_budget(limits: FrameLimits) -> FrameBudget {
    FrameBudget::new(
        limits.max_passes(),
        limits.max_commands(),
        limits.max_vertices(),
        limits.max_upload_bytes(),
        limits.max_texture_bytes(),
        limits.max_draw_calls(),
    )
}

fn validate_title(title: &str) -> Result<(), DesktopConfigError> {
    if title.trim().is_empty() {
        return Err(DesktopConfigError::EmptyTitle);
    }
    if title.len() > MAX_TITLE_BYTES {
        return Err(DesktopConfigError::TitleTooLong {
            limit: MAX_TITLE_BYTES,
        });
    }
    Ok(())
}

fn validate_logical_size(width: f64, height: f64) -> Result<(), DesktopConfigError> {
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        Ok(())
    } else {
        Err(DesktopConfigError::InvalidLogicalSize { width, height })
    }
}

#[cfg(test)]
#[path = "desktop/pointer_tests.rs"]
mod pointer_tests;

#[cfg(test)]
#[path = "desktop/input_tests.rs"]
mod input_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{error::Error, time::Duration};

    use sim_engine::{Camera2d, Color, LogicalViewport, Vec2};

    use crate::{
        ExtractionError,
        app::{AppConfig, Application},
        commands::LogicCommands,
        headless::{CandidateFailure, FrameFailure},
        identity::{ApplicationId, WorldGeneration},
        input::{ALL_PHYSICAL_KEYS, FixedInput, InputEvent},
        query::Query,
        system::Stage,
        time::FixedTime,
        visual::{ActiveCamera2d, CircleVisual, Transform2d},
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestAction {
        MoveRight,
    }

    fn move_circle(
        input: FixedInput<TestAction>,
        time: FixedTime,
        mut circles: Query<&mut Transform2d>,
    ) {
        if !input.held(TestAction::MoveRight) {
            return;
        }
        for mut transform in &mut circles {
            assert!(
                transform
                    .translate_by(Vec2::new(time.seconds_f32() * 60.0, 0.0))
                    .is_ok()
            );
        }
    }

    fn spawn_second_camera(mut commands: LogicCommands) {
        let camera = Camera2d::new(Vec2::ZERO, 32.0);
        let Ok(camera) = camera else {
            panic!("test camera should be valid");
        };
        assert!(commands.spawn(ActiveCamera2d::new(camera)).is_ok());
    }

    fn viewport(width: f32, height: f32) -> Result<LogicalViewport, Box<dyn Error>> {
        Ok(LogicalViewport::new(width, height)?)
    }

    fn parity_runner() -> Result<HeadlessRunner<TestAction>, Box<dyn Error>> {
        let mut application = Application::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
        let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
        let circle = CircleVisual::new(1.0, Color::rgb8(68, 144, 255))?;
        let initial = application.register_world("desktop-parity", move |world| {
            world.spawn(camera)?;
            world.spawn((Transform2d::default(), circle))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_circle);
        Ok(application.build_headless(initial)?)
    }

    #[test]
    fn desktop_configuration_rejects_unbounded_window_values() {
        assert!(matches!(
            DesktopConfig::new(" ", 800.0, 600.0),
            Err(DesktopConfigError::EmptyTitle)
        ));
        assert!(matches!(
            DesktopConfig::new("test", f64::NAN, 600.0),
            Err(DesktopConfigError::InvalidLogicalSize { .. })
        ));
    }

    #[test]
    fn gpu_timing_is_explicitly_opt_in_without_changing_window_or_cache_settings() {
        let mut config = DesktopConfig::new("GPU diagnostics", 960.0, 540.0).unwrap();
        let before = config.clone();
        assert!(!DesktopConfig::default().gpu_timing());
        assert!(!config.gpu_timing());
        config.set_gpu_timing(true);
        assert!(config.gpu_timing());
        let options = WgpuRendererOptions::new(config.present_mode(), 1.25)
            .unwrap()
            .with_gpu_timing(config.gpu_timing());
        assert!(options.gpu_timing());
        config.set_gpu_timing(false);
        assert_eq!(config, before);
        let report = DesktopRunReport::default();
        assert!(report.gpu_timings().statistics().is_none());
        assert!(report.last_three_d_updates().is_none());
        assert_eq!(report.three_d_updates(), DesktopThreeDUpdates::default());
    }

    #[test]
    fn screen_pass_is_optional_and_always_follows_the_world() {
        assert!(screen_pass_options(0).is_none());
        for count in [1, 256, usize::MAX] {
            assert_eq!(
                screen_pass_options(count).map(|options| options.order()),
                Some(1)
            );
        }
    }

    #[test]
    fn renderer_budget_conversion_is_exact() {
        let source = FrameLimits::new(2, 3, 5, 7, 11, 13);
        let converted = frame_budget(source);

        assert_eq!(converted.max_passes(), 2);
        assert_eq!(converted.max_commands(), 3);
        assert_eq!(converted.max_vertices(), 5);
        assert_eq!(converted.max_upload_bytes(), 7);
        assert_eq!(converted.max_texture_bytes(), 11);
        assert_eq!(converted.max_draw_calls(), 13);
    }

    #[test]
    fn desktop_maps_every_supported_physical_key() {
        let mappings = [
            (KeyCode::KeyW, PhysicalKeyCode::KeyW),
            (KeyCode::KeyA, PhysicalKeyCode::KeyA),
            (KeyCode::KeyS, PhysicalKeyCode::KeyS),
            (KeyCode::KeyD, PhysicalKeyCode::KeyD),
            (KeyCode::Enter, PhysicalKeyCode::Enter),
            (KeyCode::Space, PhysicalKeyCode::Space),
            (KeyCode::ArrowLeft, PhysicalKeyCode::ArrowLeft),
            (KeyCode::ArrowRight, PhysicalKeyCode::ArrowRight),
            (KeyCode::ArrowDown, PhysicalKeyCode::ArrowDown),
            (KeyCode::ArrowUp, PhysicalKeyCode::ArrowUp),
            (KeyCode::Escape, PhysicalKeyCode::Escape),
            (KeyCode::KeyP, PhysicalKeyCode::KeyP),
            (KeyCode::KeyR, PhysicalKeyCode::KeyR),
            (KeyCode::KeyN, PhysicalKeyCode::KeyN),
            (KeyCode::Digit1, PhysicalKeyCode::Digit1),
            (KeyCode::Digit2, PhysicalKeyCode::Digit2),
            (KeyCode::Digit3, PhysicalKeyCode::Digit3),
            (KeyCode::Digit4, PhysicalKeyCode::Digit4),
            (KeyCode::Digit5, PhysicalKeyCode::Digit5),
            (KeyCode::F3, PhysicalKeyCode::F3),
            (KeyCode::F4, PhysicalKeyCode::F4),
            (KeyCode::F5, PhysicalKeyCode::F5),
            (KeyCode::F6, PhysicalKeyCode::F6),
            (KeyCode::F8, PhysicalKeyCode::F8),
            (KeyCode::F9, PhysicalKeyCode::F9),
            (KeyCode::KeyL, PhysicalKeyCode::KeyL),
            (KeyCode::KeyF, PhysicalKeyCode::KeyF),
            (KeyCode::KeyM, PhysicalKeyCode::KeyM),
            (KeyCode::KeyT, PhysicalKeyCode::KeyT),
            (KeyCode::KeyV, PhysicalKeyCode::KeyV),
            (KeyCode::Digit6, PhysicalKeyCode::Digit6),
            (KeyCode::Digit7, PhysicalKeyCode::Digit7),
            (KeyCode::Digit8, PhysicalKeyCode::Digit8),
            (KeyCode::Digit9, PhysicalKeyCode::Digit9),
            (KeyCode::F7, PhysicalKeyCode::F7),
            (KeyCode::KeyE, PhysicalKeyCode::KeyE),
            (KeyCode::ShiftLeft, PhysicalKeyCode::ShiftLeft),
        ];

        assert_eq!(mappings.map(|(_, portable)| portable), ALL_PHYSICAL_KEYS);
        for (platform, portable) in mappings {
            assert_eq!(map_key(PhysicalKey::Code(platform)), Some(portable));
            assert_eq!(ALL_PHYSICAL_KEYS[physical_key_index(portable)], portable);
        }
        assert_eq!(map_key(PhysicalKey::Code(KeyCode::KeyQ)), None);
        assert_eq!(map_key(PhysicalKey::Code(KeyCode::Numpad1)), None);
        assert_eq!(
            map_key(PhysicalKey::Unidentified(
                winit::keyboard::NativeKeyCode::Unidentified
            )),
            None
        );
    }

    #[test]
    fn terminal_callbacks_cannot_run_after_close_or_failure() {
        assert!(callback_may_run(false, false));
        assert!(
            !callback_may_run(false, true),
            "normal exit has no fatal error"
        );
        assert!(
            !callback_may_run(true, false),
            "failure stops before exit dispatch"
        );
        assert!(!callback_may_run(true, true));
    }

    #[test]
    fn focus_loss_queues_one_core_owned_boundary_after_all_supported_keys()
    -> Result<(), Box<dyn Error>> {
        let mut host = DesktopHost::new(
            parity_runner()?,
            DesktopConfig::default(),
            ALL_PHYSICAL_KEYS.len() + 1,
            frame_budget(FrameLimits::default()),
            ThreeDRenderLimits::default(),
        );
        let new_keys = ALL_PHYSICAL_KEYS;

        for key in new_keys.into_iter().rev() {
            host.collect_physical_key(key, ButtonState::Pressed);
        }
        host.collect_focus_loss();

        assert_eq!(host.pending_events.len(), new_keys.len() + 1);
        for (index, key) in new_keys.into_iter().enumerate() {
            assert_eq!(
                host.pending_events[index],
                InputEvent::key(new_keys[new_keys.len() - index - 1], ButtonState::Pressed)
            );
            assert!(!host.held_keys[physical_key_index(key)]);
        }
        assert_eq!(host.pending_events.last(), Some(&InputEvent::FocusLost));
        assert_eq!(host.input_failure, None);
        Ok(())
    }

    #[test]
    fn stale_or_unpublished_snapshots_are_never_fresh() {
        let application = ApplicationId::from_raw(9);
        let old = WorldGeneration::new(application, 1);
        let active = WorldGeneration::new(application, 2);

        assert!(snapshot_is_fresh(Some(active), Some(active), active));
        assert!(!snapshot_is_fresh(Some(active), Some(old), active));
        assert!(!snapshot_is_fresh(Some(old), Some(active), active));
        assert!(!snapshot_is_fresh(None, Some(active), active));
        assert!(!snapshot_is_fresh(Some(active), None, active));
    }

    #[test]
    fn desktop_adapter_and_headless_call_the_same_core_lifecycle() -> Result<(), Box<dyn Error>> {
        let mut desktop_core = parity_runner()?;
        let mut headless_core = parity_runner()?;
        let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let logical_viewport = viewport(800.0, 600.0)?;

        let desktop_outcome = advance_shared_core(
            &mut desktop_core,
            FrameRequest::new(Duration::from_millis(25), &events, logical_viewport),
        );
        let headless_outcome = headless_core.advance_frame(FrameRequest::new(
            Duration::from_millis(25),
            &events,
            logical_viewport,
        ));
        let (FrameOutcome::Advanced(desktop_report), FrameOutcome::Advanced(headless_report)) =
            (desktop_outcome, headless_outcome)
        else {
            panic!("both bounded core frames should advance");
        };

        assert_eq!(desktop_report.timing(), headless_report.timing());
        assert_eq!(
            desktop_report.fixed_ticks_attempted(),
            headless_report.fixed_ticks_attempted()
        );
        assert_eq!(desktop_report.spawned(), headless_report.spawned());
        assert_eq!(desktop_report.despawned(), headless_report.despawned());
        assert!(desktop_report.failure().is_none());
        assert!(headless_report.failure().is_none());

        let desktop_frame = desktop_core
            .extracted_frame()
            .ok_or("desktop core should publish a frame")?;
        let headless_frame = headless_core
            .extracted_frame()
            .ok_or("headless core should publish a frame")?;
        assert_eq!(desktop_frame.background(), headless_frame.background());
        assert_eq!(desktop_frame.camera(), headless_frame.camera());
        let [desktop_circle] = desktop_frame.resolved_circles() else {
            panic!("desktop core should resolve one circle");
        };
        let [headless_circle] = headless_frame.resolved_circles() else {
            panic!("headless core should resolve one circle");
        };
        assert_eq!(desktop_circle.position(), headless_circle.position());
        assert_eq!(desktop_circle.radius(), headless_circle.radius());
        assert_eq!(desktop_circle.color(), headless_circle.color());
        assert_eq!(desktop_circle.layer(), headless_circle.layer());
        assert_eq!(
            desktop_circle.draw_order_depth(),
            headless_circle.draw_order_depth()
        );
        Ok(())
    }

    #[test]
    fn presentation_requires_the_report_snapshot_and_active_generation_to_match()
    -> Result<(), Box<dyn Error>> {
        let mut runner = parity_runner()?;
        let outcome = advance_shared_core(
            &mut runner,
            FrameRequest::new(Duration::ZERO, &[], viewport(800.0, 600.0)?),
        );
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded core frame should advance");
        };
        let active = runner.world_generation();
        let snapshot = runner
            .extracted_frame()
            .map(crate::ExtractedFrame::world_generation);
        let foreign = WorldGeneration::new(ApplicationId::from_raw(900), 1);

        assert_eq!(
            presentation_decision(&report, snapshot, active, false),
            PresentationDecision::PresentWorld
        );
        assert_eq!(
            presentation_decision(&report, Some(foreign), active, false),
            PresentationDecision::SkipStaleSnapshot
        );
        assert_eq!(
            presentation_decision(&report, snapshot, active, true),
            PresentationDecision::SkipOccludedWindow
        );
        Ok(())
    }

    #[test]
    fn application_exit_is_recorded_without_another_presentation() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
        let initial = application.register_world("desktop-exit", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::FrameUpdate, |mut commands: LogicCommands| {
            assert!(commands.request_exit().is_ok());
        });
        let mut runner = application.build_headless(initial)?;

        let outcome = advance_shared_core(
            &mut runner,
            FrameRequest::new(Duration::ZERO, &[], viewport(800.0, 600.0)?),
        );
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded exit frame should advance");
        };
        assert!(report.exit_requested());
        assert_eq!(report.extracted_generation(), None);
        assert_eq!(
            presentation_decision(
                &report,
                runner
                    .extracted_frame()
                    .map(crate::ExtractedFrame::world_generation),
                runner.world_generation(),
                false,
            ),
            PresentationDecision::ApplicationExit
        );

        let mut desktop = DesktopRunReport::default();
        assert_eq!(desktop.exit_reason(), DesktopExitReason::WindowClosed);
        desktop.record_application_exit(report);
        assert_eq!(
            desktop.exit_reason(),
            DesktopExitReason::ApplicationRequested
        );
        assert_eq!(desktop.drawn_frames(), 0);
        assert_eq!(desktop.skipped_frames(), 0);
        assert!(desktop.last_render_frame().is_none());
        assert!(
            desktop
                .last_logic_frame()
                .is_some_and(LogicFrameReport::exit_requested)
        );
        Ok(())
    }

    #[test]
    fn extraction_and_renderer_failures_choose_separate_paths() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
        let initial = application.register_world("invalid-after-frame", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::FrameUpdate, spawn_second_camera);
        let mut runner = application.build_headless(initial)?;

        let outcome = advance_shared_core(
            &mut runner,
            FrameRequest::new(Duration::ZERO, &[], viewport(800.0, 600.0)?),
        );
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("BeginFrame should succeed before extraction fails");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::Extraction(
                ExtractionError::MultipleActiveCameras
            ))
        ));
        assert_eq!(
            presentation_decision(
                &report,
                runner
                    .extracted_frame()
                    .map(crate::ExtractedFrame::world_generation),
                runner.world_generation(),
                false,
            ),
            PresentationDecision::DiagnosticsOnly
        );

        let renderer_validation = FrameComposerError::Frame(RendererFrameError::Surface(
            RendererSurfaceStatus::Validation,
        ));
        assert_eq!(
            renderer_failure_action(&renderer_validation),
            RendererFailureAction::Fail
        );
        assert_eq!(
            renderer_failure_action(&FrameComposerError::InvalidBackground),
            RendererFailureAction::Fail
        );
        Ok(())
    }

    #[test]
    fn surface_outcomes_follow_the_desktop_retry_policy() {
        assert_eq!(
            skipped_surface_action(RendererSurfaceStatus::Timeout),
            SkippedSurfaceAction::RetryLater
        );
        assert_eq!(
            skipped_surface_action(RendererSurfaceStatus::Occluded),
            SkippedSurfaceAction::WaitForWindowEvent
        );
        assert_eq!(
            skipped_surface_action(RendererSurfaceStatus::Outdated),
            SkippedSurfaceAction::ReconfigureThenRetry
        );
        assert_eq!(
            skipped_surface_action(RendererSurfaceStatus::Lost),
            SkippedSurfaceAction::Fail
        );
        assert_eq!(
            skipped_surface_action(RendererSurfaceStatus::Validation),
            SkippedSurfaceAction::Fail
        );

        let lost =
            FrameComposerError::Frame(RendererFrameError::Surface(RendererSurfaceStatus::Lost));
        assert_eq!(
            renderer_failure_action(&lost),
            RendererFailureAction::RecoverDeviceAndSurface {
                reset_wall_clock_on_success: true,
            }
        );
    }

    #[test]
    fn resize_and_render_skip_do_not_advance_fixed_state() -> Result<(), Box<dyn Error>> {
        let mut runner = parity_runner()?;
        let pressed = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let first = advance_shared_core(
            &mut runner,
            FrameRequest::new(Duration::from_millis(17), &pressed, viewport(800.0, 600.0)?),
        );
        assert!(matches!(first, FrameOutcome::Advanced(_)));
        let before_resize = runner
            .components::<Transform2d>()
            .next()
            .map(|(_, transform)| transform.translation())
            .ok_or("test World should contain a transform")?;

        assert!(!extent_is_occluded(1_200, 800));
        let resized = advance_shared_core(
            &mut runner,
            FrameRequest::new(Duration::ZERO, &[], viewport(1_200.0, 800.0)?),
        );
        assert!(matches!(resized, FrameOutcome::Advanced(_)));
        assert_eq!(
            skipped_surface_action(RendererSurfaceStatus::Timeout),
            SkippedSurfaceAction::RetryLater
        );

        let after_skip = runner
            .components::<Transform2d>()
            .next()
            .map(|(_, transform)| transform.translation())
            .ok_or("test World should contain a transform")?;
        assert_eq!(before_resize, after_skip);
        assert!(extent_is_occluded(0, 800));
        assert!(extent_is_occluded(1_200, 0));
        Ok(())
    }

    #[test]
    fn transition_and_recovery_clock_reset_policy_is_explicit() {
        let application = ApplicationId::from_raw(91);
        let old = WorldGeneration::new(application, 1);
        let new = WorldGeneration::new(application, 2);
        let target = WorldFactoryId::new(application, 0, 1);

        assert!(!transition_resets_wall_clock(&FrameTransition::None));
        assert!(transition_resets_wall_clock(&FrameTransition::Committed {
            old,
            new,
            target,
            warning: None,
        }));
        assert!(transition_resets_wall_clock(
            &FrameTransition::PreparationFailed {
                target,
                error: CandidateFailure::InvalidFactory,
            }
        ));

        let lost =
            FrameComposerError::Frame(RendererFrameError::Surface(RendererSurfaceStatus::Lost));
        assert_eq!(
            renderer_failure_action(&lost),
            RendererFailureAction::RecoverDeviceAndSurface {
                reset_wall_clock_on_success: true,
            }
        );
    }
}
