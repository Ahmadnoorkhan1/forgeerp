use leptos::*;
use leptos_router::*;

use crate::{api, store::use_auth};
use crate::api::CreateItemRequest;


// ─── Inventory List ───────────────────────────────────────────────────────────

#[component]
pub fn InventoryPage() -> impl IntoView {
    let auth     = use_auth();
    let navigate = use_navigate();

    if !auth.is_authenticated() {
        navigate("/login", Default::default());
    }

    let token       = auth.token_str();
    let username    = auth.username();
    let tenant_name = auth.tenant_name();

    // ── Data ──────────────────────────────────────────────────────────────────
    // Add a refresh trigger that forces refetch when component remounts
    let refresh = create_rw_signal(0);
    let items = {
        let tok = token.clone();
        create_resource(
            move || refresh.get(),
            move |_| {
                let t = tok.clone();
                async move { api::list_inventory(&t).await.unwrap_or_default() }
            }
        )
    };

    let online = create_resource(|| (), |_| async move { api::health_check().await });

    // Force refetch when component mounts/remounts
    create_effect(move |_| {
        refresh.set(refresh.get() + 1);
    });

    // ── Handlers ─────────────────────────────────────────────────────────────
    let on_logout = {
        let auth     = auth.clone();
        let navigate = navigate.clone();
        move |_| { auth.clear(); navigate("/login", Default::default()); }
    };

    view! {
        <div class="forge-app">
            <div class="scanlines"></div>
            <div class="grid-bg"></div>

            // ── Nav ───────────────────────────────────────────────────────
            <nav class="top-nav">
                <div class="nav-left">
                    <span class="nav-logo-icon">"dashboard"</span>
                    <span class="nav-logo">"ForgeERP"</span>
                    <span class="nav-module">"Inventory"</span>
                </div>
                <div class="nav-center">
                    <span class="nav-tenant">
                        <span class="tenant-icon">"domain"</span>
                        {tenant_name}
                    </span>
                </div>
                <div class="nav-right">
                    {move || online.get().map(|up| if up {
                        view! { <span class="badge-online">"● Online"</span> }.into_view()
                    } else {
                        view! { <span class="badge-offline">"○ Offline"</span> }.into_view()
                    })}
                    <span class="nav-user">
                        <span class="user-icon">"person"</span>
                        {username}
                    </span>
                    <button class="btn-logout" on:click=on_logout>"Sign Out"</button>
                </div>
            </nav>

            // ── Content ───────────────────────────────────────────────────
            <main class="main-content">
                <div class="page-header">
                    <div>
                        <h1 class="page-title">"Inventory"</h1>
                        <p class="page-sub">"Manage your stock items"</p>
                    </div>
                </div>

                <div class="table-panel">
                    <div class="panel-bar">
                        <span class="panel-status">
                            <span class="dot-pulse"></span>
                            {move || items.get()
                                .map(|v| format!("{} items", v.len()))
                                .unwrap_or_else(|| "Loading…".into())}
                        </span>
                        <span class="panel-id">"Inventory"</span>
                        <div class="panel-actions">
                            <A href="/inventory/create" class="btn-action">"+ New Item"</A>
                        </div>
                    </div>

                    {move || match items.get() {
                        None => view! {
                            <div class="loading-state">
                                <div class="spin-ring"></div>
                                <span>"Loading inventory…"</span>
                            </div>
                        }.into_view(),
                        Some(rows) => view! {
                            <div class="table-scroll">
                                <table class="data-table">
                                    <thead>
                                        <tr>
                                            <th>"ID"</th>
                                            <th>"Name"</th>
                                            <th>"Quantity"</th>
                                            <th>"Actions"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {if rows.is_empty() {
                                            view! {
                                                <tr><td colspan="4" class="empty-cell">
                                                    "No inventory items yet. Create your first item to get started."
                                                </td></tr>
                                            }.into_view()
                                        } else {
                                            rows.into_iter().map(|item| {
                                                let id    = item.id.clone();
                                                let short = id[..8.min(id.len())].to_string();
                                                let qty   = item.quantity;
                                                view! {
                                                    <tr class="data-row">
                                                        <td class="mono-cell">
                                                            <span class="id-badge">{short}"…"</span>
                                                        </td>
                                                        <td class="name-cell">{item.name}</td>
                                                        <td class="qty-cell">
                                                            <span class=move || {
                                                                if qty == 0     { "qty-badge out" }
                                                                else if qty < 5 { "qty-badge low" }
                                                                else            { "qty-badge ok"  }
                                                            }>{qty}</span>
                                                        </td>
                                                        <td class="action-cell">
                                                            <A href=format!("/adjust/{}", item.id)
                                                               class="btn-action">"ADJUST"</A>
                                                        </td>
                                                    </tr>
                                                }
                                            }).collect_view()
                                        }}
                                    </tbody>
                                </table>
                            </div>
                        }.into_view(),
                    }}
                </div>
            </main>
        </div>
    }
}

