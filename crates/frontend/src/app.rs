use leptos::*;
use leptos_router::*;

use crate::core::module_registry::*;
use crate::modules::inventory;

#[component]
pub fn App() -> impl IntoView {

    inventory::register();

    let modules = get_modules();
    println!("Registered modules: {:?}", modules.iter().map(|m| &m.name).collect::<Vec<_>>());

    view! {
        
    }
    // view! {
    //     <Router>

    //         <Routes>
    //             {
    //                 modules.into_iter().flat_map(|m| {
    //                     m.pages.into_iter().map(|p| {
    //                         view! {
    //                             <Route path=p.path view=p.view/>
    //                         }
    //                     }).collect::<Vec<_>>()
    //                 }).collect_view()
    //             }
    //         </Routes>

    //     </Router>
    // }
}