//! Explicit, headless interaction helpers for application-owned UI.
//!
//! These helpers neither draw widgets nor consume runtime input. Route UI and
//! application actions in one ordered owner; FrameUpdate cannot undo gameplay
//! already applied by FixedUpdate. World/entity lifetime and eligible hit
//! selection remain explicit application decisions.

mod pointer_button;
pub use pointer_button::{
    PointerButton, PointerButtonCancellation, PointerButtonEvent, PointerButtonOutcome,
};
