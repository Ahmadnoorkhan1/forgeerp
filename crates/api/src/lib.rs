//! HTTP API: server, routing, and request/response mapping.

pub mod app;
pub mod context;
pub mod middleware;
pub mod authz;

/// HTTP server bootstrap and runtime wiring (no framework specifics yet).
pub mod server {}

/// Routing tree and handler wiring (framework-specific code will live here later).
pub mod routes {}

/// Request/response DTOs and mapping to/from domain types.
pub mod mapping {}


