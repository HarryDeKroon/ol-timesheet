use leptos::prelude::*;

#[component]
pub fn PasswordField(
    #[prop(into)] label: String,
    #[prop(into)] placeholder: String,
    #[prop(optional)] link_url: Option<String>,
    value: RwSignal<String>,
    #[prop(optional)] disabled: bool,
) -> impl IntoView {
    let password_label = if let Some(url) = link_url {
        view! {
            <label>
                <a href={url} target="_blank" rel="noopener noreferrer">{label}</a>":"
            </label>
        }
        .into_any()
    } else {
        view! { <label>{label}":"</label> }.into_any()
    };

    view! {
        {password_label}
        <input
            type="password"
            placeholder={placeholder}
            prop:value={move || value.get()}
            on:input=move |ev| {
                value.set(event_target_value(&ev));
            }
            class="settings-input"
            disabled={disabled}
        />
    }
}
