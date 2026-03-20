use chrono::NaiveDate;
use std::collections::HashMap;
use std::sync::LazyLock;

/// All available translation keys.
/// Using constants prevents typos at compile time.
pub mod keys {
    pub const LOADING: &str = "loading";
    pub const LOADING_SETTINGS: &str = "loading_settings";
    pub const LOADING_TIMESHEET: &str = "loading_timesheet";
    pub const SETTINGS_TITLE: &str = "settings_title";
    pub const SAVE: &str = "save";
    pub const CANCEL: &str = "cancel";
    pub const CLOSE: &str = "close";
    pub const OK: &str = "ok";
    pub const EMAIL: &str = "email";
    pub const API_TOKEN: &str = "api_token";

    pub const USERNAME: &str = "username";
    pub const APP_PASSWORD: &str = "app_password";
    pub const PASSWORD: &str = "password";
    pub const REPOSITORY: &str = "repository";
    pub const HOURS_PER_WEEK: &str = "hours_per_week";
    pub const HOURS_PER_DAY: &str = "hours_per_day";
    pub const TIMESHEET_TITLE: &str = "timesheet_title";
    pub const TODAY: &str = "today";
    pub const WEEKEND: &str = "weekend";
    pub const WEEKEND_TITLE: &str = "weekend_title";
    pub const TOTAL: &str = "total";
    pub const DESCRIPTION: &str = "description";
    pub const HOURS: &str = "hours";
    pub const DELETE: &str = "delete";
    pub const ADD_NEW: &str = "add_new";
    pub const NO_ENTRIES: &str = "no_entries";
    pub const UPLAND_JIRA: &str = "upland_jira";
    pub const BITBUCKET: &str = "bitbucket";
    pub const OL_JIRA: &str = "ol_jira";
    pub const GIT_WORKSPACE: &str = "git_workspace";
    pub const PREFERENCES: &str = "preferences";
    pub const MON: &str = "mon";
    pub const TUE: &str = "tue";
    pub const WED: &str = "wed";
    pub const THU: &str = "thu";
    pub const FRI: &str = "fri";
    pub const SAT: &str = "sat";
    pub const SUN: &str = "sun";
    pub const SEARCH_WORK_ITEM: &str = "search_work_item";
    pub const REFRESH: &str = "refresh";
    pub const WEEK_ABBR: &str = "week_abbr";
    pub const DAY_ABBR: &str = "day_abbr";
    pub const HOUR_ABBR: &str = "hour_abbr";
    pub const MINUTE_ABBR: &str = "minute_abbr";
    // Placeholders and extra labels for settings dialog
    pub const EMAIL_PLACEHOLDER: &str = "email_placeholder";
    pub const API_TOKEN_PLACEHOLDER: &str = "api_token_placeholder";

    pub const USERNAME_PLACEHOLDER: &str = "username_placeholder";
    pub const APP_PASSWORD_PLACEHOLDER: &str = "app_password_placeholder";
    pub const OL_JIRA_USERNAME_PLACEHOLDER: &str = "ol_jira_username_placeholder";
    pub const PASSWORD_PLACEHOLDER: &str = "password_placeholder";
    pub const REPOSITORY_PLACEHOLDER: &str = "repository_placeholder";
    pub const GIT_POLL_INTERVAL: &str = "git_poll_interval";
    pub const CONNECTION_CONNECTED: &str = "connection_connected";
    pub const CONNECTION_SYNCING: &str = "connection_syncing";
    pub const CONNECTION_DISCONNECTED: &str = "connection_disconnected";
    pub const CONNECTION_UNAVAILABLE: &str = "connection_unavailable";
    pub const TIMER_START: &str = "timer_start";
    pub const TIMER_PAUSE: &str = "timer_pause";
    pub const TIMER_RESUME: &str = "timer_resume";
    pub const TIMER_STOP: &str = "timer_stop";
    pub const PREVIOUS_WEEK: &str = "previous_week";
    pub const NEXT_WEEK: &str = "next_week";
    pub const OPEN_IN_JIRA: &str = "open_in_jira";
    pub const OPEN_SETTINGS: &str = "open_settings";
    pub const REFRESH_CACHED: &str = "refresh_cached";
    pub const LOGOUT: &str = "logout";
    pub const BITBUCKET_DISABLED: &str = "bitbucket_disabled";
    pub const OL_JIRA_DISABLED: &str = "ol_jira_disabled";
    pub const ISSUE_ICON_ALT: &str = "issue_icon_alt";
}

