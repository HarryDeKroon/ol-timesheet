use crate::components::settings_group::SettingsGroup;
use crate::i18n::{I18n, keys};
use crate::model::Settings;
use leptos::prelude::*;

#[server(GetSettings, "/api")]
pub async fn get_settings() -> Result<Settings, ServerFnError> {
    let (session_id, session) = crate::auth::current_user_session().await?;
    let _ = session_id;
    Ok(session.preferences)
}

#[server(SaveSettings, "/api")]
pub async fn save_settings(settings: Settings) -> Result<(), ServerFnError> {
    let (session_id, session) = crate::auth::current_user_session().await?;
    crate::auth::save_user_prefs(&session.account_id, &settings).map_err(ServerFnError::new)?;
    crate::auth::update_session_prefs(&session_id, settings);
    Ok(())
}

#[component]
pub fn SettingsDialog(on_ok: Callback<()>, on_cancel: Callback<()>) -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().unwrap_or_else(|| {
        log::error!("I18n context not provided in SettingsDialog, using English fallback");
        RwSignal::new(I18n::default())
    });

    let ti = i18n.get_untracked();
    let title_prefs = ti.t(keys::PREFERENCES);
    let lbl_hpw = ti.t(keys::HOURS_PER_WEEK);
    let lbl_hpd = ti.t(keys::HOURS_PER_DAY);

    let settings_resource = Resource::new(|| (), |_| get_settings());

    let hours_per_week = RwSignal::new("40".to_string());
    let hours_per_day = RwSignal::new("8".to_string());
    let error_msg = RwSignal::new(Option::<String>::None);

    let save_action = Action::new(move |_: &()| {
        let settings = Settings {
            hours_per_week: hours_per_week.get().parse().unwrap_or(40.0),
            hours_per_day: hours_per_day.get().parse().unwrap_or(8.0),
        };
        async move { save_settings(settings).await }
    });

    Effect::new(move |_| {
        if let Some(Ok(())) = save_action.value().get() {
            on_ok.run(());
        }
        if let Some(Err(e)) = save_action.value().get() {
            error_msg.set(Some(e.to_string()));
        }
    });

    Effect::new(move |_| {
        settings_resource.get().map(|result| {
            if let Ok(s) = result {
                hours_per_week.set(s.hours_per_week.to_string());
                hours_per_day.set(s.hours_per_day.to_string());
            }
        });
    });

    view! {
        <div class="settings-overlay">
            <div class="settings-dialog">
                <h2>{move || i18n.get().t(keys::SETTINGS_TITLE)}</h2>

                <Suspense fallback=move || view! { <p>{move || i18n.get().t(keys::LOADING_SETTINGS)}</p> }>
                    <SettingsGroup title=title_prefs.clone()>
                        <label>{lbl_hpw.clone()}":"</label>
                        <input
                            type="number"
                            step="0.5"
                            min="1"
                            max="168"
                            prop:value={move || hours_per_week.get()}
                            on:input=move |ev| hours_per_week.set(event_target_value(&ev))
                            class="settings-input"
                        />
                        <label>{lbl_hpd.clone()}":"</label>
                        <input
                            type="number"
                            step="0.5"
                            min="1"
                            max="24"
                            prop:value={move || hours_per_day.get()}
                            on:input=move |ev| hours_per_day.set(event_target_value(&ev))
                            class="settings-input"
                        />
                    </SettingsGroup>
                </Suspense>

                {move || error_msg.get().map(|msg| view! {
                    <p class="error">{msg}</p>
                })}

                <div>
                    <button
                        class="btn-ok"
                        on:click=move |_| { save_action.dispatch(()); }
                    >
                        {move || i18n.get().t(keys::OK)}
                    </button>
                    <button
                        class="btn-cancel"
                        on:click=move |_| { on_cancel.run(()); }
                    >
                        {move || i18n.get().t(keys::CANCEL)}
                    </button>
                </div>
            </div>
        </div>
    }
}
