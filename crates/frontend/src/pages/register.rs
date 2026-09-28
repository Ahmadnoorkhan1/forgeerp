use leptos::*;
use leptos_router::*;

use crate::{
    api::{self, RegisterRequest},
    store::use_auth,
};

#[component]
pub fn RegisterPage() -> impl IntoView {
    let auth     = use_auth();
    let navigate = use_navigate();

    if auth.is_authenticated() {
        navigate("/inventory", Default::default());
    }

    let username     = create_rw_signal(String::new());
    let email        = create_rw_signal(String::new());
    let password     = create_rw_signal(String::new());
    let invite_token = create_rw_signal(String::new());
    let show_invite  = create_rw_signal(false);
    let error        = create_rw_signal(Option::<String>::None);
    let loading      = create_rw_signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let u      = username.get();
        let e_val  = email.get();
        let p      = password.get();
        let invite = invite_token.get();

        if u.is_empty() || e_val.is_empty() || p.is_empty() {
            error.set(Some("All fields are required".into()));
            return;
        }
        if !e_val.contains('@') {
            error.set(Some("Enter a valid email address".into()));
            return;
        }
        if p.len() < 6 {
            error.set(Some("Password must be at least 6 characters".into()));
            return;
        }

        loading.set(true);
        error.set(None);
        let auth     = auth.clone();
        let navigate = navigate.clone();

        spawn_local(async move {
            let req = RegisterRequest {
                username:     u,
                email:        e_val,
                password:     p,
                invite_token: if invite.is_empty() { None } else { Some(invite) },
            };
            match api::register(req).await {
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
                        <p class="auth-tagline">"Create your account"</p>
                        <div class="auth-status">
                            <span class="dot-pulse"></span>
                            "Registration is quick and free"
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
                                    placeholder="choose username..."
                                    prop:value=username
                                    on:input=move |e| username.set(event_target_value(&e))
                                />
                            </div>
                        </div>

                        <div class="field">
                            <label class="field-lbl">"Email"</label>
                            <div class="field-inner">
                                <span class="field-pfx">"mail"</span>
                                <input
                                    type="email"
                                    class="field-input"
                                    placeholder="you@example.com"
                                    prop:value=email
                                    on:input=move |e| email.set(event_target_value(&e))
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
                                    placeholder="min 6 characters..."
                                    prop:value=password
                                    on:input=move |e| password.set(event_target_value(&e))
                                />
                            </div>
                        </div>

                        // ── Collapsible invite section ────────────────────
                        <button
                            type="button"
                            class="btn-ghost"
                            on:click=move |_| show_invite.update(|v| *v = !*v)
                        >
                            {move || if show_invite.get() {
                                "▾ Hide invite token"
                            } else {
                                "▸ Have an invite token?"
                            }}
                        </button>

                        {move || show_invite.get().then(|| view! {
                            <div class="field">
                                <label class="field-lbl">"Invite Token"</label>
                                <div class="field-inner">
                                    <span class="field-pfx">"key"</span>
                                    <input
                                        type="text"
                                        class="field-input"
                                        placeholder="Paste token from your admin"
                                        prop:value=invite_token
                                        on:input=move |e| invite_token.set(event_target_value(&e))
                                    />
                                </div>
                                <p class="field-hint">
                                    "Joining an existing workspace? Paste the invite token your admin sent you."
                                </p>
                            </div>
                        })}

                        {move || error.get().map(|msg| view! {
                            <div class="err-bar">
                                <span class="err-icon">"⚠"</span>
                                {msg}
                            </div>
                        })}

                        <button type="submit" class="btn-primary" disabled=move || loading.get()>
                            {move || if loading.get() { "Creating account…" } else { "Create Account" }}
                        </button>
                    </form>

                    <footer class="auth-foot">
                        "Already have an account? "
                        <A href="/login" class="link">"Sign in"</A>
                    </footer>
                </div>
            </div>
        </div>
    }
}