type TranslationMap = HashMap<&'static str, HashMap<&'static str, &'static str>>;

static TRANSLATIONS: LazyLock<TranslationMap> = LazyLock::new(|| {
    let en: HashMap<&str, &str> = [
        (keys::LOADING, "Loading\u{2026}"),
        (keys::LOADING_SETTINGS, "Loading settings\u{2026}"),
        (keys::LOADING_TIMESHEET, "Loading timesheet\u{2026}"),
        (keys::SETTINGS_TITLE, "\u{2699} Settings"),
        (keys::SAVE, "Save"),
        (keys::CANCEL, "Cancel"),
        (keys::CLOSE, "Close"),
        (keys::OK, "OK"),
        (keys::EMAIL, "Email"),
        (keys::API_TOKEN, "API token"),
        (keys::USERNAME, "Username"),
        (keys::APP_PASSWORD, "App password"),
        (keys::PASSWORD, "Password"),
        (keys::REPOSITORY, "Repository"),
        (keys::HOURS_PER_WEEK, "Hours per week"),
        (keys::HOURS_PER_DAY, "Hours per day"),
        (keys::TIMESHEET_TITLE, "Timesheet"),
        (keys::TODAY, "Today"),
        (keys::WEEKEND, "W/E"),
        (keys::WEEKEND_TITLE, "Weekend"),
        (keys::TOTAL, "Total"),
        (keys::DESCRIPTION, "Description"),
        (keys::HOURS, "Hours"),
        (keys::DELETE, "Delete"),
        (keys::ADD_NEW, "Add new"),
        (keys::NO_ENTRIES, "No entries found"),
        (keys::UPLAND_JIRA, "Upland Jira"),
        (keys::BITBUCKET, "Bitbucket"),
        (keys::OL_JIRA, "Objectif Lune Jira"),
        (keys::GIT_WORKSPACE, "Git workspace"),
        (keys::PREFERENCES, "Preferences"),
        (keys::MON, "Mon"),
        (keys::TUE, "Tue"),
        (keys::WED, "Wed"),
        (keys::THU, "Thu"),
        (keys::FRI, "Fri"),
        (keys::SAT, "Sat"),
        (keys::SUN, "Sun"),
        (keys::SEARCH_WORK_ITEM, "Search work item\u{2026}"),
        (keys::REFRESH, "\u{1F504} Refresh"),
        (keys::WEEK_ABBR, "w"),
        (keys::DAY_ABBR, "d"),
        (keys::HOUR_ABBR, "h"),
        (keys::MINUTE_ABBR, "m"),
        // Placeholders and extra labels for settings dialog
        (keys::EMAIL_PLACEHOLDER, "user@uplandsoftware.com"),
        (keys::API_TOKEN_PLACEHOLDER, "012345678901234567890123"),
        (keys::USERNAME_PLACEHOLDER, "user name"),
        (keys::APP_PASSWORD_PLACEHOLDER, "012345678901234567890123"),
        (keys::OL_JIRA_USERNAME_PLACEHOLDER, "Domain username"),
        (keys::PASSWORD_PLACEHOLDER, "Domain password"),
        (keys::REPOSITORY_PLACEHOLDER, "Repository folder"),
        (keys::GIT_POLL_INTERVAL, "Git poll interval (minutes):"),
        (keys::CONNECTION_CONNECTED, "Connected"),
        (keys::CONNECTION_SYNCING, "Syncing…"),
        (keys::CONNECTION_DISCONNECTED, "Disconnected"),
        (keys::CONNECTION_UNAVAILABLE, "Connection unavailable"),
        (keys::TIMER_START, "Start timer"),
        (keys::TIMER_PAUSE, "Pause timer"),
        (keys::TIMER_RESUME, "Resume timer"),
        (keys::TIMER_STOP, "Stop timer"),
        (keys::PREVIOUS_WEEK, "Previous week"),
        (keys::NEXT_WEEK, "Next week"),
        (keys::OPEN_IN_JIRA, "Open in Jira"),
        (keys::OPEN_SETTINGS, "Open settings"),
        (keys::REFRESH_CACHED, "Refresh cached work items"),
        (keys::LOGOUT, "Log out"),
        (keys::BITBUCKET_DISABLED, "Bitbucket integration is currently disabled due to API deprecation."),
        (keys::OL_JIRA_DISABLED, "OL Jira integration is disabled because over three years have passed since the last update."),
        (keys::ISSUE_ICON_ALT, "Issue icon"),
    ]
    .into_iter()
    .collect();

    let fr: HashMap<&str, &str> = [
        (keys::LOADING, "Chargement\u{2026}"),
        (
            keys::LOADING_SETTINGS,
            "Chargement des param\u{00E8}tres\u{2026}",
        ),
        (
            keys::LOADING_TIMESHEET,
            "Chargement de la feuille de temps\u{2026}",
        ),
        (keys::SETTINGS_TITLE, "\u{2699} Param\u{00E8}tres"),
        (keys::SAVE, "Enregistrer"),
        (keys::CANCEL, "Annuler"),
        (keys::CLOSE, "Fermer"),
        (keys::OK, "OK"),
        (keys::EMAIL, "Courriel"),
        (keys::API_TOKEN, "Jeton API"),
        (keys::USERNAME, "Nom d\u{2019}utilisateur"),
        (keys::APP_PASSWORD, "Mot de passe d\u{2019}application"),
        (keys::PASSWORD, "Mot de passe"),
        (keys::REPOSITORY, "D\u{00E9}p\u{00F4}t"),
        (keys::HOURS_PER_WEEK, "Heures par semaine"),
        (keys::HOURS_PER_DAY, "Heures par jour"),
        (keys::TIMESHEET_TITLE, "Feuille de temps"),
        (keys::TODAY, "Aujourd\u{2019}hui"),
        (keys::WEEKEND, "F/S"),
        (keys::WEEKEND_TITLE, "Fin-de-semaine"),
        (keys::TOTAL, "Total"),
        (keys::DESCRIPTION, "Description"),
        (keys::HOURS, "Heures"),
        (keys::DELETE, "Supprimer"),
        (keys::ADD_NEW, "Ajouter"),
        (keys::NO_ENTRIES, "Aucune entr\u{00E9}e trouv\u{00E9}e"),
        (keys::UPLAND_JIRA, "Upland Jira"),
        (keys::BITBUCKET, "Bitbucket"),
        (keys::OL_JIRA, "Objectif Lune Jira"),
        (keys::GIT_WORKSPACE, "Espace de travail Git"),
        (keys::PREFERENCES, "Pr\u{00E9}f\u{00E9}rences"),
        (keys::MON, "Lun"),
        (keys::TUE, "Mar"),
        (keys::WED, "Mer"),
        (keys::THU, "Jeu"),
        (keys::FRI, "Ven"),
        (keys::SAT, "Sam"),
        (keys::SUN, "Dim"),
        (
            keys::SEARCH_WORK_ITEM,
            "Rechercher un \u{00E9}l\u{00E9}ment\u{2026}",
        ),
        (keys::REFRESH, "\u{1F504} Rafra\u{00EE}chir"),
        (keys::WEEK_ABBR, "s"),
        (keys::DAY_ABBR, "j"),
        (keys::HOUR_ABBR, "h"),
        (keys::MINUTE_ABBR, "m"),
        // Placeholders and extra labels for settings dialog
        (keys::EMAIL_PLACEHOLDER, "utilisateur@uplandsoftware.com"),
        (keys::API_TOKEN_PLACEHOLDER, "012345678901234567890123"),
        (keys::USERNAME_PLACEHOLDER, "nom d'utilisateur"),
        (keys::APP_PASSWORD_PLACEHOLDER, "012345678901234567890123"),
        (
            keys::OL_JIRA_USERNAME_PLACEHOLDER,
            "Nom d'utilisateur du domaine",
        ),
        (keys::PASSWORD_PLACEHOLDER, "Mot de passe du domaine"),
        (keys::REPOSITORY_PLACEHOLDER, "Dossier du dépôt"),
        (
            keys::GIT_POLL_INTERVAL,
            "Intervalle de poll Git (minutes) :",
        ),
        (keys::CONNECTION_CONNECTED, "Connecté"),
        (keys::CONNECTION_SYNCING, "Synchronisation…"),
        (keys::CONNECTION_DISCONNECTED, "Déconnecté"),
        (keys::CONNECTION_UNAVAILABLE, "Connexion indisponible"),
        (keys::TIMER_START, "Démarrer le minuteur"),
        (keys::TIMER_PAUSE, "Mettre en pause"),
        (keys::TIMER_RESUME, "Reprendre le minuteur"),
        (keys::TIMER_STOP, "Arrêter le minuteur"),
        (keys::PREVIOUS_WEEK, "Semaine précédente"),
        (keys::NEXT_WEEK, "Semaine suivante"),
        (keys::OPEN_IN_JIRA, "Ouvrir dans Jira"),
        (keys::OPEN_SETTINGS, "Ouvrir les paramètres"),
        (keys::REFRESH_CACHED, "Rafraîchir les éléments en cache"),
        (keys::LOGOUT, "Se déconnecter"),
        (keys::BITBUCKET_DISABLED, "L\u{2019}intégration Bitbucket est actuellement désactivée en raison de la dépréciation de l\u{2019}API."),
        (keys::OL_JIRA_DISABLED, "L\u{2019}intégration OL Jira est désactivée car plus de trois ans se sont écoulés depuis la dernière mise à jour."),
        (keys::ISSUE_ICON_ALT, "Icône du ticket"),
    ]
    .into_iter()
    .collect();

    let nl: HashMap<&str, &str> = [
        (keys::LOADING, "Laden\u{2026}"),
        (keys::LOADING_SETTINGS, "Instellingen laden\u{2026}"),
        (keys::LOADING_TIMESHEET, "Urenstaat laden\u{2026}"),
        (keys::SETTINGS_TITLE, "\u{2699} Instellingen"),
        (keys::SAVE, "Opslaan"),
        (keys::CANCEL, "Annuleren"),
        (keys::CLOSE, "Sluiten"),
        (keys::OK, "OK"),
        (keys::EMAIL, "E-mail"),
        (keys::API_TOKEN, "API-token"),
        (keys::USERNAME, "Gebruikersnaam"),
        (keys::APP_PASSWORD, "App-wachtwoord"),
        (keys::PASSWORD, "Wachtwoord"),
        (keys::REPOSITORY, "Repository"),
        (keys::HOURS_PER_WEEK, "Uren per week"),
        (keys::HOURS_PER_DAY, "Uren per dag"),
        (keys::TIMESHEET_TITLE, "Urenstaat"),
        (keys::TODAY, "Vandaag"),
        (keys::WEEKEND, "W/E"),
        (keys::WEEKEND_TITLE, "Weekend"),
        (keys::TOTAL, "Totaal"),
        (keys::DESCRIPTION, "Beschrijving"),
        (keys::HOURS, "Uren"),
        (keys::DELETE, "Verwijderen"),
        (keys::ADD_NEW, "Nieuw toevoegen"),
        (keys::NO_ENTRIES, "Geen items gevonden"),
        (keys::UPLAND_JIRA, "Upland Jira"),
        (keys::BITBUCKET, "Bitbucket"),
        (keys::OL_JIRA, "Objectif Lune Jira"),
        (keys::GIT_WORKSPACE, "Git-werkruimte"),
        (keys::PREFERENCES, "Voorkeuren"),
        (keys::MON, "Ma"),
        (keys::TUE, "Di"),
        (keys::WED, "Wo"),
        (keys::THU, "Do"),
        (keys::FRI, "Vr"),
        (keys::SAT, "Za"),
        (keys::SUN, "Zo"),
        (keys::SEARCH_WORK_ITEM, "Zoek werkitem\u{2026}"),
        (keys::REFRESH, "\u{1F504} Vernieuwen"),
        (keys::WEEK_ABBR, "w"),
        (keys::DAY_ABBR, "d"),
        (keys::HOUR_ABBR, "u"),
        (keys::MINUTE_ABBR, "m"),
        // Placeholders and extra labels for settings dialog
        (keys::EMAIL_PLACEHOLDER, "gebruiker@uplandsoftware.com"),
        (keys::API_TOKEN_PLACEHOLDER, "012345678901234567890123"),
        (keys::USERNAME_PLACEHOLDER, "gebruikersnaam"),
        (keys::APP_PASSWORD_PLACEHOLDER, "012345678901234567890123"),
        (keys::OL_JIRA_USERNAME_PLACEHOLDER, "Domeingebruikersnaam"),
        (keys::PASSWORD_PLACEHOLDER, "Domeinwachtwoord"),
        (keys::REPOSITORY_PLACEHOLDER, "Repositorymap"),
        (keys::GIT_POLL_INTERVAL, "Git-poll-interval (minuten):"),
        (keys::CONNECTION_CONNECTED, "Verbonden"),
        (keys::CONNECTION_SYNCING, "Synchroniseren…"),
        (keys::CONNECTION_DISCONNECTED, "Verbinding verbroken"),
        (keys::CONNECTION_UNAVAILABLE, "Verbinding niet beschikbaar"),
        (keys::TIMER_START, "Timer starten"),
        (keys::TIMER_PAUSE, "Timer pauzeren"),
        (keys::TIMER_RESUME, "Timer hervatten"),
        (keys::TIMER_STOP, "Timer stoppen"),
        (keys::PREVIOUS_WEEK, "Vorige week"),
        (keys::NEXT_WEEK, "Volgende week"),
        (keys::OPEN_IN_JIRA, "Openen in Jira"),
        (keys::OPEN_SETTINGS, "Instellingen openen"),
        (keys::REFRESH_CACHED, "Werkitems in cache vernieuwen"),
        (keys::LOGOUT, "Uitloggen"),
        (keys::BITBUCKET_DISABLED, "Bitbucket-integratie is momenteel uitgeschakeld vanwege API-afschaffing."),
        (keys::OL_JIRA_DISABLED, "OL Jira-integratie is uitgeschakeld omdat er meer dan drie jaar zijn verstreken sinds de laatste update."),
        (keys::ISSUE_ICON_ALT, "Ticketpictogram"),
    ]
    .into_iter()
    .collect();

    [("en", en), ("fr", fr), ("nl", nl)].into_iter().collect()
});

