#![cfg(feature = "ssr")]

use crate::api::jira::{JiraCredentials, fetch_work_items, fetch_worklogs};
use crate::model::{NonBillableMinutes, ReportData, Settings};
use chrono::{Datelike, Local, NaiveDate};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex, OnceLock};

#[derive(Clone, Debug)]
struct ReportCacheEntry {
    settings: Settings,
    report: ReportData,
    covered_until: NaiveDate,
}

static REPORT_CACHE: LazyLock<Mutex<HashMap<String, ReportCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static REPORT_BOOT_DATE: OnceLock<NaiveDate> = OnceLock::new();

fn yyyymmdd(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

fn parse_day_key(day_key: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(day_key, "%Y%m%d").ok()
}

fn normalise_list(values: &[String]) -> HashSet<String> {
    values
        .iter()
        .map(|v| v.trim().to_uppercase())
        .filter(|v| !v.is_empty())
        .collect()
}

fn issue_project_prefix(issue_key: &str) -> String {
    issue_key
        .split_once('-')
        .map(|(prefix, _)| prefix.to_uppercase())
        .unwrap_or_else(|| issue_key.to_uppercase())
}

fn merge_report_data(target: &mut ReportData, delta: ReportData) {
    for (project, days) in delta.billable {
        let entry = target.billable.entry(project).or_default();
        for (day_key, minutes) in days {
            entry.entry(day_key).and_modify(|m| *m += minutes).or_insert(minutes);
        }
    }
    for (day_key, values) in delta.non_billable {
        let entry = target.non_billable.entry(day_key).or_default();
        entry.holidays += values.holidays;
        entry.meetings += values.meetings;
        entry.other += values.other;
        entry.pto += values.pto;
        entry.study += values.study;
    }
}

fn clear_report_range(report: &mut ReportData, start: NaiveDate, end: NaiveDate) {
    for days in report.billable.values_mut() {
        days.retain(|day_key, _| {
            parse_day_key(day_key)
                .map(|d| d < start || d > end)
                .unwrap_or(true)
        });
    }
    report.billable.retain(|_, days| !days.is_empty());
    report.non_billable.retain(|day_key, _| {
        parse_day_key(day_key)
            .map(|d| d < start || d > end)
            .unwrap_or(true)
    });
}

async fn build_report_range(
    creds: &JiraCredentials,
    settings: &Settings,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<ReportData, String> {
    let work_items = fetch_work_items(creds, start, end).await?;

    let meetings = normalise_list(&settings.meeting_keys);
    let holidays = normalise_list(&settings.local_holiday_keys);
    let pto = normalise_list(&settings.planned_time_off_keys);
    let study = normalise_list(&settings.study_keys);
    let non_billable_prefixes = normalise_list(&settings.non_billable_project_prefixes);

    let mut billable: HashMap<String, HashMap<String, u64>> = HashMap::new();
    let mut non_billable: HashMap<String, NonBillableMinutes> = HashMap::new();

    for item in work_items {
        let (entries, _) = fetch_worklogs(creds, &item.key, start, end).await?;
        let issue_key_norm = item.key.trim().to_uppercase();
        let project_prefix = issue_project_prefix(&item.key);

        for entry in entries {
            let day_key = yyyymmdd(entry.date);
            let minutes = (entry.hours * 60.0).round().max(0.0) as u64;
            if minutes == 0 {
                continue;
            }

            if meetings.contains(&issue_key_norm) {
                let day_bucket = non_billable.entry(day_key.clone()).or_default();
                day_bucket.meetings += minutes;
                continue;
            }
            if holidays.contains(&issue_key_norm) {
                let day_bucket = non_billable.entry(day_key.clone()).or_default();
                day_bucket.holidays += minutes;
                continue;
            }
            if pto.contains(&issue_key_norm) {
                let day_bucket = non_billable.entry(day_key.clone()).or_default();
                day_bucket.pto += minutes;
                continue;
            }
            if study.contains(&issue_key_norm) {
                let day_bucket = non_billable.entry(day_key.clone()).or_default();
                day_bucket.study += minutes;
                continue;
            }
            if non_billable_prefixes.contains(&project_prefix) {
                let day_bucket = non_billable.entry(day_key.clone()).or_default();
                day_bucket.other += minutes;
                continue;
            }

            billable
                .entry(project_prefix.clone())
                .or_default()
                .entry(day_key)
                .and_modify(|m| *m += minutes)
                .or_insert(minutes);
        }
    }

    Ok(ReportData {
        billable,
        non_billable,
    })
}

pub fn set_report_boot_date(date: NaiveDate) {
    let _ = REPORT_BOOT_DATE.set(date);
}

fn report_boot_date() -> NaiveDate {
    REPORT_BOOT_DATE
        .get()
        .copied()
        .unwrap_or_else(|| Local::now().date_naive())
}

fn report_cache_key(account_id: &str, year: i32) -> String {
    format!("{}:{}", account_id, year)
}

pub async fn prewarm_current_year_reports(users: Vec<crate::auth::StartupWarmUser>) {
    if users.is_empty() {
        return;
    }
    let boot_date = report_boot_date();
    let current_year = boot_date.year();
    let jan1 = match NaiveDate::from_ymd_opt(current_year, 1, 1) {
        Some(d) => d,
        None => return,
    };
    let warm_end =
        NaiveDate::from_ymd_opt(current_year, 12, 31).unwrap_or_else(|| report_boot_date());

    for user in users {
        let settings = crate::auth::load_user_prefs(&user.creds.account_id);
        match build_report_range(&user.creds, &settings, jan1, warm_end).await {
            Ok(report) => {
                let key = report_cache_key(&user.creds.account_id, current_year);
                if let Ok(mut cache) = REPORT_CACHE.lock() {
                    cache.insert(
                        key,
                        ReportCacheEntry {
                            settings,
                            report,
                            covered_until: warm_end,
                        },
                    );
                }
                log::info!(
                    "[report] startup prewarm complete account={} year={} range={}..{}",
                    user.creds.account_id,
                    current_year,
                    jan1,
                    warm_end
                );
            }
            Err(err) => {
                log::warn!(
                    "[report] startup prewarm failed account={} year={} err={}",
                    user.creds.account_id,
                    current_year,
                    err
                );
            }
        }
    }
}

pub async fn build_report_for_year(
    creds: &JiraCredentials,
    settings: &Settings,
    year: i32,
) -> Result<ReportData, String> {
    let jan1 = NaiveDate::from_ymd_opt(year, 1, 1).ok_or_else(|| "Invalid year".to_string())?;
    let dec31 =
        NaiveDate::from_ymd_opt(year, 12, 31).ok_or_else(|| "Invalid year end".to_string())?;
    let today = Local::now().date_naive();
    let current_year = today.year();
    let cache_key = report_cache_key(&creds.account_id, year);

    if year != current_year {
        if let Ok(cache) = REPORT_CACHE.lock() {
            if let Some(entry) = cache.get(&cache_key) {
                if entry.settings == *settings && entry.covered_until == dec31 {
                    return Ok(entry.report.clone());
                }
            }
        }
        let report = build_report_range(creds, settings, jan1, dec31).await?;
        if let Ok(mut cache) = REPORT_CACHE.lock() {
            cache.insert(
                cache_key,
                ReportCacheEntry {
                    settings: settings.clone(),
                    report: report.clone(),
                    covered_until: dec31,
                },
            );
        }
        return Ok(report);
    }

    let cached_report = if let Ok(cache) = REPORT_CACHE.lock() {
        if let Some(entry) = cache.get(&cache_key) {
            if entry.settings == *settings {
                Some(entry.report.clone())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let mut report = if let Some(existing) = cached_report {
        existing
    } else {
        build_report_range(creds, settings, jan1, dec31).await?
    };

    let refresh_start = std::cmp::max(today, jan1);
    if refresh_start <= dec31 {
        let delta = build_report_range(creds, settings, refresh_start, dec31).await?;
        clear_report_range(&mut report, refresh_start, dec31);
        merge_report_data(&mut report, delta);
    }

    if let Ok(mut cache) = REPORT_CACHE.lock() {
        cache.insert(
            cache_key,
            ReportCacheEntry {
                settings: settings.clone(),
                report: report.clone(),
                covered_until: dec31,
            },
        );
    }

    Ok(report)
}
