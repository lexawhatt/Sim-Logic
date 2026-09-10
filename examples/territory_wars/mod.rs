//! Frontier: an original offline territory-conquest example.
//!
//! Rules live in `simulation`, with no renderer or ECS dependency. `app`
//! connects them to Sim;Logic, and `view` uses managed screen primitives.

pub mod app;
mod drawing;
pub mod layout;
pub mod simulation;
mod view;

// Different consumers use the launch helper or the headless inspection types.
#[allow(unused_imports)]
pub use app::{Action, Session, build_application};

pub const DEFAULT_SEED: u64 = 0x51_4d_4c;