// ─── Adjust Stock ─────────────────────────────────────────────────────────────

#[component]
pub fn AdjustStockPage() -> impl IntoView {
    let auth     = use_auth();
    let navigate = use_navigate();

    if !auth.is_authenticated() {
        navigate("/login", Default::default());
    }

    let params  = use_params_map();
    let item_id = move || params.get().get("id").cloned().unwrap_or_default();
    let token   = auth.token_str();

    let delta      = create_rw_signal(0i64);
    let submitting = create_rw_signal(false);
    let error      = create_rw_signal(Option::<String>::None);

    // Pre-clone before closures consume ownership
    let auth_for_submit = auth.clone();
    let nav_for_submit  = navigate.clone();
    let auth_for_logout = auth.clone();
    let nav_for_logout  = navigate.clone();

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if submitting.get() { return; }
        submitting.set(true);
        error.set(None);

        let iid  = item_id();
        let tok  = token.clone();
        let d    = delta.get();
        // Clone again for the async block inside this sync closure
        let nav  = nav_for_submit.clone();
        let auth = auth_for_submit.clone();

        spawn_local(async move {
            match api::adjust_stock(&iid, d, &tok).await {
                Ok(()) => nav("/inventory", Default::default()),
                Err(e) if e == "SESSION_EXPIRED" => {
                    auth.clear();
                    nav("/login", Default::default());
                }
                Err(e) => {
                    error.set(Some(e));
                    submitting.set(false);
                }
            }
        });
    };

    let on_logout = move |_| {
        auth_for_logout.clear();
        nav_for_logout("/login", Default::default());
    };

    view! {
        <div class="forge-app">
            <div class="scanlines"></div>
            <div class="grid-bg"></div>

            <nav class="top-nav">
                <div class="nav-left">
                    <span class="nav-logo-icon">"dashboard"</span>
                    <span class="nav-logo">"ForgeERP"</span>
                    <A href="/inventory" class="nav-back">"← Back to Inventory"</A>
                </div>
                <div class="nav-right">
                    <button class="btn-logout" on:click=on_logout>"Sign Out"</button>
                </div>
            </nav>

            <main class="main-content">
                <div class="page-header">
                    <div>
                        <h1 class="page-title">"Adjust Stock"</h1>
                        <p class="page-sub">
                            {move || format!("Item: {}…", &item_id()[..8.min(item_id().len())])}
                        </p>
                    </div>
                </div>

                <div class="form-panel">
                    <div class="bracket tl"></div>
                    <div class="bracket tr"></div>
                    <div class="bracket bl"></div>
                    <div class="bracket br"></div>

                    <form on:submit=on_submit class="adjust-form">
                        <div class="field">
                            <label class="field-lbl">
                                "Quantity delta (positive = add, negative = subtract)"
                            </label>
                            <div class="field-inner">
                                <span class="field-pfx">"add"</span>
                                <input
                                    type="number"
                                    class="field-input"
                                    prop:value=move || delta.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(n) = event_target_value(&ev).parse::<i64>() {
                                            delta.set(n);
                                        }
                                    }
                                />
                            </div>
                        </div>

                        {move || error.get().map(|msg| view! {
                            <div class="err-bar">
                                <span class="err-icon">"⚠"</span>
                                {msg}
                            </div>
                        })}

                        <div class="form-actions">
                            <A href="/inventory" class="btn-secondary">"Cancel"</A>
                            <button
                                type="submit"
                                class="btn-primary"
                                disabled=move || submitting.get()
                            >
                                {move || if submitting.get() { "Applying…" } else { "Apply" }}
                            </button>
                        </div>
                    </form>
                </div>
            </main>
        </div>
    }
}

