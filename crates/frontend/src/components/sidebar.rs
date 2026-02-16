use leptos::*;
use crate::core::module_registry::*;

#[component]
pub fn Sidebar() -> impl IntoView {

    let modules = get_modules();

    view! {
        <nav>
        {
            modules.into_iter().map(|m| {
                view!{
                    <div class="menu">
                        {m.name}
                    </div>
                }
            }).collect_view()
        }
        </nav>
    }
}