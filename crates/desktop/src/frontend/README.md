# Leptos Frontend for Tauri Desktop

This frontend is built with Leptos using Client-Side Rendering (CSR) for the Tauri desktop app.

## Building the Frontend

The frontend needs to be compiled to WebAssembly. You'll need:

1. **Install wasm-pack**:
   ```bash
   cargo install wasm-pack
   ```

2. **Build the frontend**:
   ```bash
   cd crates/desktop
   wasm-pack build --target web --out-dir pkg src/frontend
   ```

3. **Configure Tauri** to serve the frontend:
   - Copy `index.html` and the `pkg/` directory to your Tauri dist directory
   - Or configure Tauri to load from the build directory

## Development

For development, you can use a simple HTTP server to serve the frontend:

```bash
# After building with wasm-pack
cd crates/desktop
python3 -m http.server 8000
# Or use any static file server
```

Then configure Tauri to load from `http://localhost:8000` during development.

## Structure

- `main.rs` - WASM entry point that mounts the Leptos app
- `app.rs` - Main application component with routing
- `api.rs` - Tauri invoke bindings for backend communication
- `index.html` - HTML entry point

## Routes

- `/` - Inventory list page
- `/adjust/:id` - Adjust stock form for a specific item

## Tauri Events

The frontend can listen to Tauri events for sync updates:

- `sync:completed` - Emitted when sync completes successfully
- `sync:conflict` - Emitted when conflicts are detected
- `sync:failed` - Emitted when sync fails

