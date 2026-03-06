pub mod api;
pub mod app;
pub mod components;
pub mod connection;
pub mod flags;
pub mod formatting;
pub mod i18n;
pub mod model;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use app::App;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
