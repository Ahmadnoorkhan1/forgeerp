//! HTTP API: server, routing, and request/response mapping.

/// HTTP server bootstrap and runtime wiring (no framework specifics yet).
pub mod server {}

/// Routing tree and handler wiring (framework-specific code will live here later).
pub mod routes {}

/// Request/response DTOs and mapping to/from domain types.
pub mod mapping {}


