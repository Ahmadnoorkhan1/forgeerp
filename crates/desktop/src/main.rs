//! Tauri application entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "tauri")]
use forgeerp_desktop::commands::*;
#[cfg(feature = "tauri")]
use std::sync::Arc;

#[cfg(feature = "tauri")]
#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Get API URL from environment or use default
    let api_url = std::env::var("FORGEERP_API_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    // Get auth token from environment (optional)
    let app_state = if let Ok(token) = std::env::var("FORGEERP_AUTH_TOKEN") {
        tracing::info!("Initializing AppState with authentication token");
        AppState::with_token(api_url, token)
    } else {
        tracing::info!("Initializing AppState without authentication token");
        AppState::new(api_url)
    };

    let app_state_arc = Arc::new(app_state);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state_arc.clone())
        .invoke_handler(tauri::generate_handler![
            list_inventory_items,
            adjust_stock,
            sync_now,
            get_connectivity_state,
            list_pending_commands,
            resolve_conflict,
        ])
        .setup(move |app| {
            // Start background sync worker
            let app_handle = app.handle();
            let worker = forgeerp_desktop::sync_worker::SyncWorker::new(
                app_handle.clone(),
                app_state_arc.clone(),
            );

            // Start the worker
            let _worker_handle = worker.start();

            // Note: Worker will automatically stop when shutdown is notified
            // The worker task will complete when the app closes

            tracing::info!("Background sync worker started");

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(not(feature = "tauri"))]
fn main() {
    eprintln!("This binary requires the 'tauri' feature to be enabled.");
    eprintln!("Build with: cargo build --features tauri");
    std::process::exit(1);
}

