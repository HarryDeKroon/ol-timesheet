use crate::components::settings_dialog::SettingsDialog;
use crate::components::timesheet_view::TimesheetView;
use crate::connection::provide_connection_context;
use crate::i18n::I18n;
use leptos::prelude::*;

#[cfg(not(feature = "ssr"))]
use leptos::web_sys;

// Import SVG flag icons from the shared flags module
pub use crate::flags::{FLAG_FR, FLAG_NL, FLAG_UK};

#[server(ValidateToken, "/api")]
pub async fn validate_token(token: String) -> Result<bool, ServerFnError> {
    Ok(crate::model::validate_token(&token))
}

#[component]
pub fn App() -> impl IntoView {
    // ── I18n context ──
    let i18n = RwSignal::new(I18n::default());
    provide_context(i18n);

    // ── Connection heartbeat context ──
    provide_connection_context();

    // ── Auth state ──
    // None = still checking, Some(false) = show settings, Some(true) = show timesheet
    let view_state = RwSignal::new(Option::<bool>::None);

    #[cfg(not(feature = "ssr"))]
    {
        let token = web_sys::window()
            .and_then(|w| w.local_storage().ok()?)
            .and_then(|s| s.get_item("timesheet_token").ok()?);

        leptos::task::spawn_local(async move {
            let confirmed = if let Some(token) = token {
                matches!(validate_token(token).await, Ok(true))
            } else {
                false
            };
            view_state.set(Some(confirmed));
        });
    }

    let on_settings_ok = Callback::new(move |_: ()| {
        view_state.set(Some(true));
    });

    view! {
        <main>
            {move || match view_state.get() {
                None => view! { <div class="loading">{move || i18n.get().t(crate::i18n::keys::LOADING)}</div> }.into_any(),
                Some(false) => view! { <SettingsDialog on_ok=on_settings_ok on_cancel=on_settings_ok /> }.into_any(),
                Some(true) => view! { <TimesheetView /> }.into_any(),
            }}
        </main>
    }
}
