use leptos::*;
use leptos_router::*;

use crate::{api::{self, CreateItemRequest}, store::use_auth};

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
                    <span class="nav-logo-icon">"◈"</span>
                    <span class="nav-logo">"FORGE/ERP"</span>
                    <A href="/inventory" class="nav-back">"‹ INVENTORY"</A>
                </div>
                <div class="nav-right">
                    <button class="btn-logout" on:click=on_logout>"[ LOGOUT ]"</button>
                </div>
            </nav>

            <main class="main-content">
                <div class="page-header">
                    <div>
                        <h1 class="page-title">"CREATE ITEM"</h1>
                        <p class="page-sub">"// ADD NEW INVENTORY ITEM //"</p>
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
                                "› ITEM ID"
                            </label>
                            <div class="field-inner">
                                <span class="field-pfx">"#"</span>
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
                                "› NAME"
                            </label>
                            <div class="field-inner">
                                <span class="field-pfx">"∴"</span>
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
                            <A href="/inventory" class="btn-secondary">"[ CANCEL ]"</A>
                            <button
                                type="submit"
                                class="btn-primary"
                                disabled=move || submitting.get() || item_id.get().is_empty() || name.get().is_empty()
                            >
                                {move || if submitting.get() { "CREATING…" } else { "[ CREATE ITEM ]" }}
                            </button>
                        </div>
                    </form>
                </div>
            </main>
        </div>
    }
}
