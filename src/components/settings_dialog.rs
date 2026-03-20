use crate::components::folder_field::FolderField;
use crate::components::password_field::PasswordField;
use crate::components::settings_group::SettingsGroup;
use crate::i18n::{I18n, keys};
use crate::model::Settings;
use leptos::prelude::*;

#[server(GetSettings, "/api")]
pub async fn get_settings() -> Result<Settings, ServerFnError> {
    Ok(crate::model::load_settings())
}

#[server(SaveSettings, "/api")]
pub async fn save_settings(settings: Settings) -> Result<String, ServerFnError> {
    crate::model::save_settings(&settings).map_err(|e| ServerFnError::new(e))
}

#[component]
pub fn SettingsDialog(on_ok: Callback<()>, on_cancel: Callback<()>) -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().expect("I18n context");

    // Compute translated labels eagerly — safe because this component only
    // renders after hydration, when the browser locale has already been set.
    let ti = i18n.get_untracked();
    let title_ol = ti.t(keys::OL_JIRA);
    let title_git = ti.t(keys::GIT_WORKSPACE);
    let title_prefs = ti.t(keys::PREFERENCES);
    let lbl_email = ti.t(keys::EMAIL);
    let lbl_api_token = ti.t(keys::API_TOKEN);
    let lbl_username = ti.t(keys::USERNAME);
    let lbl_username2 = ti.t(keys::USERNAME);
    let lbl_app_password = ti.t(keys::APP_PASSWORD);
    let lbl_password = ti.t(keys::PASSWORD);
    let lbl_repository = ti.t(keys::REPOSITORY);
    let lbl_hpw = ti.t(keys::HOURS_PER_WEEK);
    let lbl_hpd = ti.t(keys::HOURS_PER_DAY);
    let email_placeholder = ti.t(keys::EMAIL_PLACEHOLDER);
    let api_token_placeholder = ti.t(keys::API_TOKEN_PLACEHOLDER);
    let username_placeholder = ti.t(keys::USERNAME_PLACEHOLDER);
    let app_password_placeholder = ti.t(keys::APP_PASSWORD_PLACEHOLDER);
    let password_placeholder = ti.t(keys::PASSWORD_PLACEHOLDER);
    let repository_placeholder = ti.t(keys::REPOSITORY_PLACEHOLDER);
    let bitbucket_disabled_msg = ti.t(keys::BITBUCKET_DISABLED);
    let ol_jira_disabled_msg = ti.t(keys::OL_JIRA_DISABLED);
    let git_poll_label = ti.t(keys::GIT_POLL_INTERVAL);
    let username_placeholder2 = username_placeholder.clone();

    let settings_resource = Resource::new(|| (), |_| get_settings());

    let email = RwSignal::new(String::new());
    let upland_jira_token = RwSignal::new(String::new());
    let bitbucket_username = RwSignal::new(String::new());
    let bitbucket_app_password = RwSignal::new(String::new());
    let ol_jira_username = RwSignal::new(String::new());
    let ol_jira_password = RwSignal::new(String::new());
    let git_folder = RwSignal::new(String::new());
    let git_poll_interval_minutes = RwSignal::new("5".to_string());
    let hours_per_week = RwSignal::new("40".to_string());
    let hours_per_day = RwSignal::new("8".to_string());
    let error_msg = RwSignal::new(Option::<String>::None);

    let save_action = Action::new(move |_: &()| {
        let settings = Settings {
            email: email.get(),
            upland_jira_token: upland_jira_token.get(),
            bitbucket_username: bitbucket_username.get(),
            bitbucket_app_password: bitbucket_app_password.get(),
            ol_jira_username: ol_jira_username.get(),
            ol_jira_password: ol_jira_password.get(),
            git_folder: git_folder.get(),
            git_poll_interval_minutes: git_poll_interval_minutes.get().parse().unwrap_or(5),
            hours_per_week: hours_per_week.get().parse().unwrap_or(40.0),
            hours_per_day: hours_per_day.get().parse().unwrap_or(8.0),
            ..Default::default()
        };
        async move { save_settings(settings).await }
    });

    Effect::new(move |_| {
        #[allow(unused_variables)]
        if let Some(Ok(token)) = save_action.value().get() {
            #[cfg(feature = "hydrate")]
            {
                if let Some(storage) =
                    leptos::web_sys::window().and_then(|w| w.local_storage().ok()?)
                {
                    let _ = storage.set_item("timesheet_token", &token);
                }
            }
            on_ok.run(());
        }
        if let Some(Err(e)) = save_action.value().get() {
            error_msg.set(Some(e.to_string()));
        }
    });

    Effect::new(move |_| {
        settings_resource.get().map(|result| {
            if let Ok(s) = result {
                email.set(s.email);
                upland_jira_token.set(s.upland_jira_token);
                bitbucket_username.set(s.bitbucket_username);
                bitbucket_app_password.set(s.bitbucket_app_password);
                ol_jira_username.set(s.ol_jira_username);
                ol_jira_password.set(s.ol_jira_password);
                git_folder.set(s.git_folder);
                git_poll_interval_minutes.set(s.git_poll_interval_minutes.to_string());
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

                    <SettingsGroup title={ti.tr(keys::UPLAND_JIRA)}>
                            <label>{lbl_email.clone()}":"</label>
                            <input
                                type="text"
                                placeholder={email_placeholder}
                                prop:value={move || email.get()}
                                on:input=move |ev| email.set(event_target_value(&ev))
                                class="settings-input"
                        />
                            <PasswordField
                                label=lbl_api_token.clone()
                                placeholder={api_token_placeholder}
                                link_url="https://id.atlassian.com/manage-profile/security/api-tokens".to_string()
                                value=upland_jira_token
                            />
                        </SettingsGroup>

                        <SettingsGroup title={ti.t(keys::BITBUCKET)} disabled=true>
                            <div class="warning warning-full-row">
                                {bitbucket_disabled_msg.clone()}
                            </div>
                            <label>
                                <a href="https://bitbucket.org/account/settings/" target="_blank" rel="noopener noreferrer">
                                    {lbl_username.clone()}
                                </a>":"
                            </label>
                            <input
                                type="text"
                                placeholder={username_placeholder.clone()}
                                prop:value=move || bitbucket_username.get()
                                disabled=true
                                class="settings-input"
                            />
                            <PasswordField
                                label=lbl_app_password.clone()
                                placeholder={app_password_placeholder}
                                link_url="https://bitbucket.org/account/settings/app-passwords/".to_string()
                                value=bitbucket_app_password
                                disabled=true
                            />
                        </SettingsGroup>

                        <SettingsGroup title=title_ol.clone() disabled=true>
                            <div class="warning warning-full-row">
                                {ol_jira_disabled_msg.clone()}
                            </div>
                            <label>{lbl_username2.clone()}":"</label>
                            <input
                                type="text"
                                placeholder={username_placeholder2}
                                prop:value={move || ol_jira_username.get()}
                                on:input=move |ev| ol_jira_username.set(event_target_value(&ev))
                                class="settings-input"
                            />
                            <PasswordField
                                label=lbl_password.clone()
                                placeholder={password_placeholder}
                                value=ol_jira_password
                            />
                        </SettingsGroup>

                    <SettingsGroup title=title_git.clone()>
                        <FolderField
                            label=lbl_repository.clone()
                            placeholder={repository_placeholder}
                            value=git_folder
                        />
                        <label>{git_poll_label.clone()}</label>
                        <input
                            type="number"
                            min="1"
                            prop:value={move || git_poll_interval_minutes.get()}
                            on:input=move |ev| git_poll_interval_minutes.set(event_target_value(&ev))
                            class="settings-input"
                        />
                    </SettingsGroup>

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
