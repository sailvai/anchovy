//! The meeting prompt: when Zoom, Teams, or a browser starts using the
//! microphone and Anchovy is not recording, ask whether to record.
//!
//! `detector` holds the logic and is tested with fake process snapshots and
//! a fake clock; `mac` is the thin layer over Core Audio's process list and
//! macOS notifications; `commands` connects them to the interface.

pub mod commands;
pub mod detector;
pub mod mac;
