//! caps-gui: a native editor for the keyd Caps Lock layer.
//!
//! The core is framework-neutral — `keyd` holds the config model, the macro builder, the action
//! catalog and the system glue; the Rune UI lives behind the `ui` feature.

pub mod cli;
pub mod keyd;
#[cfg(feature = "ui")]
pub mod ui;

pub use keyd::config::{Binding, Tap};
