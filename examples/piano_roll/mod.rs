//! Editable synthetic piano roll sharing one score and source between views.

pub mod app;
pub mod control;
mod labels;
pub mod layout;
pub mod model3d;
pub mod music;
mod view;

pub use app::{Action, Session, View, build_application};
