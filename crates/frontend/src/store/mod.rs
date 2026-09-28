use gloo_storage::{LocalStorage, Storage};
use leptos::*;

use crate::models::{SessionTenant, SessionUser};

const KEY_TOKEN:  &str = "forge_token";
const KEY_USER:   &str = "forge_user";
const KEY_TENANT: &str = "forge_tenant";

/// Reactive auth session — survives page refresh via localStorage.
#[derive(Clone, Debug)]
pub struct AuthState {
    pub token:  RwSignal<Option<String>>,
    pub user:   RwSignal<Option<SessionUser>>,
    pub tenant: RwSignal<Option<SessionTenant>>,
}

impl AuthState {
    pub fn load() -> Self {
        Self {
            token:  create_rw_signal(LocalStorage::get(KEY_TOKEN).ok()),
            user:   create_rw_signal(LocalStorage::get(KEY_USER).ok()),
            tenant: create_rw_signal(LocalStorage::get(KEY_TENANT).ok()),
        }
    }

    /// Persist session after successful login / register.
    pub fn set(&self, token: String, user: SessionUser, tenant: SessionTenant) {
        let _ = LocalStorage::set(KEY_TOKEN,  &token);
        let _ = LocalStorage::set(KEY_USER,   &user);
        let _ = LocalStorage::set(KEY_TENANT, &tenant);
        self.token.set(Some(token));
        self.user.set(Some(user));
        self.tenant.set(Some(tenant));
    }

    /// Clear session on logout or 401.
    pub fn clear(&self) {
        LocalStorage::delete(KEY_TOKEN);
        LocalStorage::delete(KEY_USER);
        LocalStorage::delete(KEY_TENANT);
        self.token.set(None);
        self.user.set(None);
        self.tenant.set(None);
    }

    pub fn is_authenticated(&self) -> bool {
        self.token.get_untracked().is_some()
    }

    pub fn token_str(&self) -> String {
        self.token.get_untracked().unwrap_or_default()
    }

    pub fn username(&self) -> String {
        self.user.get_untracked().map(|u| u.username).unwrap_or_default()
    }

    pub fn tenant_name(&self) -> String {
        self.tenant.get_untracked().map(|t| t.name).unwrap_or_default()
    }

    pub fn tenant_id(&self) -> String {
        self.tenant.get_untracked().map(|t| t.id).unwrap_or_default()
    }
}

/// Mount AuthState into Leptos context tree.
#[component]
pub fn AuthProvider(children: Children) -> impl IntoView {
    provide_context(AuthState::load());
    children()
}

/// Access AuthState from any child component.
pub fn use_auth() -> AuthState {
    use_context::<AuthState>().expect("AuthProvider must be mounted above this component")
}
