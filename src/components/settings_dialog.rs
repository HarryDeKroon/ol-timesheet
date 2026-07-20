use crate::components::settings_group::SettingsGroup;
use crate::flags::{FLAG_FR, FLAG_NL, FLAG_UK};
use crate::i18n::{I18n, keys};
use crate::model::{Settings, WorkItem};
use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos::web_sys;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn normalize_list(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .iter()
        .map(|value| value.trim().to_uppercase())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn issue_project_prefix(issue_key: &str) -> String {
    issue_key
        .split_once('-')
        .map(|(prefix, _)| prefix.to_uppercase())
        .unwrap_or_else(|| issue_key.to_uppercase())
}

fn derive_non_billable_options(active_items: &[WorkItem]) -> Vec<String> {
    let mut seen = HashSet::new();
    active_items
        .iter()
        .map(|item| issue_project_prefix(&item.key))
        .filter(|prefix| seen.insert(prefix.clone()))
        .collect()
}

fn derive_scoped_issue_options(active_items: &[WorkItem], prefixes: &[String]) -> Vec<String> {
    let prefix_set = prefixes
        .iter()
        .map(|prefix| prefix.trim().to_uppercase())
        .filter(|prefix| !prefix.is_empty())
        .collect::<HashSet<_>>();
    if prefix_set.is_empty() {
        return Vec::new();
    }
    active_items
        .iter()
        .map(|item| item.key.trim().to_uppercase())
        .filter(|key| {
            key.split_once('-')
                .map(|(prefix, _)| prefix_set.contains(prefix))
                .unwrap_or(false)
        })
        .collect()
}

fn invalid_values(selected: &[String], allowed: &[String]) -> Vec<String> {
    let allowed_set = allowed.iter().collect::<HashSet<_>>();
    selected
        .iter()
        .filter(|value| !allowed_set.contains(value))
        .cloned()
        .collect()
}

fn duplicate_values(selected: &[String], counts: &HashMap<String, usize>) -> Vec<String> {
    selected
        .iter()
        .filter(|value| counts.get(*value).copied().unwrap_or(0) > 1)
        .cloned()
        .collect()
}

fn is_whole_number(value: f64) -> bool {
    (value - value.round()).abs() < f64::EPSILON
}

fn is_half_step(value: f64) -> bool {
    ((value * 2.0) - (value * 2.0).round()).abs() < f64::EPSILON
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReportingOptions {
    pub active_items: Vec<WorkItem>,
}

#[server(GetSettings, "/api")]
pub async fn get_settings() -> Result<Settings, ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
    Ok(session.preferences)
}

#[server(GetReportingOptions, "/api")]
pub async fn get_reporting_options() -> Result<ReportingOptions, ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
    let creds = session.jira_credentials();
    let active_items = crate::api::jira::fetch_assigned_active_work_items_oldest(&creds, false)
        .await
        .map_err(ServerFnError::new)?;
    Ok(ReportingOptions { active_items })
}

#[server(SaveSettings, "/api")]
pub async fn save_settings(settings: Settings) -> Result<(), ServerFnError> {
    let (session_id, session) = crate::auth::current_user_session().await?;
    crate::auth::save_user_prefs(&session.account_id, &settings).map_err(ServerFnError::new)?;
    crate::auth::update_session_prefs(&session_id, settings);
    Ok(())
}

fn add_unique(signal: RwSignal<Vec<String>>, value: String) {
    signal.update(|items| {
        if !items.iter().any(|item| item == &value) {
            items.push(value);
        }
    });
}

fn remove_value(signal: RwSignal<Vec<String>>, value: &str) {
    signal.update(|items| items.retain(|item| item != value));
}

