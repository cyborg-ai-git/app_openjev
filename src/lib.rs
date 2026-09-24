//! Typed TypeSafe System One client, also used by the `openjev` terminal UI.
pub mod entity_migra;
pub mod utility;
pub use entity_migra::*;
pub use utility::*;

// Stable module aliases for existing library callers.
pub use entity_migra::em_openjev_decision as model;
#[cfg(feature = "local")]
pub use utility::u_openjev_local as local;
pub use utility::{
    u_openjev_app as app, u_openjev_client as client, u_openjev_metrics as metrics,
    u_openjev_tui as ui,
};