static DEFAULT_TRANSLATIONS: LazyLock<&HashMap<&str, &str>> =
    LazyLock::new(|| TRANSLATIONS.get("en").unwrap());

/// Internationalisation context provided to all components.
#[derive(Clone, Debug, PartialEq)]
pub struct I18n {
    /// The two-letter language code (e.g. "en", "fr").
    pub lang: String,
    /// The decimal separator for the current locale.
    pub decimal_separator: char,
}

impl I18n {
    /// Create a new I18n instance from a browser locale string (e.g. "en-US", "fr-FR").
    pub fn new(locale: &str) -> Self {
        let lang = locale.split('-').next().unwrap_or("en").to_lowercase();
        let decimal_separator = match lang.as_str() {
            "fr" | "de" | "nl" | "es" | "it" | "pt" | "da" | "fi" | "nb" | "sv" | "pl" | "cs"
            | "sk" | "hu" | "ro" | "bg" | "hr" | "sl" | "el" | "tr" | "ru" | "uk" => ',',
            _ => '.',
        };
        Self {
            lang,
            decimal_separator,
        }
    }

    pub fn tr<'t>(&self, key: &'t str) -> &'t str {
        TRANSLATIONS
            .get(self.lang.as_str())
            .and_then(|lang_map| lang_map.get(key).copied())
            .or_else(|| DEFAULT_TRANSLATIONS.get(key).map(|s| *s))
            .unwrap_or(key)
    }

    /// Look up a translation by key. Falls back to English, then to "[key]".
    pub fn t(&self, key: &str) -> String {
        TRANSLATIONS
            .get(self.lang.as_str())
            .and_then(|lang_map| lang_map.get(key).copied())
            .or_else(|| {
                TRANSLATIONS
                    .get("en")
                    .and_then(|lang_map| lang_map.get(key).copied())
            })
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("[{}]", key))
    }

    /// Format a full date according to the current locale.
    ///
    /// - `"fr"` → `"31/12/2025"` (dd/mm/yyyy)
    /// - `"nl"` → `"31-12-2025"` (dd-mm-yyyy)
    /// - other  → `"2025-12-31"` (yyyy-mm-dd, ISO)
    pub fn format_date(&self, date: &NaiveDate) -> String {
        match self.lang.as_str() {
            "fr" => date.format("%d/%m/%Y").to_string(),
            "nl" => date.format("%d-%m-%Y").to_string(),
            _ => date.format("%Y-%m-%d").to_string(),
        }
    }

    /// Format a short day/month string according to the current locale.
    ///
    /// - `"fr"` → `"31/12"` (dd/mm)
    /// - `"nl"` → `"31-12"` (dd-mm)
    /// - other  → `"12/31"` (mm/dd)
    pub fn format_day_month(&self, date: &NaiveDate) -> String {
        match self.lang.as_str() {
            "fr" => date.format("%d/%m").to_string(),
            "nl" => date.format("%d-%m").to_string(),
            _ => date.format("%m/%d").to_string(),
        }
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new("en")
    }
}