#[component]
pub fn CreateItemPage() -> impl IntoView {
    let auth     = use_auth();
    let navigate = use_navigate();

    if !auth.is_authenticated() {
        navigate("/login", Default::default());
    }

    let token      = auth.token_str();
    let tenant_id  = auth.tenant_id();
    let item_id    = create_rw_signal(String::new());
    let name       = create_rw_signal(String::new());
    let submitting = create_rw_signal(false);
    let error      = create_rw_signal(Option::<String>::None);

    // Pre-clone before closures consume ownership
    let auth_for_submit = auth.clone();
    let nav_for_submit  = navigate.clone();
    let auth_for_logout = auth.clone();
    let nav_for_logout  = navigate.clone();

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if submitting.get() { return; }
        submitting.set(true);
        error.set(None);

        let iid = item_id.get();
        let nm  = name.get();
        let tid = tenant_id.clone();
        let tok = token.clone();

        // Clone again for the async block inside this sync closure
        let nav  = nav_for_submit.clone();
        let auth = auth_for_submit.clone();

        spawn_local(async move {
            let req = CreateItemRequest {
                tenant_id: tid,
                item_id:   iid,
                name:      nm,
            };

            match api::create_item(req, &tok).await {
                Ok(()) => nav("/inventory", Default::default()),
                Err(e) if e == "SESSION_EXPIRED" => {
                    auth.clear();
                    nav("/login", Default::default());
                }
                Err(e) => {
                    error.set(Some(e));
                    submitting.set(false);
                }
            }
        });
    };

    let on_logout = move |_| {
        auth_for_logout.clear();
        nav_for_logout("/login", Default::default());
    };

    view! {
        <div class="forge-app">
            <div class="scanlines"></div>
            <div class="grid-bg"></div>

            <nav class="top-nav">
                <div class="nav-left">
                    <span class="nav-logo-icon">"dashboard"</span>
                    <span class="nav-logo">"ForgeERP"</span>
                    <A href="/inventory" class="nav-back">"← Back to Inventory"</A>
                </div>
                <div class="nav-right">
                    <button class="btn-logout" on:click=on_logout>"Sign Out"</button>
                </div>
            </nav>

            <main class="main-content">
                <div class="page-header">
                    <div>
                        <h1 class="page-title">"Create Item"</h1>
                        <p class="page-sub">"Add a new item to your inventory"</p>
                    </div>
                </div>

                <div class="form-panel">
                    <div class="bracket tl"></div>
                    <div class="bracket tr"></div>
                    <div class="bracket bl"></div>
                    <div class="bracket br"></div>

                    <form on:submit=on_submit class="create-item-form">
                        <div class="field">
                            <label class="field-lbl">
                                "Item ID"
                            </label>
                            <div class="field-inner">
                                <span class="field-pfx">"tag"</span>
                                <input
                                    type="text"
                                    class="field-input"
                                    placeholder="e.g., b, item-001"
                                    prop:value=move || item_id.get()
                                    on:input=move |ev| {
                                        item_id.set(event_target_value(&ev));
                                    }
                                    required
                                />
                            </div>
                        </div>

                        <div class="field">
                            <label class="field-lbl">
                                "Name"
                            </label>
                            <div class="field-inner">
                                <span class="field-pfx">"label"</span>
                                <input
                                    type="text"
                                    class="field-input"
                                    placeholder="e.g., Widget B, Product Name"
                                    prop:value=move || name.get()
                                    on:input=move |ev| {
                                        name.set(event_target_value(&ev));
                                    }
                                    required
                                />
                            </div>
                        </div>

                        {move || error.get().map(|msg| view! {
                            <div class="err-bar">
                                <span class="err-icon">"⚠"</span>
                                {msg}
                            </div>
                        })}

                        <div class="form-actions">
                            <A href="/inventory" class="btn-secondary">"Cancel"</A>
                            <button
                                type="submit"
                                class="btn-primary"
                                disabled=move || submitting.get() || item_id.get().is_empty() || name.get().is_empty()
                            >
                                {move || if submitting.get() { "Creating…" } else { "Create Item" }}
                            </button>
                        </div>
                    </form>
                </div>
            </main>
        </div>
    }
}