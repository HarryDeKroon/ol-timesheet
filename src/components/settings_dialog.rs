use crate::components::settings_group::SettingsGroup;
use crate::flags::{FLAG_FR, FLAG_NL, FLAG_UK};
use crate::i18n::{I18n, keys};
use crate::model::Settings;
use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos::web_sys;

fn parse_list_input(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c == ';' || c == '\n' || c == '\r')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn join_list_input(items: &[String]) -> String {
    items.join(", ")
}

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

    let supported_langs = std::sync::Arc::new(vec![
        ("en", "English", FLAG_UK),
        ("fr", "Français", FLAG_FR),
        ("nl", "Nederlands", FLAG_NL),
    ]);
    let langs_for_button = supported_langs.clone();
    let lang_menu_open = RwSignal::new(false);
    let current_lang = RwSignal::new(i18n.get_untracked().lang.clone());
    let title_language = i18n.get_untracked().t(keys::LANGUAGE);

    let on_lang_change = {
        let i18n = i18n.clone();
        move |new_lang: String| {
            #[cfg(feature = "hydrate")]
            {
                if let Some(window) = web_sys::window() {
                    if let Some(storage) = window.local_storage().ok().flatten() {
                        let _ = storage.set_item("timesheet_lang", &new_lang);
                    }
                }
            }
            current_lang.set(new_lang.clone());
            i18n.set(I18n::new(&new_lang));
        }
    };

    let ti = i18n.get_untracked();
    let title_prefs = ti.t(keys::DURATIONS);
    let title_reporting = ti.t(keys::REPORTING);
    let title_pull_requests = ti.t(keys::PULL_REQUESTS);
    let lbl_hpw = ti.t(keys::HOURS_PER_WEEK);
    let lbl_hpd = ti.t(keys::HOURS_PER_DAY);

    let settings_resource = Resource::new(|| (), |_| get_settings());

    let hours_per_week = RwSignal::new("40".to_string());
    let hours_per_day = RwSignal::new("8".to_string());
    let non_billable_project_prefixes = RwSignal::new(String::new());
    let meeting_keys = RwSignal::new(String::new());
    let local_holiday_keys = RwSignal::new(String::new());
    let planned_time_off_keys = RwSignal::new(String::new());
    let study_keys = RwSignal::new(String::new());
    let show_merged_pr_activity = RwSignal::new(true);
    let error_msg = RwSignal::new(Option::<String>::None);

    let save_action = Action::new(move |_: &()| {
        let settings = Settings {
            hours_per_week: hours_per_week.get().parse().unwrap_or(40.0),
            hours_per_day: hours_per_day.get().parse().unwrap_or(8.0),
            non_billable_project_prefixes: parse_list_input(&non_billable_project_prefixes.get()),
            meeting_keys: parse_list_input(&meeting_keys.get()),
            local_holiday_keys: parse_list_input(&local_holiday_keys.get()),
            planned_time_off_keys: parse_list_input(&planned_time_off_keys.get()),
            study_keys: parse_list_input(&study_keys.get()),
            show_merged_pr_activity: show_merged_pr_activity.get(),
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
                non_billable_project_prefixes
                    .set(join_list_input(&s.non_billable_project_prefixes));
                meeting_keys.set(join_list_input(&s.meeting_keys));
                local_holiday_keys.set(join_list_input(&s.local_holiday_keys));
                planned_time_off_keys.set(join_list_input(&s.planned_time_off_keys));
                study_keys.set(join_list_input(&s.study_keys));
                show_merged_pr_activity.set(s.show_merged_pr_activity);
            }
        });
    });

    view! {
        <div class="settings-overlay">
            <div class="settings-backdrop" on:click=move |_| on_cancel.run(())></div>
            <div class="settings-dialog">
                <h2>{move || i18n.get().t(keys::SETTINGS_TITLE)}</h2>

                <Suspense fallback=move || view! { <p>{move || i18n.get().t(keys::LOADING_SETTINGS)}</p> }>
                    <SettingsGroup title=title_language.clone()>
                        <div class="lang-dropdown">
                            <button class="lang-btn settings-lang-btn" on:click=move |_| lang_menu_open.update(|open| *open = !*open)>
                                <span inner_html={move || {
                                    langs_for_button
                                        .iter()
                                        .find(|(code, _, _)| *code == current_lang.get())
                                        .map(|(_, _, flag)| *flag)
                                        .unwrap_or(FLAG_UK)
                                }}></span>
                                <span class="lang-caret">{move || if lang_menu_open.get() { "▲" } else { "▼" }}</span>
                            </button>
                            <div class=move || if lang_menu_open.get() { "lang-menu lang-menu-open" } else { "lang-menu" }>
                                {supported_langs.iter().map(|(code, name, flag)| {
                                    let code = code.to_string();
                                    let is_selected = current_lang.get() == *code;
                                    let on_click = {
                                        let code = code.clone();
                                        let on_lang_change = on_lang_change.clone();
                                        let lang_menu_open = lang_menu_open.clone();
                                        move |_| {
                                            on_lang_change(code.clone());
                                            lang_menu_open.set(false);
                                        }
                                    };
                                    view! {
                                        <div class="lang-menu-item" class:lang-menu-item-selected=is_selected on:click=on_click>
                                            <span inner_html={*flag} title={*name}></span>
                                            <span>{*name}</span>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        </div>
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
                    <SettingsGroup title=title_reporting.clone()>
                        <label>{move || i18n.get().t(keys::NON_BILLABLE_PROJECTS)}":"</label>
                        <input
                            type="text"
                            prop:value={move || non_billable_project_prefixes.get()}
                            placeholder={move || i18n.get().t(keys::LIST_INPUT_HINT)}
                            on:input=move |ev| non_billable_project_prefixes.set(event_target_value(&ev))
                            class="settings-input"
                        />
                        <label>{move || i18n.get().t(keys::MEETINGS)}":"</label>
                        <input
                            type="text"
                            prop:value={move || meeting_keys.get()}
                            placeholder={move || i18n.get().t(keys::LIST_INPUT_HINT)}
                            on:input=move |ev| meeting_keys.set(event_target_value(&ev))
                            class="settings-input"
                        />
                        <label>{move || i18n.get().t(keys::LOCAL_HOLIDAYS)}":"</label>
                        <input
                            type="text"
                            prop:value={move || local_holiday_keys.get()}
                            placeholder={move || i18n.get().t(keys::LIST_INPUT_HINT)}
                            on:input=move |ev| local_holiday_keys.set(event_target_value(&ev))
                            class="settings-input"
                        />
                        <label>{move || i18n.get().t(keys::PLANNED_TIME_OFF)}":"</label>
                        <input
                            type="text"
                            prop:value={move || planned_time_off_keys.get()}
                            placeholder={move || i18n.get().t(keys::LIST_INPUT_HINT)}
                            on:input=move |ev| planned_time_off_keys.set(event_target_value(&ev))
                            class="settings-input"
                        />
                        <label>{move || i18n.get().t(keys::STUDY)}":"</label>
                        <input
                            type="text"
                            prop:value={move || study_keys.get()}
                            placeholder={move || i18n.get().t(keys::LIST_INPUT_HINT)}
                            on:input=move |ev| study_keys.set(event_target_value(&ev))
                            class="settings-input"
                        />
                    </SettingsGroup>
                    <SettingsGroup title=title_pull_requests.clone()>
                        <label class="settings-checkbox-row">
                            <input
                                type="checkbox"
                                prop:checked={move || show_merged_pr_activity.get()}
                                on:change=move |ev| show_merged_pr_activity.set(event_target_checked(&ev))
                            />
                            {move || i18n.get().t(keys::SHOW_MERGED_PR_ACTIVITY)}
                        </label>
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
