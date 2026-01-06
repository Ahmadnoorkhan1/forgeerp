//! HTTP API: server, routing, and request/response mapping.

pub mod app;
pub mod context;
pub mod middleware;
pub mod authz;

// Note: the real “routing” and “mapping” modules live under `app/` for now:
// - `app/routes/*`: one file per REST area
// - `app/dto.rs`: request/response mapping helpers


