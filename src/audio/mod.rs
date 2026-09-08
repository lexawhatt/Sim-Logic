//! Optional application-owned output for one streaming mono PCM source.
//!
//! The `audio` feature uses Rodio for device output and conversion. Keep the
//! output guard outside World factories and renderer ownership. Opening or
//! playing audio is an immediate external operation, not a transactional ECS
//! command: a later failed stage cannot undo sound already submitted.
//!
//! This module provides no note scheduler, codec, voice mixer, World lifetime
//! policy, or automatic device recovery. Applications own their source and
//! controls. Offline source tests need no output device.

mod output;

pub use output::{
    AudioOutput, AudioOutputConfig, AudioOutputError, AudioOutputInfo, AudioOutputStatus,
    AudioPlayback,
};
