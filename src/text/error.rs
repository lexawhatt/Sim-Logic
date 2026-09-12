use std::{collections::TryReserveError, error::Error, fmt};

use sim_engine::{FontError, LogicalScreenPosition};

use crate::screen::ScreenVisualError;

/// Rejected font registration or label change; prior published values survive.
///
/// Font parsing, shaping, and owned vector reservation happen before a new
/// label or registration is installed. As elsewhere in Rust, shared-handle
/// control-block allocation and dependency internals are not an OOM sandbox.
#[derive(Debug)]
#[non_exhaustive]
pub enum TextError {
    /// Engine rejected trusted font bytes, single-line input, or font geometry.
    Font(FontError),
    /// The application has reached its fixed registration-count limit.
    FontLimitExceeded {
        /// Maximum distinct font registrations.
        limit: usize,
    },
    /// Source Vec capacity would exceed the application's total font budget.
    FontByteLimitExceeded {
        /// Aggregate permitted font source capacity in bytes.
        limit: usize,
        /// Capacity already retained by successful registrations.
        retained: usize,
        /// Capacity of the rejected incoming font source.
        incoming: usize,
    },
    /// Shaped glyphs cannot fit the configured desktop run even before rasterization.
    RunBudgetExceeded {
        /// Number of shaped glyphs, conservatively including whitespace.
        glyphs: usize,
        /// Configured maximum glyph count for one retained run.
        max_glyphs: usize,
        /// Conservative retained per-glyph storage bytes required by this line.
        required_bytes: usize,
        /// Configured per-run retained-byte ceiling.
        max_retained_bytes: usize,
    },
    /// Position, tint, or draw-order depth is invalid.
    Screen(ScreenVisualError),
    /// Aligned baseline or typographic bounds overflow or lose nonzero extent.
    InvalidBounds {
        /// Requested logical baseline anchor before alignment.
        position: LogicalScreenPosition,
    },
    /// Owned string or registry metadata could not be reserved before publication.
    AllocationFailed {
        /// Original fallible allocator error.
        source: TryReserveError,
    },
}

impl fmt::Display for TextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Font(error) => write!(formatter, "text font: {error}"),
            Self::FontLimitExceeded { limit } => {
                write!(
                    formatter,
                    "text font registry reached its {limit}-font limit"
                )
            }
            Self::FontByteLimitExceeded {
                limit,
                retained,
                incoming,
            } => write!(
                formatter,
                "font source capacity {retained} plus {incoming} bytes exceeds limit {limit}"
            ),
            Self::RunBudgetExceeded {
                glyphs,
                max_glyphs,
                required_bytes,
                max_retained_bytes,
            } => write!(
                formatter,
                "text run needs {glyphs} glyphs and {required_bytes} retained bytes; limits are {max_glyphs} and {max_retained_bytes}"
            ),
            Self::Screen(error) => write!(formatter, "screen text: {error}"),
            Self::InvalidBounds { position } => write!(
                formatter,
                "text baseline and typographic bounds are not representable at {position:?}"
            ),
            Self::AllocationFailed { source } => {
                write!(formatter, "text allocation failed: {source}")
            }
        }
    }
}

impl Error for TextError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Font(error) => Some(error),
            Self::Screen(error) => Some(error),
            Self::AllocationFailed { source } => Some(source),
            _ => None,
        }
    }
}

impl From<FontError> for TextError {
    fn from(error: FontError) -> Self {
        Self::Font(error)
    }
}

impl From<ScreenVisualError> for TextError {
    fn from(error: ScreenVisualError) -> Self {
        Self::Screen(error)
    }
}
