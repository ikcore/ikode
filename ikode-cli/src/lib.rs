//! iKode library surface — exposed so the binary (`src/main.rs`) and the
//! integration tests under `tests/` share one implementation.

pub mod harness;
mod ikignore;
pub mod index;
pub mod lang;
pub mod mcp;
pub mod models;
pub mod prompts;
pub mod settings;
pub mod skills;
pub mod tools;
pub mod util;
pub mod web;
