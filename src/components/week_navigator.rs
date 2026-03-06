use crate::components::popup_flush::use_popup_flush;
use crate::i18n::{I18n, keys};
use chrono::{Datelike, Local, NaiveDate};
use leptos::prelude::*;

/// Compute the Monday of the week containing `date`.
pub fn week_monday(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

#[component]
pub fn WeekNavigator(
    /// The currently selected Monday (start of selected week).
    selected_monday: RwSignal<NaiveDate>,
) -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().expect("I18n context");
    let flush_mgr = use_popup_flush();

    let today_monday = week_monday(Local::now().date_naive());

    let go_prev = {
        let flush_mgr = flush_mgr.clone();
        move |_| {
            flush_mgr.flush_all_then(move || {
                selected_monday.set(selected_monday.get() - chrono::Duration::weeks(1));
            });
        }
    };
    let go_next = {
        let flush_mgr = flush_mgr.clone();
        move |_| {
            flush_mgr.flush_all_then(move || {
                selected_monday.set(selected_monday.get() + chrono::Duration::weeks(1));
            });
        }
    };
    let on_date_change = {
        let flush_mgr = flush_mgr.clone();
        move |ev: leptos::ev::Event| {
            let value = event_target_value(&ev);
            if let Ok(date) = NaiveDate::parse_from_str(&value, "%Y-%m-%d") {
                let monday = week_monday(date);
                flush_mgr.flush_all_then(move || {
                    selected_monday.set(monday);
                });
            }
        }
    };

    // Show "Today" button only when not viewing the current week
    let show_today = move || selected_monday.get() != today_monday;

    // Clone flush_mgr for the Today button closure (which is re-created each
    // time `show_today` is re-evaluated).
    let flush_mgr_today = flush_mgr.clone();

    view! {
        <div class="week-navigator">
            <button class="nav-btn" on:click=go_prev title=move || i18n.get().t(keys::PREVIOUS_WEEK)>
                "\u{25C0}"
            </button>

            <input
                type="date"
                class="nav-date"
                prop:value={move || selected_monday.get().format("%Y-%m-%d").to_string()}
                on:change=on_date_change
            />

            <button class="nav-btn" on:click=go_next title=move || i18n.get().t(keys::NEXT_WEEK)>
                "\u{25B6}"
            </button>

            {move || {
                let flush_mgr = flush_mgr_today.clone();
                show_today().then(move || {
                    let go_today = move |_| {
                        flush_mgr.flush_all_then(move || {
                            selected_monday.set(week_monday(Local::now().date_naive()));
                        });
                    };
                    view! {
                        <button class="nav-btn nav-today" on:click=go_today>
                            {move || i18n.get().t(keys::TODAY)}
                        </button>
                    }
                })
            }}
        </div>
    }
}
