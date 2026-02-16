use leptos::*;
use leptos_router::*;

use crate::{
    api::{self, LoginRequest},
    store::use_auth,
};

#[component]
pub fn LoginPage() -> impl IntoView {
    let auth     = use_auth();
    let navigate = use_navigate();

    if auth.is_authenticated() {
        navigate("/inventory", Default::default());
    }

    let username = create_rw_signal(String::new());
    let password = create_rw_signal(String::new());
    let error    = create_rw_signal(Option::<String>::None);
    let loading  = create_rw_signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let u = username.get();
        let p = password.get();
        if u.is_empty() || p.is_empty() {
            error.set(Some("Username and password are required".into()));
            return;
        }
        loading.set(true);
        error.set(None);
        let auth     = auth.clone();
        let navigate = navigate.clone();
        spawn_local(async move {
            match api::login(LoginRequest { username: u, password: p }).await {
                Ok(resp) => {
                    let (token, user, tenant) = resp.into();
                    auth.set(token, user, tenant);
                    navigate("/inventory", Default::default());
                }
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    view! {
        <div class="auth-page">
            <div class="scanlines"></div>
            <div class="grid-bg"></div>
            <div class="auth-wrap">
                <div class="auth-card">
                    <div class="bracket tl"></div>
                    <div class="bracket tr"></div>
                    <div class="bracket bl"></div>
                    <div class="bracket br"></div>

                    <header class="auth-head">
                        <div class="auth-logo">
                            <span class="logo-icon">"dashboard"</span>
                            <span class="logo-text">"ForgeERP"</span>
                        </div>
                        <p class="auth-tagline">"Enterprise Resource Planning"</p>
                        <div class="auth-status">
                            <span class="dot-pulse"></span>
                            "Sign in to your account"
                        </div>
                    </header>

                    <form on:submit=on_submit class="auth-form">
                        <div class="field">
                            <label class="field-lbl">"Username"</label>
                            <div class="field-inner">
                                <span class="field-pfx">"person"</span>
                                <input
                                    type="text"
                                    class="field-input"
                                    placeholder="enter username..."
                                    autocomplete="username"
                                    prop:value=username
                                    on:input=move |e| username.set(event_target_value(&e))
                                />
                            </div>
                        </div>

                        <div class="field">
                            <label class="field-lbl">"Password"</label>
                            <div class="field-inner">
                                <span class="field-pfx">"lock"</span>
                                <input
                                    type="password"
                                    class="field-input"
                                    placeholder="enter password..."
                                    autocomplete="current-password"
                                    prop:value=password
                                    on:input=move |e| password.set(event_target_value(&e))
                                />
                            </div>
                        </div>

                        {move || error.get().map(|msg| view! {
                            <div class="err-bar">
                                <span class="err-icon">"⚠"</span>
                                {msg}
                            </div>
                        })}

                        <button type="submit" class="btn-primary" disabled=move || loading.get()>
                            {move || if loading.get() { "Signing in…" } else { "Sign In" }}
                        </button>
                    </form>

                    <footer class="auth-foot">
                        "Don't have an account? "
                        <A href="/register" class="link">"Create account"</A>
                    </footer>
                </div>
            </div>
        </div>
    }
}
