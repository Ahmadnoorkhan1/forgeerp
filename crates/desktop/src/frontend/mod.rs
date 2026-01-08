//! Leptos frontend for Tauri desktop app.

pub mod api;
pub mod app;

/// WASM entry point for the frontend.
/// This is called automatically when the WASM module loads.
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn main() {
    // Initialize console error panic hook for better error messages
    console_error_panic_hook::set_once();

    // Mount the Leptos app to the body
    leptos::mount_to_body(app::App);
}

