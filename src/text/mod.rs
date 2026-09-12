//! Optional single-line screen labels using Sim;Engine's font implementation.
//!
//! Fonts and validated label values can be created and inspected without a
//! window or GPU. The `text` feature includes Engine's GPU atlas types because
//! label settings also configure desktop limits. The separate `fonts` feature
//! exposes Engine's CPU font primitives without GPU dependencies.
//! Font discovery, fallback, wrapping, text input, and widgets are not included.
//!
//! ```no_run
//! use sim_logic::prelude::*;
//!
//! # fn main() -> LogicResult {
//! #[derive(Clone, Copy, PartialEq, Eq, Hash)]
//! enum Action {}
//! let mut app = Application::<Action>::new(AppConfig::default())?;
//! let font = app.register_font(
//!     std::fs::read("assets/your-font.ttf")?,
//!     TextSettings::new(24.0)?,
//! )?;
//! let mut label = ScreenTextVisual::new(
//!     font, "Привет, мир!", LogicalScreenPosition::new(400.0, 64.0),
//! )?;
//! label.set_alignment(TextAlignment::Center)?;
//! label.set_text("Ready")?;
//! # Ok(())
//! # }
//! ```
//!
//! Before spawning labels, opt into nonzero text source/byte/glyph allowances
//! on [`crate::render::RenderLimits`]. Desktop drawing also needs enough frame
//! texture allowance for every referenced font atlas. Preparing a value alone
//! does not enlarge these limits or draw anything.

mod error;
mod font;
mod session;
mod settings;
mod visual;

pub use error::TextError;
pub use font::TextFont;
pub(crate) use font::TextRegistry;
pub use session::TextPreparationSession;
pub use settings::{TextLimits, TextSettings};
pub use visual::{ScreenTextVisual, TextAlignment, TextMetrics};

#[cfg(test)]
mod tests;
