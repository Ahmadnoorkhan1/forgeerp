//! API bindings using Tauri's invoke system via JavaScript.

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::window;

use crate::types::{
    ConflictResolution, ConnectivityState, InventoryReadModel, QueuedCommand, SyncResult,
};

/// Helper to invoke Tauri commands from WASM.
/// 
/// This uses Tauri's JavaScript API through wasm-bindgen.
/// The Tauri API should be available via window.__TAURI__ in Tauri v2.
async fn invoke_tauri<T>(cmd: &str, args: JsValue) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    let window = window().ok_or_else(|| "No window object".to_string())?;
    
    // Get the Tauri API from window.__TAURI__
    let tauri_obj = js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI__"))
        .map_err(|e| format!("Failed to get __TAURI__: {:?}", e))?;
    
    let core = js_sys::Reflect::get(&tauri_obj, &JsValue::from_str("core"))
        .map_err(|e| format!("Failed to get core: {:?}", e))?;
    
    let invoke_fn = js_sys::Reflect::get(&core, &JsValue::from_str("invoke"))
        .map_err(|e| format!("Failed to get invoke: {:?}", e))?;
    
    // Call invoke(cmd, { args })
    let invoke_args_obj = js_sys::Object::new();
    js_sys::Reflect::set(&invoke_args_obj, &JsValue::from_str("args"), &args)
        .map_err(|e| format!("Failed to set args: {:?}", e))?;
    
    // Call the invoke function: invoke(cmd, { args })
    let invoke_function = js_sys::Function::from(invoke_fn);
    let promise = invoke_function
        .call2(&core, &JsValue::from_str(cmd), &invoke_args_obj)
        .map_err(|e| format!("Failed to call invoke: {:?}", e))?;
    
    let result = JsFuture::from(js_sys::Promise::from(promise))
        .await
        .map_err(|e| format!("Invoke failed: {:?}", e))?;
    
    // Deserialize the result
    serde_wasm_bindgen::from_value(result)
        .map_err(|e| format!("Failed to deserialize result: {:?}", e))
}

/// List all cached inventory items for a tenant.
pub async fn list_inventory_items(tenant_id: String) -> Result<Vec<InventoryReadModel>, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "tenant_id": tenant_id
    })).map_err(|e| format!("Failed to serialize args: {:?}", e))?;
    
    invoke_tauri("list_inventory_items", args).await
}

/// Adjust stock for an inventory item.
pub async fn adjust_stock(
    tenant_id: String,
    item_id: String,
    delta: i64,
) -> Result<(), String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "tenant_id": tenant_id,
        "item_id": item_id,
        "delta": delta
    })).map_err(|e| format!("Failed to serialize args: {:?}", e))?;
    
    invoke_tauri("adjust_stock", args).await
}

/// Trigger a full sync operation for the current tenant.
pub async fn sync_now(tenant_id: String) -> Result<SyncResult, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "tenant_id": tenant_id
    })).map_err(|e| format!("Failed to serialize args: {:?}", e))?;
    
    invoke_tauri("sync_now", args).await
}

/// Get the current connectivity state.
pub async fn get_connectivity_state() -> Result<ConnectivityState, String> {
    let args = JsValue::NULL;
    invoke_tauri("get_connectivity_state", args).await
}

/// List all pending commands for a tenant.
pub async fn list_pending_commands(tenant_id: String) -> Result<Vec<QueuedCommand>, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "tenant_id": tenant_id
    })).map_err(|e| format!("Failed to serialize args: {:?}", e))?;
    
    invoke_tauri("list_pending_commands", args).await
}

/// Resolve a conflict by applying the specified resolution strategy.
pub async fn resolve_conflict(
    conflict_aggregate_type: String,
    conflict_aggregate_id: String,
    resolution: ConflictResolution,
    tenant_id: String,
) -> Result<(), String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "conflict_aggregate_type": conflict_aggregate_type,
        "conflict_aggregate_id": conflict_aggregate_id,
        "resolution": resolution,
        "tenant_id": tenant_id
    })).map_err(|e| format!("Failed to serialize args: {:?}", e))?;
    
    invoke_tauri("resolve_conflict", args).await
}
