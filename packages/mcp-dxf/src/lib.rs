#![forbid(unsafe_code)]
//! Library target of `mcp-dxf` so integration tests can exercise the parser
//! and the edit mutator directly. The stdio server binary lives in `main.rs`.

pub mod create;
pub mod edit;
pub mod parser;
pub mod writer;
