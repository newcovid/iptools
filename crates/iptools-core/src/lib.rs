//! Platform-independent domain model and application state machine.

mod config;
mod effect;
mod input;
mod model;

pub use config::*;
pub use effect::*;
pub use input::*;
pub use model::*;

/// Version of the cross-platform application protocol.
pub const ARCHITECTURE_VERSION: u8 = 2;
