use leptos::*;

#[derive(Clone)]
pub struct ModulePage {
    pub path: &'static str,
    pub view: fn() -> View,
}

#[derive(Clone)]
pub struct Module {
    pub name: &'static str,
    pub pages: Vec<ModulePage>,
}

static mut MODULES: Vec<Module> = Vec::new();

pub fn register_module(module: Module) {
    unsafe {
        MODULES.push(module);
    }
}

pub fn get_modules() -> Vec<Module> {
    unsafe { MODULES.clone() }
}