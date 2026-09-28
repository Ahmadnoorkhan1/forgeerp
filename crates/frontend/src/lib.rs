mod api;
mod models;
mod pages;
mod store;

use leptos::*;
use leptos_router::*;
use wasm_bindgen::prelude::*;

use pages::{
    inventory::{AdjustStockPage, InventoryPage, CreateItemPage},
    login::LoginPage,
    register::RegisterPage,
};
use store::AuthProvider;

/// WASM entry point — called automatically when the module loads in the browser.
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    view! {
        <AuthProvider>
            <Router>
                <Routes>
                    <Route path="/"           view=|| view! { <Redirect path="/login"/> }/>
                    <Route path="/login"      view=LoginPage/>
                    <Route path="/register"   view=RegisterPage/>
                    <Route path="/inventory"  view=InventoryPage/>
                    <Route path="/inventory/create"  view=CreateItemPage/>
                    <Route path="/adjust/:id" view=AdjustStockPage/>
                </Routes>
            </Router>
        </AuthProvider>
    }
    // mount_to_body(|| {
    //     view! {
    //         <AuthProvider>
    //             <App/>
    //         </AuthProvider>
    //     }
    // });
}