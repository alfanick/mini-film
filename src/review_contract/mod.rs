//! Public review JSON contracts shared by the application and its asset build.
//! This module intentionally depends on no application or rendering code, so
//! Cargo can generate browser schemas before compiling the embedded frontend.

mod common;
mod patch;
mod requests;
mod responses;

pub use common::*;
pub use patch::*;
pub use requests::*;
pub use responses::*;
