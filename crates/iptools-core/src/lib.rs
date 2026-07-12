//! Platform-independent domain model and application state machine.

mod config;
mod effect;
mod input;
pub mod link_quality;
mod model;

pub use config::*;
pub use effect::*;
pub use input::*;
pub use model::*;

/// Version of the cross-platform application protocol.
pub const ARCHITECTURE_VERSION: u8 = 3;
