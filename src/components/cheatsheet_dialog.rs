use crate::i18n::{I18n, keys};
use crate::model::CustomAction;
use leptos::prelude::*;

/// A single shortcut row: the literal key combination (never translated) and
/// the translated description of the action it performs.
struct Shortcut {
    combo: &'static str,
    action: String,
}

struct Section {
    title: String,
    shortcuts: Vec<Shortcut>,
}

fn shortcut(combo: &'static str, action: String) -> Shortcut {
    Shortcut { combo, action }
}

fn custom_action_shortcuts(actions: &[CustomAction], i18n: &I18n) -> Vec<Shortcut> {
    const COMBOS: [&str; 5] = ["Alt+1", "Alt+2", "Alt+3", "Alt+4", "Alt+5"];
    if actions.is_empty() {
        return vec![shortcut(
            "Alt+1 \u{2026} Alt+5",
            i18n.t(keys::CHEATSHEET_NO_CUSTOM_ACTIONS),
        )];
    }
    actions
        .iter()
        .zip(COMBOS)
        .map(|(action, combo)| shortcut(combo, crate::components::timesheet_view::custom_action_title(action)))
        .collect()
}

fn timesheet_sections(i18n: &I18n, actions: &[CustomAction]) -> Vec<Section> {
    vec![
        Section {
            title: i18n.t(keys::CHEATSHEET_SECTION_GENERAL),
            shortcuts: vec![
                shortcut("?", i18n.t(keys::CHEATSHEET_SHOW)),
                shortcut("Esc", i18n.t(keys::CHEATSHEET_CLOSE_DIALOG)),
                shortcut("Alt+S", i18n.t(keys::CHEATSHEET_OPEN_SETTINGS)),
                shortcut("Alt+R", i18n.t(keys::CHEATSHEET_OPEN_REPORT)),
                shortcut("Alt+F", i18n.t(keys::CHEATSHEET_REFRESH)),
                shortcut("Alt+A", i18n.t(keys::CHEATSHEET_ANONYMOUS_TIMER)),
                shortcut("Alt+X", i18n.t(keys::CHEATSHEET_LOGOUT)),
            ],
        },
        Section {
            title: i18n.t(keys::CHEATSHEET_SECTION_NAVIGATION),
            shortcuts: vec![
                shortcut("Alt+P", i18n.t(keys::CHEATSHEET_PREVIOUS_WEEK)),
                shortcut("Alt+N", i18n.t(keys::CHEATSHEET_NEXT_WEEK)),
                shortcut("Alt+T", i18n.t(keys::CHEATSHEET_TODAY)),
                shortcut("Alt+D", i18n.t(keys::CHEATSHEET_PICK_DATE)),
                shortcut("Alt+L", i18n.t(keys::CHEATSHEET_FOCUS_LAST_CELL)),
            ],
        },
        Section {
            title: i18n.t(keys::CHEATSHEET_SECTION_GRID),
            shortcuts: vec![
                shortcut("\u{2190} \u{2191} \u{2192} \u{2193}", i18n.t(keys::CHEATSHEET_GRID_MOVE)),
                shortcut("Enter", i18n.t(keys::CHEATSHEET_GRID_OPEN)),
                shortcut("0 \u{2026} 9", i18n.t(keys::CHEATSHEET_GRID_DIGIT)),
                shortcut("A \u{2026} Z", i18n.t(keys::CHEATSHEET_GRID_LETTER)),
            ],
        },
        Section {
            title: i18n.t(keys::CHEATSHEET_SECTION_POPUP),
            shortcuts: vec![
                shortcut("Ctrl+Enter", i18n.t(keys::CHEATSHEET_POPUP_SAVE)),
                shortcut("Esc", i18n.t(keys::CHEATSHEET_POPUP_CLOSE)),
                shortcut("Alt+M", i18n.t(keys::CHEATSHEET_POPUP_MOVE)),
                shortcut("Alt+I", i18n.t(keys::CHEATSHEET_POPUP_IMPORT)),
                shortcut("Alt+T", i18n.t(keys::CHEATSHEET_POPUP_TIMER)),
            ],
        },
        Section {
            title: i18n.t(keys::CHEATSHEET_SECTION_POPUP_MOVE),
            shortcuts: vec![
                shortcut("\u{2190} \u{2191} \u{2192} \u{2193}", i18n.t(keys::CHEATSHEET_MOVE_ARROWS)),
                shortcut(
                    "Shift+\u{2190} \u{2191} \u{2192} \u{2193}",
                    i18n.t(keys::CHEATSHEET_MOVE_ARROWS_FINE),
                ),
                shortcut("Enter / Alt+M", i18n.t(keys::CHEATSHEET_MOVE_FINISH)),
                shortcut("Esc", i18n.t(keys::CHEATSHEET_MOVE_CANCEL)),
            ],
        },
        Section {
            title: i18n.t(keys::CUSTOM_ACTIONS),
            shortcuts: custom_action_shortcuts(actions, i18n),
        },
    ]
}