#[component]
fn MultiSelectField(
    label: Signal<String>,
    selected: RwSignal<Vec<String>>,
    options: Signal<Vec<String>>,
    item_titles: Signal<HashMap<String, String>>,
    select_value: RwSignal<String>,
    add_placeholder: Signal<String>,
    remove_label: Signal<String>,
    error: Signal<Option<String>>,
) -> impl IntoView {
    let select_ref: NodeRef<leptos::html::Select> = NodeRef::new();
    let open_select = {
        #[cfg(feature = "hydrate")]
        {
            let select_ref = select_ref.clone();
            move || {
                use js_sys::{Function, Reflect};
                use leptos::wasm_bindgen::{JsCast, JsValue};
                if let Some(select) = select_ref.get() {
                    let select_el: &web_sys::HtmlSelectElement = select.unchecked_ref();
                    let _ = select_el.focus();
                    let select_js: &JsValue = select_el.as_ref();
                    if let Ok(show_picker) =
                        Reflect::get(select_js, &JsValue::from_str("showPicker"))
                    {
                        if let Some(show_picker_fn) = show_picker.dyn_ref::<Function>() {
                            let _ = show_picker_fn.call0(select_js);
                            return;
                        }
                    }
                    let element: &web_sys::HtmlElement = select.unchecked_ref();
                    element.click();
                }
            }
        }
        #[cfg(not(feature = "hydrate"))]
        {
            move || {}
        }
    };

    view! {
        <label>{move || label.get()}":"</label>
        <div class="settings-select-field" class:settings-field-invalid=move || error.get().is_some()>
            <div
                class="settings-chip-list settings-input"
                tabindex="0"
                role="button"
                on:click=move |_| open_select()
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    if matches!(ev.key().as_str(), "Enter" | " " | "ArrowDown") {
                        ev.prevent_default();
                        open_select();
                    }
                }
            >
                <For
                    each=move || selected.get()
                    key=|item| item.clone()
                    children=move |item| {
                        let remove_item = item.clone();
                        let key_text = item.clone();
                        view! {
                            <span
                                class="settings-chip"
                                title=move || item_titles.get().get(&key_text).cloned().unwrap_or_default()
                            >
                                <span class="settings-chip-text">{item.clone()}</span>
                                <button
                                    type="button"
                                    class="settings-chip-remove settings-input"
                                    on:mousedown=move |ev| ev.stop_propagation()
                                    on:click=move |ev| {
                                        ev.stop_propagation();
                                        remove_value(selected, &remove_item);
                                    }
                                    aria-label=move || remove_label.get()
                                >
                                    "×"
                                </button>
                            </span>
                        }
                    }
                />
            </div>
            <select
                node_ref=select_ref
                tabindex="-1"
                class="settings-select-native"
                prop:value=move || select_value.get()
                on:change=move |ev| {
                    let value = event_target_value(&ev);
                    if !value.is_empty() {
                        add_unique(selected, value.clone());
                    }
                    select_value.set(String::new());
                }
            >
                <option value="">{move || add_placeholder.get()}</option>
                <For
                    each=move || {
                        let selected_set = selected.get().into_iter().collect::<HashSet<_>>();
                        options
                            .get()
                            .into_iter()
                            .filter(|option| !selected_set.contains(option))
                            .collect::<Vec<_>>()
                    }
                    key=|item| item.clone()
                    children=move |item| {
                        let item_value = item.clone();
                        let option_text = item_titles
                            .get()
                            .get(&item)
                            .map(|summary| format!("{} — {}", item, summary))
                            .unwrap_or_else(|| item.clone());
                        view! {
                            <option
                                value={item_value}
                                title={option_text.clone()}
                            >
                                {option_text.clone()}
                            </option>
                        }
                    }
                />
            </select>
            {move || {
                error.get().map(|message| {
                    view! {
                        <span class="settings-error-indicator" title={message}>!</span>
                    }
                })
            }}
        </div>
    }
}

