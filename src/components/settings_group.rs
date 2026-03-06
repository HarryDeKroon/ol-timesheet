use leptos::prelude::*;

#[component]
pub fn SettingsGroup(
    #[prop(into)] title: String,
    #[prop(optional)] disabled: bool,
    children: Children,
) -> impl IntoView {
    view! {
        <fieldset class="settings-group" disabled=disabled>
            <legend>{title}</legend>
            <div class="group-content">
                {children()}
            </div>
        </fieldset>
    }
}
