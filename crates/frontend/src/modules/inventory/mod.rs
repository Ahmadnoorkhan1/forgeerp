mod inventory_page;
mod create_item_page;

use leptos::*;
use crate::core::module_registry::*;
use crate::modules::inventory::inventory_page::InventoryPage;
use crate::modules::inventory::create_item_page::CreateItemPage;

pub fn register() {
    register_module(Module {
        name: "Inventory",
        pages: vec![
            ModulePage {
                path: "/inventory",
                view: || view! { <InventoryPage/> }.into_view(),
            },
            ModulePage {
                path: "/inventory/create",
                view: || view! { <CreateItemPage/> }.into_view(),
            }
        ],
    });
}