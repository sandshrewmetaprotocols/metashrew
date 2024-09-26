#[macro_use]
extern crate log;

pub mod proto;
pub mod runtime;
pub mod wasmi;

pub use runtime::*;