#[component]
pub fn SettingsDialog(on_ok: Callback<()>, on_cancel: Callback<()>) -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().unwrap_or_else(|| {
        log::error!("I18n context not provided in SettingsDialog, using English fallback");
        RwSignal::new(I18n::default())
    });

    let supported_langs = Arc::new(vec![
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
    let reporting_options_resource = Resource::new(|| (), |_| get_reporting_options());

    let hours_per_week = RwSignal::new(40.0_f64);
    let hours_per_day = RwSignal::new(8.0_f64);
    let non_billable_project_prefixes = RwSignal::new(Vec::<String>::new());
    let meeting_keys = RwSignal::new(Vec::<String>::new());
    let local_holiday_keys = RwSignal::new(Vec::<String>::new());
    let planned_time_off_keys = RwSignal::new(Vec::<String>::new());
    let study_keys = RwSignal::new(Vec::<String>::new());
    let show_merged_pr_activity = RwSignal::new(true);
    let active_work_items = RwSignal::new(Vec::<WorkItem>::new());
    let non_billable_select_value = RwSignal::new(String::new());
    let meetings_select_value = RwSignal::new(String::new());
    let holidays_select_value = RwSignal::new(String::new());
    let pto_select_value = RwSignal::new(String::new());
    let study_select_value = RwSignal::new(String::new());
    let error_msg = RwSignal::new(Option::<String>::None);
    let loaded_settings = RwSignal::new(false);
    let loaded_reporting_options = RwSignal::new(false);
    let dialog_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    // Focus the dialog container the moment it mounts so that keyboard events
    // (Tab, Escape) are captured immediately — before the settings resource resolves.
    #[cfg(feature = "hydrate")]
    dialog_ref.on_load(move |el| {
        use leptos::wasm_bindgen::JsCast;
        let _ = el.unchecked_ref::<web_sys::HtmlElement>().focus();
    });

    let non_billable_options =
        Memo::new(move |_| derive_non_billable_options(&active_work_items.get()));
    let scoped_issue_options = Memo::new(move |_| {
        derive_scoped_issue_options(
            &active_work_items.get(),
            &non_billable_project_prefixes.get(),
        )
    });
    let issue_title_by_key = Memo::new(move |_| {
        active_work_items
            .get()
            .into_iter()
            .map(|item| (item.key.trim().to_uppercase(), item.summary))
            .collect::<HashMap<_, _>>()
    });
    let cross_category_counts = Memo::new(move |_| {
        let mut counts = HashMap::<String, usize>::new();
        for key in meeting_keys
            .get()
            .into_iter()
            .chain(local_holiday_keys.get())
            .chain(planned_time_off_keys.get())
            .chain(study_keys.get())
        {
            *counts.entry(key).or_insert(0) += 1;
        }
        counts
    });
    let meeting_options = Memo::new(move |_| {
        let blocked = local_holiday_keys
            .get()
            .into_iter()
            .chain(planned_time_off_keys.get())
            .chain(study_keys.get())
            .collect::<HashSet<_>>();
        scoped_issue_options
            .get()
            .into_iter()
            .filter(|key| !blocked.contains(key))
            .collect::<Vec<_>>()
    });
    let holiday_options = Memo::new(move |_| {
        let blocked = meeting_keys
            .get()
            .into_iter()
            .chain(planned_time_off_keys.get())
            .chain(study_keys.get())
            .collect::<HashSet<_>>();
        scoped_issue_options
            .get()
            .into_iter()
            .filter(|key| !blocked.contains(key))
            .collect::<Vec<_>>()
    });
    let pto_options = Memo::new(move |_| {
        let blocked = meeting_keys
            .get()
            .into_iter()
            .chain(local_holiday_keys.get())
            .chain(study_keys.get())
            .collect::<HashSet<_>>();
        scoped_issue_options
            .get()
            .into_iter()
            .filter(|key| !blocked.contains(key))
            .collect::<Vec<_>>()
    });
    let study_options = Memo::new(move |_| {
        let blocked = meeting_keys
            .get()
            .into_iter()
            .chain(local_holiday_keys.get())
            .chain(planned_time_off_keys.get())
            .collect::<HashSet<_>>();
        scoped_issue_options
            .get()
            .into_iter()
            .filter(|key| !blocked.contains(key))
            .collect::<Vec<_>>()
    });

    let hpw_error = Memo::new(move |_| {
        let value = hours_per_week.get();
        if (16.0..=42.0).contains(&value) && is_whole_number(value) {
            None
        } else {
            Some(i18n.get().t(keys::SETTINGS_ERROR_HOURS_PER_WEEK))
        }
    });
    let hpd_error = Memo::new(move |_| {
        let value = hours_per_day.get();
        if (4.0..=9.5).contains(&value) && is_half_step(value) {
            None
        } else {
            Some(i18n.get().t(keys::SETTINGS_ERROR_HOURS_PER_DAY))
        }
    });
    let non_billable_error = Memo::new(move |_| {
        let invalid = invalid_values(
            &non_billable_project_prefixes.get(),
            &non_billable_options.get(),
        );
        if invalid.is_empty() {
            None
        } else {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_NON_BILLABLE),
                invalid.join(", ")
            ))
        }
    });
    let meetings_error = Memo::new(move |_| {
        let invalid = invalid_values(&meeting_keys.get(), &scoped_issue_options.get());
        let duplicates = duplicate_values(&meeting_keys.get(), &cross_category_counts.get());
        if !invalid.is_empty() {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_REPORTING_WORK_ITEMS),
                invalid.join(", ")
            ))
        } else if !duplicates.is_empty() {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_REPORTING_UNIQUE),
                duplicates.join(", ")
            ))
        } else {
            None
        }
    });
    let holidays_error = Memo::new(move |_| {
        let invalid = invalid_values(&local_holiday_keys.get(), &scoped_issue_options.get());
        let duplicates = duplicate_values(&local_holiday_keys.get(), &cross_category_counts.get());
        if !invalid.is_empty() {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_REPORTING_WORK_ITEMS),
                invalid.join(", ")
            ))
        } else if !duplicates.is_empty() {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_REPORTING_UNIQUE),
                duplicates.join(", ")
            ))
        } else {
            None
        }
    });
    let pto_error = Memo::new(move |_| {
        let invalid = invalid_values(&planned_time_off_keys.get(), &scoped_issue_options.get());
        let duplicates =
            duplicate_values(&planned_time_off_keys.get(), &cross_category_counts.get());
        if !invalid.is_empty() {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_REPORTING_WORK_ITEMS),
                invalid.join(", ")
            ))
        } else if !duplicates.is_empty() {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_REPORTING_UNIQUE),
                duplicates.join(", ")
            ))
        } else {
            None
        }
    });
    let study_error = Memo::new(move |_| {
        let invalid = invalid_values(&study_keys.get(), &scoped_issue_options.get());
        let duplicates = duplicate_values(&study_keys.get(), &cross_category_counts.get());
        if !invalid.is_empty() {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_REPORTING_WORK_ITEMS),
                invalid.join(", ")
            ))
        } else if !duplicates.is_empty() {
            Some(format!(
                "{}: {}",
                i18n.get().t(keys::SETTINGS_ERROR_REPORTING_UNIQUE),
                duplicates.join(", ")
            ))
        } else {
            None
        }
    });

    let form_valid = Memo::new(move |_| {
        loaded_settings.get()
            && loaded_reporting_options.get()
            && hpw_error.get().is_none()
            && hpd_error.get().is_none()
            && non_billable_error.get().is_none()
            && meetings_error.get().is_none()
            && holidays_error.get().is_none()
            && pto_error.get().is_none()
            && study_error.get().is_none()
    });

    let save_action = Action::new(move |_: &()| {
        let settings = Settings {
            hours_per_week: hours_per_week.get(),
            hours_per_day: hours_per_day.get(),
            non_billable_project_prefixes: non_billable_project_prefixes.get(),
            meeting_keys: meeting_keys.get(),
            local_holiday_keys: local_holiday_keys.get(),
            planned_time_off_keys: planned_time_off_keys.get(),
            study_keys: study_keys.get(),
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
        settings_resource.get().map(|result| match result {
            Ok(s) => {
                hours_per_week.set(s.hours_per_week);
                hours_per_day.set(s.hours_per_day);
                non_billable_project_prefixes.set(normalize_list(&s.non_billable_project_prefixes));
                meeting_keys.set(normalize_list(&s.meeting_keys));
                local_holiday_keys.set(normalize_list(&s.local_holiday_keys));
                planned_time_off_keys.set(normalize_list(&s.planned_time_off_keys));
                study_keys.set(normalize_list(&s.study_keys));
                show_merged_pr_activity.set(s.show_merged_pr_activity);
                loaded_settings.set(true);

                #[cfg(feature = "hydrate")]
                {
                    use leptos::wasm_bindgen::JsCast;
                    use leptos::wasm_bindgen::closure::Closure;

                    if let Some(dialog) = dialog_ref.get() {
                        let dialog_html: web_sys::HtmlElement =
                            dialog.unchecked_ref::<web_sys::HtmlElement>().clone();
                        let cb = Closure::once(move || {
                            let inputs =
                                dialog_html.get_elements_by_class_name("settings-initial-focus");
                            if let Some(node) = inputs.item(0) {
                                if let Some(input) = node.dyn_ref::<web_sys::HtmlElement>() {
                                    let _ = input.focus();
                                }
                            }
                        });
                        if let Some(window) = web_sys::window() {
                            let _ = window.request_animation_frame(cb.as_ref().unchecked_ref());
                        }
                        cb.forget();
                    }
                }
            }
            Err(err) => {
                error_msg.set(Some(err.to_string()));
            }
        });
    });

    Effect::new(move |_| {
        reporting_options_resource.get().map(|result| match result {
            Ok(options) => {
                active_work_items.set(options.active_items);
                loaded_reporting_options.set(true);
            }
            Err(err) => {
                error_msg.set(Some(err.to_string()));
            }
        });
    });

    let on_dialog_keydown = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "Tab" => {
            #[cfg(feature = "hydrate")]
            {
                use leptos::wasm_bindgen::JsCast;

                let Some(dialog) = dialog_ref.get() else {
                    return;
                };
                let mut focusables = Vec::<web_sys::HtmlElement>::new();

                for class_name in ["settings-lang-btn", "settings-input"] {
                    let nodes = dialog.get_elements_by_class_name(class_name);
                    for idx in 0..nodes.length() {
                        let Some(node) = nodes.item(idx) else {
                            continue;
                        };
                        let Some(el) = node.dyn_ref::<web_sys::HtmlElement>() else {
                            continue;
                        };
                        if el.offset_parent().is_some() {
                            focusables.push(el.clone());
                        }
                    }
                }
                let inputs = dialog.get_elements_by_tag_name("input");
                for idx in 0..inputs.length() {
                    let Some(node) = inputs.item(idx) else {
                        continue;
                    };
                    let Some(input) = node.dyn_ref::<web_sys::HtmlInputElement>() else {
                        continue;
                    };
                    if input.type_() == "checkbox"
                        && !input.disabled()
                        && input.offset_parent().is_some()
                    {
                        let el: &web_sys::HtmlElement = input.unchecked_ref();
                        if !focusables.iter().any(|existing| {
                            let existing_node: &web_sys::Node = existing.unchecked_ref();
                            let input_node: &web_sys::Node = el.unchecked_ref();
                            existing_node.is_same_node(Some(input_node))
                        }) {
                            focusables.push(el.clone());
                        }
                    }
                }
                for class_name in ["btn-ok", "btn-cancel"] {
                    let nodes = dialog.get_elements_by_class_name(class_name);
                    for idx in 0..nodes.length() {
                        let Some(node) = nodes.item(idx) else {
                            continue;
                        };
                        let Some(el) = node.dyn_ref::<web_sys::HtmlElement>() else {
                            continue;
                        };
                        if el.offset_parent().is_some() {
                            focusables.push(el.clone());
                        }
                    }
                }
                if focusables.is_empty() {
                    return;
                }
                let Some(window) = web_sys::window() else {
                    return;
                };
                let Some(document) = window.document() else {
                    return;
                };
                let active = document.active_element();
                let current_idx = active.and_then(|active| {
                    focusables.iter().position(|el| {
                        let active_node: &web_sys::Node = active.unchecked_ref();
                        let el_node: &web_sys::Node = el.unchecked_ref();
                        el_node.is_same_node(Some(active_node))
                    })
                });
                let next_idx = if ev.shift_key() {
                    current_idx.map(|idx| idx.checked_sub(1).unwrap_or(focusables.len() - 1))
                } else {
                    current_idx.map(|idx| (idx + 1) % focusables.len())
                }
                .unwrap_or(0);
                ev.prevent_default();
                let _ = focusables[next_idx].focus();
            }
        }
        "Escape" => {
            ev.prevent_default();
            on_cancel.run(());
        }
        "Enter" => {
            ev.prevent_default();
            if form_valid.get() {
                save_action.dispatch(());
            }
        }
        _ => {}
    };

    view! {
        <div class="settings-overlay">
            <div class="settings-backdrop" on:click=move |_| on_cancel.run(())></div>
            <div class="settings-dialog" node_ref=dialog_ref tabindex="-1" on:keydown=on_dialog_keydown>
                <h2>{move || i18n.get().t(keys::SETTINGS_TITLE)}</h2>

                <Suspense fallback=move || view! { <p>{move || i18n.get().t(keys::LOADING_SETTINGS)}</p> }>
                    <SettingsGroup title=title_language.clone()>
                        <div class="lang-dropdown">
                            <button class="lang-btn settings-lang-btn settings-initial-focus" on:click=move |_| lang_menu_open.update(|open| *open = !*open)>
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
                        <div class="settings-range-field" class:settings-field-invalid=move || hpw_error.get().is_some()>
                            <div class="settings-range-row">
                                <input
                                    type="range"
                                    step="1"
                                    min="16"
                                    max="42"
                                    prop:value={move || hours_per_week.get().to_string()}
                                    on:input=move |ev| {
                                        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
                                            hours_per_week.set(value);
                                        }
                                    }
                                    class="settings-input"
                                />
                                <span class="settings-range-value">{move || format!("{:.0}", hours_per_week.get())}</span>
                            </div>
                            {move || {
                                hpw_error.get().map(|message| {
                                    view! {
                                        <span class="settings-error-indicator" title={message}>!</span>
                                    }
                                })
                            }}
                        </div>
                        <label>{lbl_hpd.clone()}":"</label>
                        <div class="settings-range-field" class:settings-field-invalid=move || hpd_error.get().is_some()>
                            <div class="settings-range-row">
                                <input
                                    type="range"
                                    step="0.5"
                                    min="4"
                                    max="9.5"
                                    prop:value={move || hours_per_day.get().to_string()}
                                    on:input=move |ev| {
                                        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
                                            hours_per_day.set(value);
                                        }
                                    }
                                    class="settings-input"
                                />
                                <span class="settings-range-value">{move || format!("{:.1}", hours_per_day.get())}</span>
                            </div>
                            {move || {
                                hpd_error.get().map(|message| {
                                    view! {
                                        <span class="settings-error-indicator" title={message}>!</span>
                                    }
                                })
                            }}
                        </div>
                    </SettingsGroup>
                    <SettingsGroup title=title_reporting.clone()>
                        <MultiSelectField
                            label={Signal::derive(move || i18n.get().t(keys::NON_BILLABLE_PROJECTS))}
                            selected={non_billable_project_prefixes}
                            options={Signal::derive(move || non_billable_options.get())}
                            item_titles={Signal::derive(move || HashMap::new())}
                            select_value={non_billable_select_value}
                            add_placeholder={Signal::derive(move || i18n.get().t(keys::SETTINGS_ADD_ENTRY))}
                            remove_label={Signal::derive(move || i18n.get().t(keys::DELETE))}
                            error={Signal::derive(move || non_billable_error.get())}
                        />
                        <MultiSelectField
                            label={Signal::derive(move || i18n.get().t(keys::MEETINGS))}
                            selected={meeting_keys}
                            options={Signal::derive(move || meeting_options.get())}
                            item_titles={Signal::derive(move || issue_title_by_key.get())}
                            select_value={meetings_select_value}
                            add_placeholder={Signal::derive(move || i18n.get().t(keys::SETTINGS_ADD_ENTRY))}
                            remove_label={Signal::derive(move || i18n.get().t(keys::DELETE))}
                            error={Signal::derive(move || meetings_error.get())}
                        />
                        <MultiSelectField
                            label={Signal::derive(move || i18n.get().t(keys::LOCAL_HOLIDAYS))}
                            selected={local_holiday_keys}
                            options={Signal::derive(move || holiday_options.get())}
                            item_titles={Signal::derive(move || issue_title_by_key.get())}
                            select_value={holidays_select_value}
                            add_placeholder={Signal::derive(move || i18n.get().t(keys::SETTINGS_ADD_ENTRY))}
                            remove_label={Signal::derive(move || i18n.get().t(keys::DELETE))}
                            error={Signal::derive(move || holidays_error.get())}
                        />
                        <MultiSelectField
                            label={Signal::derive(move || i18n.get().t(keys::PLANNED_TIME_OFF))}
                            selected={planned_time_off_keys}
                            options={Signal::derive(move || pto_options.get())}
                            item_titles={Signal::derive(move || issue_title_by_key.get())}
                            select_value={pto_select_value}
                            add_placeholder={Signal::derive(move || i18n.get().t(keys::SETTINGS_ADD_ENTRY))}
                            remove_label={Signal::derive(move || i18n.get().t(keys::DELETE))}
                            error={Signal::derive(move || pto_error.get())}
                        />
                        <MultiSelectField
                            label={Signal::derive(move || i18n.get().t(keys::STUDY))}
                            selected={study_keys}
                            options={Signal::derive(move || study_options.get())}
                            item_titles={Signal::derive(move || issue_title_by_key.get())}
                            select_value={study_select_value}
                            add_placeholder={Signal::derive(move || i18n.get().t(keys::SETTINGS_ADD_ENTRY))}
                            remove_label={Signal::derive(move || i18n.get().t(keys::DELETE))}
                            error={Signal::derive(move || study_error.get())}
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

                <div class="dialog-buttons">
                    <button
                        class="btn-ok"
                        disabled=move || !form_valid.get() || save_action.pending().get()
                        on:click=move |_| { save_action.dispatch(()); }
                    >
                        {move || i18n.get().t(keys::SAVE)}
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
