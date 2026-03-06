use leptos::prelude::*;

#[component]
pub fn FolderField(
    #[prop(into)] label: String,
    #[prop(into)] placeholder: String,
    value: RwSignal<String>,
) -> impl IntoView {
    view! {
        <label>{label}":"</label>
        <input
            type="text"
            placeholder={placeholder}
            prop:value={move || value.get()}
            on:input=move |ev| {
                value.set(event_target_value(&ev));
            }
            class="settings-input"
        />
    }
}
