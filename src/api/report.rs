#![cfg(feature = "ssr")]

use crate::api::jira::{JiraCredentials, fetch_work_items, fetch_worklogs};
use crate::model::ReportData;
use chrono::{Datelike, Local, NaiveDate};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Debug)]
struct ReportCacheEntry {
    report: ReportData,
    covered_until: NaiveDate,
}

static REPORT_CACHE: LazyLock<Mutex<HashMap<String, ReportCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn yyyymmdd(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

fn parse_day_key(day_key: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(day_key, "%Y%m%d").ok()
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
            entry
                .entry(day_key)
                .and_modify(|m| *m += minutes)
                .or_insert(minutes);
        }
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
}

fn report_cache_key(account_id: &str, year: i32) -> String {
    format!("{}:{}", account_id, year)
}

async fn build_report_range(
    creds: &JiraCredentials,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<ReportData, String> {
    let work_items = fetch_work_items(creds, start, end).await?;
    let mut billable: HashMap<String, HashMap<String, u64>> = HashMap::new();

    for item in work_items {
        let (entries, _) = fetch_worklogs(creds, &item.key, start, end).await?;
        let project_prefix = issue_project_prefix(&item.key);
        for entry in entries {
            let minutes = (entry.hours * 60.0).round().max(0.0) as u64;
            if minutes == 0 {
                continue;
            }
            billable
                .entry(project_prefix.clone())
                .or_default()
                .entry(yyyymmdd(entry.date))
                .and_modify(|m| *m += minutes)
                .or_insert(minutes);
        }
    }

    Ok(ReportData {
        billable,
        non_billable: HashMap::new(),
    })
}

pub async fn build_report_for_year(
    creds: &JiraCredentials,
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
                if entry.covered_until == dec31 {
                    return Ok(entry.report.clone());
                }
            }
        }
        let report = build_report_range(creds, jan1, dec31).await?;
        if let Ok(mut cache) = REPORT_CACHE.lock() {
            cache.insert(
                cache_key,
                ReportCacheEntry {
                    report: report.clone(),
                    covered_until: dec31,
                },
            );
        }
        return Ok(report);
    }

    let cached_report = if let Ok(cache) = REPORT_CACHE.lock() {
        cache.get(&cache_key).map(|entry| entry.report.clone())
    } else {
        None
    };

    let mut report = if let Some(existing) = cached_report {
        existing
    } else {
        build_report_range(creds, jan1, dec31).await?
    };

    let refresh_start = std::cmp::max(today, jan1);
    if refresh_start <= dec31 {
        let delta = build_report_range(creds, refresh_start, dec31).await?;
        clear_report_range(&mut report, refresh_start, dec31);
        merge_report_data(&mut report, delta);
    }

    if let Ok(mut cache) = REPORT_CACHE.lock() {
        cache.insert(
            cache_key,
            ReportCacheEntry {
                report: report.clone(),
                covered_until: dec31,
            },
        );
    }

    Ok(report)
}