fn report_sections(i18n: &I18n) -> Vec<Section> {
    vec![
        Section {
            title: i18n.t(keys::CHEATSHEET_SECTION_GENERAL),
            shortcuts: vec![
                shortcut("?", i18n.t(keys::CHEATSHEET_SHOW)),
                shortcut("Esc", i18n.t(keys::CHEATSHEET_CLOSE_DIALOG)),
                shortcut("Alt+S", i18n.t(keys::CHEATSHEET_OPEN_SETTINGS)),
                shortcut("Alt+W", i18n.t(keys::CHEATSHEET_REPORT_BACK)),
                shortcut("Alt+X", i18n.t(keys::CHEATSHEET_LOGOUT)),
            ],
        },
        Section {
            title: i18n.t(keys::CHEATSHEET_SECTION_REPORT),
            shortcuts: vec![
                shortcut("Alt+P", i18n.t(keys::CHEATSHEET_REPORT_PREVIOUS)),
                shortcut("Alt+N", i18n.t(keys::CHEATSHEET_REPORT_NEXT)),
                shortcut("Alt+T", i18n.t(keys::CHEATSHEET_REPORT_TODAY)),
                shortcut("Alt+D", i18n.t(keys::CHEATSHEET_REPORT_PERIOD)),
            ],
        },
    ]
}

#[component]
pub fn CheatsheetDialog(
    /// Controls visibility; set to `false` to close the dialog.
    show: RwSignal<bool>,
    /// `true` while the report view is active, which swaps the listed entries.
    show_report: RwSignal<bool>,
    /// Configured custom actions, listed with their `Alt+<n>` shortcut.
    custom_actions: RwSignal<Vec<CustomAction>>,
) -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().unwrap_or_else(|| {
        log::error!("I18n context not provided in CheatsheetDialog, using English fallback");
        RwSignal::new(I18n::default())
    });

    let dialog_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    #[cfg(feature = "hydrate")]
    dialog_ref.on_load(move |dialog| {
        request_animation_frame(move || {
            if dialog.is_connected() {
                if let Err(err) = dialog.focus() {
                    log::error!("Failed to focus the cheatsheet dialog: {err:?}");
                }
            }
        });
    });

    let close = move || show.set(false);

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            ev.prevent_default();
            ev.stop_propagation();
            close();
        }
    };

    view! {
        <div class="settings-overlay cheatsheet-overlay">
            <div class="settings-backdrop" on:click=move |_| close()></div>
            <div
                class="settings-dialog cheatsheet-dialog"
                node_ref=dialog_ref
                tabindex="-1"
                role="dialog"
                aria-modal="true"
                aria-labelledby="cheatsheet-dialog-title"
                on:keydown=on_keydown
            >
                <div class="cheatsheet-header">
                    <h2 id="cheatsheet-dialog-title">
                        {move || i18n.get().t(keys::CHEATSHEET_TITLE)}
                    </h2>
                    <button
                        type="button"
                        class="popup-title-action popup-title-close cheatsheet-close"
                        on:click=move |_| close()
                        title=move || i18n.get().t(keys::CLOSE)
                        aria-label=move || i18n.get().t(keys::CLOSE)
                    >
                        {"\u{00D7}"}
                    </button>
                </div>
                <div class="cheatsheet-sections">
                    {move || {
                        let w = i18n.get();
                        let sections = if show_report.get() {
                            report_sections(&w)
                        } else {
                            timesheet_sections(&w, &custom_actions.get())
                        };
                        sections
                            .into_iter()
                            .map(|section| {
                                view! {
                                    <section class="cheatsheet-section">
                                        <h3 class="cheatsheet-section-title">{section.title}</h3>
                                        <dl class="cheatsheet-list">
                                            {section
                                                .shortcuts
                                                .into_iter()
                                                .map(|entry| {
                                                    view! {
                                                        <>
                                                            <dt class="cheatsheet-combo">
                                                                <kbd>{entry.combo}</kbd>
                                                            </dt>
                                                            <dd class="cheatsheet-action">{entry.action}</dd>
                                                        </>
                                                    }
                                                })
                                                .collect_view()}
                                        </dl>
                                    </section>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </div>
        </div>
    }
}
