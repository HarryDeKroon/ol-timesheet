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
    pub const OTHER: &str = "other";
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
    pub const OPEN_COMMIT_IN_BITBUCKET: &str = "open_commit_in_bitbucket";
    pub const OPEN_PR_IN_BITBUCKET: &str = "open_pr_in_bitbucket";
    pub const OPEN_SETTINGS: &str = "open_settings";
    pub const REFRESH_CACHED: &str = "refresh_cached";
    pub const FORCE_PERIODIC_REFRESH: &str = "force_periodic_refresh";
    pub const USER_REPORT: &str = "user_report";
    pub const LOGOUT: &str = "logout";
    pub const OL_JIRA_DISABLED: &str = "ol_jira_disabled";
    pub const ISSUE_ICON_ALT: &str = "issue_icon_alt";
    pub const REPORTING: &str = "reporting";
    pub const NON_BILLABLE_PROJECTS: &str = "non_billable_projects";
    pub const MEETINGS: &str = "meetings";
    pub const LOCAL_HOLIDAYS: &str = "local_holidays";
    pub const PLANNED_TIME_OFF: &str = "planned_time_off";
    pub const STUDY: &str = "study";
    pub const LIST_INPUT_HINT: &str = "list_input_hint";
    pub const REPORT_PERIOD: &str = "report_period";
    pub const REPORT_PERIOD_WEEK: &str = "report_period_week";
    pub const REPORT_PERIOD_MONTH: &str = "report_period_month";
    pub const BILLABLE: &str = "billable";
    pub const NON_BILLABLE: &str = "non_billable";
    pub const PLANNED_TIME_OFF_HOURS: &str = "planned_time_off_hours";
    pub const REPORT_NO_DATA: &str = "report_no_data";
    pub const REPORT_PREVIOUS: &str = "report_previous";
    pub const REPORT_NEXT: &str = "report_next";
    pub const REPORT_LOADING: &str = "report_loading";
    pub const REPORT_GRAND_TOTAL: &str = "report_grand_total";
    pub const REPORT_YTD_SCOPE: &str = "report_ytd_scope";
    pub const REPORT_YEAR_SCOPE: &str = "report_year_scope";
    pub const REPORT_PTO_TOTAL: &str = "report_pto_total";
    pub const LIVE_REFRESH_APPLIED: &str = "live_refresh_applied";
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
        (keys::OTHER, "Other"),
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
        (keys::OPEN_COMMIT_IN_BITBUCKET, "Open commit in Bitbucket"),
        (keys::OPEN_PR_IN_BITBUCKET, "Open pull request in Bitbucket"),
        (keys::OPEN_SETTINGS, "Open settings"),
        (keys::REFRESH_CACHED, "Refresh cached work items"),
        (keys::FORCE_PERIODIC_REFRESH, "Force periodic refresh"),
        (keys::USER_REPORT, "User report"),
        (keys::LOGOUT, "Log out"),
        (keys::OL_JIRA_DISABLED, "OL Jira integration is disabled because over three years have passed since the last update."),
        (keys::ISSUE_ICON_ALT, "Issue icon"),
        (keys::REPORTING, "Reporting"),
        (keys::NON_BILLABLE_PROJECTS, "Non-billable project(s)"),
        (keys::MEETINGS, "Meetings"),
        (keys::LOCAL_HOLIDAYS, "Local holidays"),
        (keys::PLANNED_TIME_OFF, "Planned time off"),
        (keys::STUDY, "Study"),
        (
            keys::LIST_INPUT_HINT,
            "Comma, semicolon, or newline separated values",
        ),
        (keys::REPORT_PERIOD, "Period"),
        (keys::REPORT_PERIOD_WEEK, "Month"),
        (keys::REPORT_PERIOD_MONTH, "Year"),
        (keys::BILLABLE, "Billable"),
        (keys::NON_BILLABLE, "Non-billable"),
        (keys::PLANNED_TIME_OFF_HOURS, "Planned time off (hours)"),
        (keys::REPORT_NO_DATA, "No data for selected period"),
        (keys::REPORT_PREVIOUS, "Previous period"),
        (keys::REPORT_NEXT, "Next period"),
        (keys::REPORT_LOADING, "Loading report…"),
        (keys::REPORT_GRAND_TOTAL, "Total"),
        (keys::REPORT_YTD_SCOPE, "YTD"),
        (keys::REPORT_YEAR_SCOPE, "Year"),
        (keys::REPORT_PTO_TOTAL, "PTO"),
        (keys::LIVE_REFRESH_APPLIED, "Timesheet updated"),
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
        (keys::OTHER, "Autre"),
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
        (
            keys::OPEN_COMMIT_IN_BITBUCKET,
            "Ouvrir le commit dans Bitbucket",
        ),
        (
            keys::OPEN_PR_IN_BITBUCKET,
            "Ouvrir la demande de tirage dans Bitbucket",
        ),
        (keys::OPEN_SETTINGS, "Ouvrir les paramètres"),
        (keys::REFRESH_CACHED, "Rafraîchir les éléments en cache"),
        (keys::FORCE_PERIODIC_REFRESH, "Forcer le rafraîchissement périodique"),
        (keys::USER_REPORT, "Rapport utilisateur"),
        (keys::LOGOUT, "Se déconnecter"),
        (keys::OL_JIRA_DISABLED, "L\u{2019}intégration OL Jira est désactivée car plus de trois ans se sont écoulés depuis la dernière mise à jour."),
        (keys::ISSUE_ICON_ALT, "Icône du ticket"),
        (keys::REPORTING, "Rapport"),
        (keys::NON_BILLABLE_PROJECTS, "Projet(s) non facturable(s)"),
        (keys::MEETINGS, "Réunions"),
        (keys::LOCAL_HOLIDAYS, "Jours fériés locaux"),
        (keys::PLANNED_TIME_OFF, "Congé planifié"),
        (keys::STUDY, "Étude"),
        (
            keys::LIST_INPUT_HINT,
            "Valeurs séparées par virgule, point-virgule ou retour ligne",
        ),
        (keys::REPORT_PERIOD, "Période"),
        (keys::REPORT_PERIOD_WEEK, "Mois"),
        (keys::REPORT_PERIOD_MONTH, "Année"),
        (keys::BILLABLE, "Facturable"),
        (keys::NON_BILLABLE, "Non facturable"),
        (keys::PLANNED_TIME_OFF_HOURS, "Congé planifié (heures)"),
        (keys::REPORT_NO_DATA, "Aucune donnée pour la période sélectionnée"),
        (keys::REPORT_PREVIOUS, "Période précédente"),
        (keys::REPORT_NEXT, "Période suivante"),
        (keys::REPORT_LOADING, "Chargement du rapport…"),
        (
            keys::REPORT_GRAND_TOTAL,
            "Total",
        ),
        (keys::REPORT_YTD_SCOPE, "YTD"),
        (keys::REPORT_YEAR_SCOPE, "Année"),
        (keys::REPORT_PTO_TOTAL, "CP"),
        (keys::LIVE_REFRESH_APPLIED, "Feuille de temps mise à jour"),
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
        (keys::OTHER, "Overig"),
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
        (
            keys::OPEN_COMMIT_IN_BITBUCKET,
            "Commit openen in Bitbucket",
        ),
        (
            keys::OPEN_PR_IN_BITBUCKET,
            "Pull request openen in Bitbucket",
        ),
        (keys::OPEN_SETTINGS, "Instellingen openen"),
        (keys::REFRESH_CACHED, "Werkitems in cache vernieuwen"),
        (keys::FORCE_PERIODIC_REFRESH, "Periodieke verversing forceren"),
        (keys::USER_REPORT, "Gebruikersrapport"),
        (keys::LOGOUT, "Uitloggen"),
        (keys::OL_JIRA_DISABLED, "OL Jira-integratie is uitgeschakeld omdat er meer dan drie jaar zijn verstreken sinds de laatste update."),
        (keys::ISSUE_ICON_ALT, "Ticketpictogram"),
        (keys::REPORTING, "Rapportage"),
        (keys::NON_BILLABLE_PROJECTS, "Niet-factureerbare project(en)"),
        (keys::MEETINGS, "Vergaderingen"),
        (keys::LOCAL_HOLIDAYS, "Lokale feestdagen"),
        (keys::PLANNED_TIME_OFF, "Gepland verlof"),
        (keys::STUDY, "Studie"),
        (
            keys::LIST_INPUT_HINT,
            "Waarden gescheiden door komma, puntkomma of nieuwe regel",
        ),
        (keys::REPORT_PERIOD, "Periode"),
        (keys::REPORT_PERIOD_WEEK, "Maand"),
        (keys::REPORT_PERIOD_MONTH, "Jaar"),
        (keys::BILLABLE, "Factureerbaar"),
        (keys::NON_BILLABLE, "Niet-factureerbaar"),
        (keys::PLANNED_TIME_OFF_HOURS, "Gepland verlof (uren)"),
        (keys::REPORT_NO_DATA, "Geen gegevens voor geselecteerde periode"),
        (keys::REPORT_PREVIOUS, "Vorige periode"),
        (keys::REPORT_NEXT, "Volgende periode"),
        (keys::REPORT_LOADING, "Rapport laden…"),
        (
            keys::REPORT_GRAND_TOTAL,
            "Totaal",
        ),
        (keys::REPORT_YTD_SCOPE, "YTD"),
        (keys::REPORT_YEAR_SCOPE, "Jaar"),
        (keys::REPORT_PTO_TOTAL, "Verlof"),
        (keys::LIVE_REFRESH_APPLIED, "Urenstaat bijgewerkt"),
    ]
    .into_iter()
    .collect();

    [("en", en), ("fr", fr), ("nl", nl)].into_iter().collect()
});

static EMPTY_TRANSLATIONS: LazyLock<HashMap<&str, &str>> = LazyLock::new(HashMap::new);
static DEFAULT_TRANSLATIONS: LazyLock<&HashMap<&str, &str>> =
    LazyLock::new(|| TRANSLATIONS.get("en").unwrap_or(&EMPTY_TRANSLATIONS));

/// Internationalisation context provided to all components.
#[derive(Clone, Debug, PartialEq)]
pub struct I18n {
    /// The two-letter language code (e.g. "en", "fr").
    pub lang: String,
    /// The decimal separator for the current locale.
    pub decimal_separator: char,
    /// The thousands separator for the current locale.
    pub thousands_separator: char,
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
        let thousands_separator = match lang.as_str() {
            "fr" => ' ',
            "de" | "nl" | "es" | "it" | "pt" | "da" | "fi" | "nb" | "sv" | "pl" | "cs" | "sk"
            | "hu" | "ro" | "bg" | "hr" | "sl" | "el" | "tr" | "ru" | "uk" => '.',
            _ => ',',
        };
        Self {
            lang,
            decimal_separator,
            thousands_separator,
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
