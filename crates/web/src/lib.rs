pub mod app;
pub mod components;
pub mod types;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount_to_body(App);
}

/// Client-side rendering entry point (no SSR/hydration needed)
#[cfg(feature = "csr")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn main() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount_to_body(App);
}
