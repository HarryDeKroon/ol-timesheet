use crate::components::timesheet_view::TimesheetView;
use crate::connection::provide_connection_context;
use crate::i18n::I18n;
use leptos::prelude::*;

#[cfg(not(feature = "ssr"))]
use leptos::web_sys;

// Import SVG flag icons from the shared flags module
pub use crate::flags::{FLAG_FR, FLAG_NL, FLAG_UK};

/// Check whether the current request has a valid authenticated session.
/// Returns `Some(display_name)` when logged in, `None` when not.
#[server(CheckSession, "/api")]
pub async fn check_session() -> Result<Option<String>, ServerFnError> {
    match crate::auth::current_user_session().await {
        Ok((_, session)) => Ok(Some(session.display_name)),
        Err(_) => Ok(None),
    }
}

#[component]
pub fn App() -> impl IntoView {
    // ── I18n context ──
    let i18n = RwSignal::new(I18n::default());
    provide_context(i18n);

    // ── Connection heartbeat context ──
    provide_connection_context();

    // ── Auth state ──
    // None = still checking, Some(false) = unauthenticated, Some(true) = authenticated
    let view_state = RwSignal::new(Option::<bool>::None);

    leptos::task::spawn_local(async move {
        let authenticated = matches!(check_session().await, Ok(Some(_)));
        view_state.set(Some(authenticated));

        #[cfg(not(feature = "ssr"))]
        if !authenticated {
            if let Some(window) = web_sys::window() {
                let _ = window.location().set_href("/auth/login");
            }
        }
    });

    view! {
        <main>
            {move || match view_state.get() {
                None => view! { <div class="loading">{move || i18n.get().t(crate::i18n::keys::LOADING)}</div> }.into_any(),
                Some(false) => view! { <div class="loading">{move || i18n.get().t(crate::i18n::keys::LOADING)}</div> }.into_any(),
                Some(true) => view! { <TimesheetView /> }.into_any(),
            }}
        </main>
    }
}
