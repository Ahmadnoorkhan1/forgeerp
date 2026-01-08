//! Leptos application with routing.

use leptos::*;
use leptos_router::*;

use crate::frontend::api;

/// Main application component.
#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes>
                <Route path="/" view=InventoryListPage/>
                <Route path="/adjust/:id" view=AdjustStockPage/>
            </Routes>
        </Router>
    }
}

/// Inventory list page component.
#[component]
fn InventoryListPage() -> impl IntoView {
    let tenant_id = "00000000-0000-0000-0000-000000000000".to_string(); // TODO: Get from auth context
    let tenant_id_for_resource = tenant_id.clone();

    let inventory_items = create_resource(
        move || tenant_id_for_resource.clone(),
        |tenant_id| async move {
            api::list_inventory_items(tenant_id).await.unwrap_or_default()
        },
    );

    let connectivity_state = create_resource(
        || (),
        |_| async move {
            api::get_connectivity_state().await.unwrap_or(crate::ConnectivityState::Offline)
        },
    );

    // Listen to sync events (TODO: implement event listeners)
    let _sync_completed = create_signal(false);
    let _sync_conflict = create_signal(false);
    let _sync_failed = create_signal(false);

    view! {
        <div class="app">
            <header>
                <h1>"ForgeERP Desktop"</h1>
                <div class="connectivity">
                    {move || {
                        connectivity_state.get().map(|state| {
                            match state {
                                crate::ConnectivityState::Online => {
                                    view! { <span class="status online">"Online"</span> }
                                }
                                crate::ConnectivityState::Offline => {
                                    view! { <span class="status offline">"Offline"</span> }
                                }
                            }
                        })
                    }}
                </div>
            </header>

            <main>
                <div class="inventory-list">
                    <h2>"Inventory Items"</h2>
                    
                    {move || {
                        inventory_items.get().map(|items| {
                            view! {
                                {if items.is_empty() {
                                    view! {
                                        <p>"No inventory items found. Items will appear here after syncing."</p>
                                    }.into_view()
                                } else {
                                    view! {
                                        <table>
                                            <thead>
                                                <tr>
                                                    <th>"ID"</th>
                                                    <th>"Name"</th>
                                                    <th>"Quantity"</th>
                                                    <th>"Actions"</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                {items.iter().map(|item| {
                                                    view! {
                                                        <tr>
                                                            <td>{&item.id}</td>
                                                            <td>{&item.name}</td>
                                                            <td>{item.quantity}</td>
                                                            <td>
                                                                <A href=format!("/adjust/{}", item.id)>
                                                                    "Adjust Stock"
                                                                </A>
                                                            </td>
                                                        </tr>
                                                    }
                                                }).collect_view()}
                                            </tbody>
                                        </table>
                                    }.into_view()
                                }}
                            }
                        })
                    }}

                    <div class="actions">
                        <button
                            on:click=move |_| {
                                let tenant_id = tenant_id.clone();
                                spawn_local(async move {
                                    if let Err(e) = api::sync_now(tenant_id).await {
                                        if let Some(w) = web_sys::window() {
                                            let _ = w.alert_with_message(&format!("Sync failed: {}", e));
                                        }
                                    }
                                });
                            }
                        >
                            "Sync Now"
                        </button>
                    </div>
                </div>
            </main>
        </div>
    }
}

/// Adjust stock page component.
#[component]
fn AdjustStockPage() -> impl IntoView {
    let params = use_params_map();
    let item_id = move || {
        params.get().get("id").cloned().unwrap_or_default()
    };

    let tenant_id = "00000000-0000-0000-0000-000000000000".to_string(); // TODO: Get from auth context
    let delta = create_rw_signal(0i64);
    let is_submitting = create_rw_signal(false);

    let submit = move |_| {
        if is_submitting.get() {
            return;
        }

        is_submitting.set(true);
        let item_id = item_id();
        let tenant_id = tenant_id.clone();
        let delta_value = delta.get();

        spawn_local(async move {
            match api::adjust_stock(tenant_id, item_id, delta_value).await {
                Ok(()) => {
                    // Navigate back to inventory list
                    leptos_router::use_navigate()("/", Default::default());
                }
                Err(e) => {
                    if let Some(w) = web_sys::window() {
                        let _ = w.alert_with_message(&format!("Failed to adjust stock: {}", e));
                    }
                }
            }
            is_submitting.set(false);
        });
    };

    view! {
        <div class="app">
            <header>
                <h1>"Adjust Stock"</h1>
                <A href="/">"Back to Inventory"</A>
            </header>

            <main>
                <div class="adjust-stock">
                    <h2>{move || format!("Adjust Stock for Item: {}", item_id())}</h2>
                    
                    <form on:submit=move |ev| {
                        ev.prevent_default();
                        submit(ev);
                    }>
                        <div class="form-group">
                            <label for="delta">"Delta (positive to add, negative to subtract):"</label>
                            <input
                                type="number"
                                id="delta"
                                prop:value=move || delta.get().to_string()
                                on:input=move |ev| {
                                    let value = event_target_value(&ev);
                                    if let Ok(num) = value.parse::<i64>() {
                                        delta.set(num);
                                    }
                                }
                            />
                        </div>

                        <div class="form-actions">
                            <button type="submit" disabled=move || is_submitting.get()>
                                {move || if is_submitting.get() { "Submitting..." } else { "Submit" }}
                            </button>
                            <A href="/">
                                <button type="button">"Cancel"</button>
                            </A>
                        </div>
                    </form>
                </div>
            </main>
        </div>
    }
}

