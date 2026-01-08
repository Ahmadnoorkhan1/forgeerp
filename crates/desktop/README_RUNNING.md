# Running the ForgeERP Desktop App

## Prerequisites

1. **Build the WASM frontend** (if not already done):
   ```bash
   cd crates/desktop
   wasm-pack build --target web --out-dir pkg -- --features tauri
   ```

2. **Copy frontend files** (if not already done):
   ```bash
   mkdir -p frontend
   cp src/frontend/index.html frontend/
   cp -r pkg frontend/
   ```

## Running the App

### Development Mode

Run the Tauri app in development mode:

```bash
cd crates/desktop
cargo tauri dev --features tauri
```

This will:
- Build the Rust backend
- Launch the Tauri window with the frontend
- Enable hot-reload for development

### Production Build

Build the app for production:

```bash
cd crates/desktop
cargo tauri build --features tauri
```

This creates a distributable app in `target/release/bundle/`.

## Environment Variables

You can configure the app with environment variables:

- `FORGEERP_API_URL`: API server URL (default: `http://localhost:8080`)
- `FORGEERP_AUTH_TOKEN`: Optional authentication token

Example:
```bash
FORGEERP_API_URL=http://localhost:8080 FORGEERP_AUTH_TOKEN=your-token cargo tauri dev --features tauri
```

## Troubleshooting

1. **If Tauri commands are not found**: Install Tauri CLI:
   ```bash
   cargo install tauri-cli
   ```

2. **If frontend doesn't load**: Make sure `frontend/index.html` and `frontend/pkg/` exist and are up to date.

3. **If WASM fails to load**: Check browser console (DevTools) for errors. Make sure the `pkg/` directory contains all WASM files.

