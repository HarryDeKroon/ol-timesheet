use crate::i18n::{I18n, keys};
use crate::model::{NonBillableMinutes, ReportData};
use chrono::{Datelike, Duration, Local, NaiveDate};
use leptos::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReportPeriod {
    Week,
    Month,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BarBucket {
    label: String,
    start: NaiveDate,
    end: NaiveDate,
}

#[derive(Clone, Debug, PartialEq)]
struct BarSegment {
    class_name: String,
    label: String,
    days: f64,
    minutes: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct PieSlice {
    class_name: String,
    label: String,
    minutes: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct TotalsRow {
    class_name: String,
    label: String,
    minutes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AlignedNumber {
    int_part: String,
    sep_part: String,
    frac_part: String,
}

#[server(GetReportData, "/api")]
pub async fn get_report_data(year: i32) -> Result<ReportData, ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
    crate::api::report::build_report_for_year(
        &session.jira_credentials(),
        &session.preferences,
        year,
    )
    .await
    .map_err(ServerFnError::new)
}

fn day_key(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

fn parse_day_key(key: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(key, "%Y%m%d").ok()
}

fn month_start(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
}

fn previous_month(date: NaiveDate) -> NaiveDate {
    let d = month_start(date);
    if d.month() == 1 {
        NaiveDate::from_ymd_opt(d.year() - 1, 12, 1).unwrap_or(d)
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() - 1, 1).unwrap_or(d)
    }
}

fn next_month(date: NaiveDate) -> NaiveDate {
    let d = month_start(date);
    if d.month() == 12 {
        NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap_or(d)
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap_or(d)
    }
}

fn month_end(date: NaiveDate) -> NaiveDate {
    next_month(month_start(date)) - Duration::days(1)
}

fn week_end_sunday(date: NaiveDate) -> NaiveDate {
    let days_to_sunday = 6 - date.weekday().num_days_from_monday() as i64;
    date + Duration::days(days_to_sunday)
}

fn bucket_label(i18n: &I18n, start: NaiveDate, end: NaiveDate) -> String {
    format!(
        "{}-{}",
        i18n.format_day_month(&start),
        i18n.format_day_month(&end)
    )
}

fn month_buckets(month: NaiveDate, i18n: &I18n) -> Vec<BarBucket> {
    let start = month_start(month);
    let end = month_end(month);
    let mut out = Vec::new();
    let mut cursor = start;
    while cursor <= end {
        let bucket_end = std::cmp::min(week_end_sunday(cursor), end);
        out.push(BarBucket {
            label: bucket_label(i18n, cursor, bucket_end),
            start: cursor,
            end: bucket_end,
        });
        cursor = bucket_end + Duration::days(1);
    }
    out
}

fn year_buckets(year: i32) -> Vec<BarBucket> {
    (1..=12)
        .filter_map(|month| {
            let start = NaiveDate::from_ymd_opt(year, month, 1)?;
            let end = month_end(start);
            Some(BarBucket {
                label: start.format("%Y-%m").to_string(),
                start,
                end,
            })
        })
        .collect()
}

fn default_report_month(today: NaiveDate) -> NaiveDate {
    if today.day() < 9 {
        previous_month(today)
    } else {
        month_start(today)
    }
}

fn minutes_to_days(minutes: u64, hours_per_day: f64) -> f64 {
    if hours_per_day <= 0.0 {
        return 0.0;
    }
    let raw = minutes as f64 / (hours_per_day * 60.0);
    (raw * 10.0).round() / 10.0
}

fn format_days(days: f64, decimal_sep: char) -> String {
    let rounded = (days * 10.0).round() / 10.0;
    let s = format!("{rounded:.1}");
    if decimal_sep == '.' {
        s
    } else {
        s.replace('.', &decimal_sep.to_string())
    }
}

fn grouped_int(value: &str, thousands_sep: char) -> String {
    let mut out_rev = String::new();
    for (idx, ch) in value.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out_rev.push(thousands_sep);
        }
        out_rev.push(ch);
    }
    out_rev.chars().rev().collect()
}

fn format_hours_one_decimal(
    hours: f64,
    decimal_sep: char,
    thousands_sep: char,
    grouped: bool,
) -> String {
    let rounded = (hours * 10.0).round() / 10.0;
    let raw = format!("{rounded:.1}");
    let (int_part, frac_part) = raw.split_once('.').unwrap_or((raw.as_str(), "0"));
    let int_display = if grouped {
        grouped_int(int_part, thousands_sep)
    } else {
        int_part.to_string()
    };
    format!("{}{}{}", int_display, decimal_sep, frac_part)
}

fn format_aligned_hours(minutes: u64, decimal_sep: char, thousands_sep: char) -> AlignedNumber {
    let value = format_hours_one_decimal(minutes as f64 / 60.0, decimal_sep, thousands_sep, true);
    if let Some((i, f)) = value.split_once(decimal_sep) {
        AlignedNumber {
            int_part: i.to_string(),
            sep_part: decimal_sep.to_string(),
            frac_part: f.to_string(),
        }
    } else {
        AlignedNumber {
            int_part: value,
            sep_part: decimal_sep.to_string(),
            frac_part: "0".to_string(),
        }
    }
}

fn format_hours_wdh(hours: f64, hours_per_day: f64, hours_per_week: f64) -> String {
    let mut remaining = (hours * 10.0).round() / 10.0;
    let mut parts = Vec::new();

    if hours_per_week > 0.0 {
        let weeks = (remaining / hours_per_week).floor();
        if weeks > 0.0 {
            parts.push(format!("{weeks:.0}w"));
            remaining -= weeks * hours_per_week;
        }
    }

    if hours_per_day > 0.0 {
        let days = (remaining / hours_per_day).floor();
        if days > 0.0 {
            parts.push(format!("{days:.0}d"));
            remaining -= days * hours_per_day;
        }
    }

    let rounded_hours = (remaining * 10.0).round() / 10.0;
    if rounded_hours > 0.0 || parts.is_empty() {
        if (rounded_hours.fract()).abs() < f64::EPSILON {
            parts.push(format!("{rounded_hours:.0}h"));
        } else {
            parts.push(format!("{rounded_hours:.1}h"));
        }
    }

    parts.join(" ")
}

fn pie_path(cx: f64, cy: f64, r: f64, start: f64, end: f64) -> String {
    let x1 = cx + r * start.cos();
    let y1 = cy + r * start.sin();
    let x2 = cx + r * end.cos();
    let y2 = cy + r * end.sin();
    let large = if end - start > std::f64::consts::PI {
        1
    } else {
        0
    };
    format!("M {cx} {cy} L {x1:.3} {y1:.3} A {r} {r} 0 {large} 1 {x2:.3} {y2:.3} Z")
}

fn non_billable_minutes_for(day: &NonBillableMinutes, category: &str) -> u64 {
    match category {
        "holidays" => day.holidays,
        "meetings" => day.meetings,
        "other" => day.other,
        "pto" => day.pto,
        "study" => day.study,
        _ => 0,
    }
}

fn days_inclusive(start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut cursor = start;
    while cursor <= end {
        out.push(cursor);
        cursor += Duration::days(1);
    }
    out
}

fn text_contrast_class(fill_class: &str) -> &'static str {
    match fill_class {
        "report-color-billable-6"
        | "report-color-billable-10"
        | "report-color-nonbillable-other"
        | "report-color-nonbillable-pto"
        | "report-color-nonbillable-study" => "report-text-dark",
        _ => "report-text-light",
    }
}

#[component]
pub fn ReportOverlay(
    hours_per_day: f64,
    hours_per_week: f64,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().unwrap_or_else(|| RwSignal::new(I18n::default()));
    let today = Local::now().date_naive();
    let period = RwSignal::new(ReportPeriod::Week);
    let selected_month = RwSignal::new(default_report_month(today));
    let selected_year = RwSignal::new(today.year());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let report_cache = RwSignal::new(HashMap::<i32, ReportData>::new());

    let context_year = Memo::new(move |_| {
        if period.get() == ReportPeriod::Week {
            selected_month.get().year()
        } else {
            selected_year.get()
        }
    });

    Effect::new(move |_| {
        let year = context_year.get();
        if report_cache.get_untracked().contains_key(&year) {
            return;
        }
        loading.set(true);
        #[cfg(feature = "hydrate")]
        leptos::task::spawn_local(async move {
            match get_report_data(year).await {
                Ok(report) => {
                    report_cache.update(|cache| {
                        cache.insert(year, report);
                    });
                    error.set(None);
                }
                Err(e) => error.set(Some(e.to_string())),
            }
            loading.set(false);
        });
    });

    let projects = Memo::new(move |_| {
        let year = context_year.get();
        let mut list = report_cache
            .get()
            .get(&year)
            .map(|report| report.billable.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        list.sort();
        list
    });

    let bar_buckets = Memo::new(move |_| {
        if period.get() == ReportPeriod::Week {
            let current_i18n = i18n.get();
            month_buckets(selected_month.get(), &current_i18n)
        } else {
            year_buckets(selected_year.get())
        }
    });

    let bar_segments = Memo::new(move |_| {
        let year = context_year.get();
        let cache = report_cache.get();
        let Some(report) = cache.get(&year) else {
            return Vec::<(BarBucket, Vec<BarSegment>)>::new();
        };

        let projects_in_year = projects.get();
        let mut buckets = bar_buckets
            .get()
            .into_iter()
            .map(|bucket| {
                let days = days_inclusive(bucket.start, bucket.end);
                let mut segs = Vec::<BarSegment>::new();

                for (idx, project) in projects_in_year.iter().enumerate() {
                    let minutes = days
                        .iter()
                        .map(|d| {
                            report
                                .billable
                                .get(project)
                                .and_then(|by_day| by_day.get(&day_key(*d)))
                                .copied()
                                .unwrap_or(0)
                        })
                        .sum::<u64>();
                    let day_total = minutes_to_days(minutes, hours_per_day);
                    if day_total > 0.0 {
                        segs.push(BarSegment {
                            class_name: format!("report-color-billable-{}", idx % 12),
                            label: project.clone(),
                            days: day_total,
                            minutes,
                        });
                    }
                }

                for (label, category, class_name) in [
                    (
                        i18n.get().t(keys::LOCAL_HOLIDAYS),
                        "holidays",
                        "report-color-nonbillable-holidays",
                    ),
                    (
                        i18n.get().t(keys::MEETINGS),
                        "meetings",
                        "report-color-nonbillable-meetings",
                    ),
                    (
                        i18n.get().t(keys::OTHER),
                        "other",
                        "report-color-nonbillable-other",
                    ),
                    (
                        i18n.get().t(keys::PLANNED_TIME_OFF),
                        "pto",
                        "report-color-nonbillable-pto",
                    ),
                    (
                        i18n.get().t(keys::STUDY),
                        "study",
                        "report-color-nonbillable-study",
                    ),
                ] {
                    let minutes = days
                        .iter()
                        .map(|d| {
                            report
                                .non_billable
                                .get(&day_key(*d))
                                .map(|nb| non_billable_minutes_for(nb, category))
                                .unwrap_or(0)
                        })
                        .sum::<u64>();
                    let day_total = minutes_to_days(minutes, hours_per_day);
                    if day_total > 0.0 {
                        segs.push(BarSegment {
                            class_name: class_name.to_string(),
                            label,
                            days: day_total,
                            minutes,
                        });
                    }
                }

                (bucket, segs)
            })
            .collect::<Vec<_>>();
        if period.get() == ReportPeriod::Week {
            let first_non_empty = buckets
                .iter()
                .position(|(_, segs)| !segs.is_empty())
                .unwrap_or(0);
            buckets.drain(0..first_non_empty);
        }
        buckets
    });

    let period_totals = Memo::new(move |_| {
        let year = context_year.get();
        let cache = report_cache.get();
        let Some(report) = cache.get(&year) else {
            return Vec::<TotalsRow>::new();
        };
        let Some(first) = bar_buckets.get().first().cloned() else {
            return Vec::<TotalsRow>::new();
        };
        let Some(last) = bar_buckets.get().last().cloned() else {
            return Vec::<TotalsRow>::new();
        };
        let period_days = days_inclusive(first.start, last.end);
        let mut rows = Vec::<TotalsRow>::new();

        let projects_in_year = projects.get();
        for (idx, project) in projects_in_year.iter().enumerate() {
            let minutes = period_days
                .iter()
                .map(|d| {
                    report
                        .billable
                        .get(project)
                        .and_then(|by_day| by_day.get(&day_key(*d)))
                        .copied()
                        .unwrap_or(0)
                })
                .sum::<u64>();
            if minutes > 0 {
                rows.push(TotalsRow {
                    class_name: format!("report-color-billable-{}", idx % 12),
                    label: project.clone(),
                    minutes,
                });
            }
        }

        for (label, category, class_name) in [
            (
                i18n.get().t(keys::LOCAL_HOLIDAYS),
                "holidays",
                "report-color-nonbillable-holidays",
            ),
            (
                i18n.get().t(keys::MEETINGS),
                "meetings",
                "report-color-nonbillable-meetings",
            ),
            (
                i18n.get().t(keys::OTHER),
                "other",
                "report-color-nonbillable-other",
            ),
            (
                i18n.get().t(keys::PLANNED_TIME_OFF),
                "pto",
                "report-color-nonbillable-pto",
            ),
            (
                i18n.get().t(keys::STUDY),
                "study",
                "report-color-nonbillable-study",
            ),
        ] {
            let minutes = period_days
                .iter()
                .map(|d| {
                    report
                        .non_billable
                        .get(&day_key(*d))
                        .map(|nb| non_billable_minutes_for(nb, category))
                        .unwrap_or(0)
                })
                .sum::<u64>();
            if minutes > 0 {
                rows.push(TotalsRow {
                    class_name: class_name.to_string(),
                    label,
                    minutes,
                });
            }
        }

        rows
    });

    let period_grand_total =
        Memo::new(move |_| period_totals.get().iter().map(|r| r.minutes).sum::<u64>());

    let pie_data = Memo::new(move |_| {
        let year = context_year.get();
        let cache = report_cache.get();
        let Some(report) = cache.get(&year) else {
            return (Vec::<PieSlice>::new(), 0_u64, 0_u64, 0_u64);
        };

        let jan1 = NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(today);
        let dec31 = NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(today);
        let scope_end = if year == today.year() { today } else { dec31 };

        let mut slices = Vec::<PieSlice>::new();
        let projects_in_year = projects.get();
        for (idx, project) in projects_in_year.iter().enumerate() {
            let minutes = report
                .billable
                .get(project)
                .map(|by_day| {
                    by_day
                        .iter()
                        .filter_map(|(k, v)| parse_day_key(k).map(|d| (d, v)))
                        .filter(|(d, _)| *d >= jan1 && *d <= scope_end)
                        .map(|(_, v)| *v)
                        .sum::<u64>()
                })
                .unwrap_or(0);
            if minutes > 0 {
                slices.push(PieSlice {
                    class_name: format!("report-color-billable-{}", idx % 12),
                    label: project.clone(),
                    minutes,
                });
            }
        }

        for (label, category, class_name) in [
            (
                i18n.get().t(keys::MEETINGS),
                "meetings",
                "report-color-nonbillable-meetings",
            ),
            (
                i18n.get().t(keys::OTHER),
                "other",
                "report-color-nonbillable-other",
            ),
            (
                i18n.get().t(keys::STUDY),
                "study",
                "report-color-nonbillable-study",
            ),
        ] {
            let minutes = report
                .non_billable
                .iter()
                .filter_map(|(k, v)| parse_day_key(k).map(|d| (d, v)))
                .filter(|(d, _)| *d >= jan1 && *d <= scope_end)
                .map(|(_, v)| non_billable_minutes_for(v, category))
                .sum::<u64>();
            if minutes > 0 {
                slices.push(PieSlice {
                    class_name: class_name.to_string(),
                    label,
                    minutes,
                });
            }
        }

        let work_total_scope = slices.iter().map(|s| s.minutes).sum::<u64>();
        let pto_total = report
            .non_billable
            .iter()
            .filter_map(|(k, v)| parse_day_key(k).map(|d| (d, v)))
            .filter(|(d, _)| *d >= jan1 && *d <= dec31)
            .map(|(_, v)| v.pto)
            .sum::<u64>();
        let annual_work_total = report
            .billable
            .values()
            .map(|by_day| {
                by_day
                    .iter()
                    .filter_map(|(k, v)| parse_day_key(k).map(|d| (d, v)))
                    .filter(|(d, _)| *d >= jan1 && *d <= dec31)
                    .map(|(_, v)| *v)
                    .sum::<u64>()
            })
            .sum::<u64>()
            + report
                .non_billable
                .iter()
                .filter_map(|(k, v)| parse_day_key(k).map(|d| (d, v)))
                .filter(|(d, _)| *d >= jan1 && *d <= dec31)
                .map(|(_, v)| v.meetings + v.other + v.pto + v.study)
                .sum::<u64>();

        (slices, work_total_scope, pto_total, annual_work_total)
    });

    let on_prev = move |_| {
        if period.get() == ReportPeriod::Week {
            selected_month.update(|m| *m = previous_month(*m));
        } else {
            selected_year.update(|y| *y -= 1);
        }
    };
    let on_next = move |_| {
        if period.get() == ReportPeriod::Week {
            selected_month.update(|m| *m = next_month(*m));
        } else {
            selected_year.update(|y| *y += 1);
        }
    };

    view! {
        <div class="report-overlay-backdrop" on:click=move |_| on_close.run(())>
            <div class="report-overlay" on:click=move |ev: leptos::ev::MouseEvent| ev.stop_propagation()>
                <div class="report-header">
                    <h2>{move || i18n.get().t(keys::USER_REPORT)}</h2>
                    <button class="report-close" on:click=move |_| on_close.run(())>{move || i18n.get().t(keys::CLOSE)}</button>
                </div>

                <div class="report-toolbar">
                    <div class="report-controls">
                        <label>
                            {move || i18n.get().t(keys::REPORT_PERIOD)}
                            <select on:change=move |ev| {
                                let value = event_target_value(&ev);
                                if value == "month" {
                                    period.set(ReportPeriod::Month);
                                    selected_year.set(selected_month.get().year());
                                } else {
                                    period.set(ReportPeriod::Week);
                                }
                            }>
                                <option value="week" selected={move || period.get() == ReportPeriod::Week}>{move || i18n.get().t(keys::REPORT_PERIOD_WEEK)}</option>
                                <option value="month" selected={move || period.get() == ReportPeriod::Month}>{move || i18n.get().t(keys::REPORT_PERIOD_MONTH)}</option>
                            </select>
                        </label>
                    </div>
                </div>

                {move || error.get().map(|msg| view! { <p class="error">{msg}</p> })}

                <div class="report-content">
                    <div class="report-chart-panel">
                        <div class="report-period-nav">
                            <button class="nav-btn" on:click=on_prev title={move || i18n.get().t(keys::REPORT_PREVIOUS)}>{"◀"}</button>
                            <span class="report-period-label">
                                {move || if period.get() == ReportPeriod::Week {
                                    selected_month.get().format("%Y-%m").to_string()
                                } else {
                                    selected_year.get().to_string()
                                }}
                            </span>
                            <button class="nav-btn" on:click=on_next title={move || i18n.get().t(keys::REPORT_NEXT)}>{"▶"}</button>
                        </div>

                        {move || {
                            if loading.get() {
                                return view! { <p>{move || i18n.get().t(keys::REPORT_LOADING)}</p> }.into_any();
                            }
                            let bars = bar_segments.get();
                            if bars.is_empty() {
                                return view! { <p>{move || i18n.get().t(keys::REPORT_NO_DATA)}</p> }.into_any();
                            }

                            let chart_width = 900.0_f64;
                            let chart_height = 250.0_f64;
                            let margin_left = 42.0_f64;
                            let margin_bottom = 20.0_f64;
                            let margin_top = 4.0_f64;
                            let plot_width = chart_width - margin_left - 10.0;
                            let plot_height = chart_height - margin_bottom - margin_top;
                            let y_max = if period.get() == ReportPeriod::Week { 7.0_f64 } else { 25.0_f64 };
                            let axis_step = if period.get() == ReportPeriod::Week { 1 } else { 5 };
                            let thick_step = if period.get() == ReportPeriod::Week { 5 } else { 10 };
                            let bar_count = bars.len().max(1) as f64;
                            let slot_width = plot_width / bar_count;
                            let bar_width = slot_width * 0.7;

                            let show_bar_totals = period.get() == ReportPeriod::Week;
                            view! {
                                <div class={if show_bar_totals { "report-bar-section" } else { "report-bar-section report-bar-section-no-totals" }}>
                                    <svg class="report-stacked-chart" viewBox="0 0 900 250" preserveAspectRatio="none" role="img" aria-label="Stacked report chart">
                                        {(0..=((y_max as i32) / axis_step)).map(|i| {
                                            let y_units = i * axis_step;
                                            let y = margin_top + plot_height - (y_units as f64 / y_max * plot_height);
                                            let class_name = if y_units % thick_step == 0 { "report-grid-line-thick" } else { "report-grid-line-thin" };
                                            view! {
                                                <g>
                                                    <line class={class_name} x1={margin_left.to_string()} y1={y.to_string()} x2={(margin_left + plot_width).to_string()} y2={y.to_string()}></line>
                                                    <text class="report-axis-label" x="4" y={(y + 4.0).to_string()}>{y_units.to_string()}</text>
                                                </g>
                                            }
                                        }).collect::<Vec<_>>()}

                                        {bars.into_iter().enumerate().map(|(idx, (bucket, segments))| {
                                            let x = margin_left + idx as f64 * slot_width + (slot_width - bar_width) / 2.0;
                                            let mut acc_days = 0.0_f64;
                                            let mut segment_nodes = Vec::new();
                                            for segment in segments {
                                                let h = (segment.days / y_max) * plot_height;
                                                let y = margin_top + plot_height - ((acc_days + segment.days) / y_max * plot_height);
                                                acc_days += segment.days;
                                                let contrast = text_contrast_class(&segment.class_name);
                                                let title = format!(
                                                    "{}: {}h",
                                                    segment.label,
                                                    format_hours_one_decimal(segment.minutes as f64 / 60.0, i18n.get().decimal_separator, i18n.get().thousands_separator, false)
                                                );
                                                segment_nodes.push(view! {
                                                    <g>
                                                        <rect class={format!("report-stack-segment {}", segment.class_name)} x={x.to_string()} y={y.to_string()} width={bar_width.to_string()} height={h.to_string()}>
                                                            <title>{title}</title>
                                                        </rect>
                                                        <text class={format!("report-stack-label {}", contrast)} x={(x + bar_width / 2.0).to_string()} y={(y + h / 2.0 + 3.0).to_string()}>
                                                            {format_days(segment.days, i18n.get().decimal_separator)}
                                                        </text>
                                                    </g>
                                                });
                                            }
                                            view! {
                                                <g>
                                                    {segment_nodes}
                                                    <text class="report-axis-label report-x-label" x={(x + bar_width / 2.0).to_string()} y={(chart_height - 8.0).to_string()}>{bucket.label}</text>
                                                </g>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </svg>

                                    {move || show_bar_totals.then(|| {
                                        view! {
                                            <div class="report-totals-grid">
                                                <For
                                                    each={move || period_totals.get()}
                                                    key={|r| format!("{}-{}", r.label, r.class_name)}
                                                    children={move |row: TotalsRow| {
                                                        let number = format_aligned_hours(row.minutes, i18n.get().decimal_separator, i18n.get().thousands_separator);
                                                        let title_text = format_hours_wdh(row.minutes as f64 / 60.0, hours_per_day, hours_per_week);
                                                        view! {
                                                            <div class="report-total-item">
                                                                <span class={format!("report-filter-swatch {}", row.class_name)}></span>
                                                                <span class="report-total-label">{row.label}</span>
                                                                <span class="report-total-number" title={title_text}>
                                                                    <span class="report-total-int">{number.int_part}</span>
                                                                    <span class="report-total-sep">{number.sep_part}</span>
                                                                    <span class="report-total-frac">{number.frac_part}</span>
                                                                </span>
                                                            </div>
                                                        }
                                                    }}
                                                />
                                                <div class="report-total-item report-total-item-grand">
                                                    <span class="report-filter-swatch report-filter-swatch-placeholder"></span>
                                                    <span class="report-total-label">{move || i18n.get().t(keys::TOTAL)}</span>
                                                    {move || {
                                                        let number = format_aligned_hours(period_grand_total.get(), i18n.get().decimal_separator, i18n.get().thousands_separator);
                                                        let title_text = format_hours_wdh(period_grand_total.get() as f64 / 60.0, hours_per_day, hours_per_week);
                                                        view! {
                                                            <span class="report-total-number" title={title_text}>
                                                                <span class="report-total-int">{number.int_part}</span>
                                                                <span class="report-total-sep">{number.sep_part}</span>
                                                                <span class="report-total-frac">{number.frac_part}</span>
                                                            </span>
                                                        }
                                                    }}
                                                </div>
                                            </div>
                                        }
                                    })}
                                </div>
                            }.into_any()
                        }}
                    </div>

                    <div class="report-chart-panel">
                        {move || {
                            let (slices, work_total_scope, pto_total, annual_work_total) = pie_data.get();
                            if slices.is_empty() {
                                return view! { <p>{move || i18n.get().t(keys::REPORT_NO_DATA)}</p> }.into_any();
                            }
                            let mut angle = -std::f64::consts::FRAC_PI_2;
                            let cx = 130.0_f64;
                            let cy = 130.0_f64;
                            let r = 129.0_f64;
                            let mut nodes = Vec::new();
                            for slice in slices.iter().cloned() {
                                let frac = if work_total_scope == 0 { 0.0 } else { slice.minutes as f64 / work_total_scope as f64 };
                                if frac <= 0.0 {
                                    continue;
                                }
                                let next = angle + frac * std::f64::consts::TAU;
                                let mid = angle + (next - angle) / 2.0;
                                let label_x = cx + (r * 0.62) * mid.cos();
                                let label_y = cy + (r * 0.62) * mid.sin();
                                let contrast = text_contrast_class(&slice.class_name);
                                let title = format!(
                                    "{}: {}h",
                                    slice.label,
                                    format_hours_one_decimal(slice.minutes as f64 / 60.0, i18n.get().decimal_separator, i18n.get().thousands_separator, false)
                                );
                                nodes.push(view! {
                                    <g>
                                        <path class={format!("report-pie-slice {}", slice.class_name)} d={pie_path(cx, cy, r, angle, next)}>
                                            <title>{title}</title>
                                        </path>
                                        <text class={format!("report-pie-label {}", contrast)} x={label_x.to_string()} y={(label_y + 3.0).to_string()}>{format!("{:.0}%", frac * 100.0)}</text>
                                    </g>
                                });
                                angle = next;
                            }
                            let work_total = format_aligned_hours(work_total_scope, i18n.get().decimal_separator, i18n.get().thousands_separator);
                            let pto = format_aligned_hours(pto_total, i18n.get().decimal_separator, i18n.get().thousands_separator);
                            let grand = format_aligned_hours(annual_work_total, i18n.get().decimal_separator, i18n.get().thousands_separator);
                            let work_total_label = if context_year.get() == today.year() {
                                format!(
                                    "{} {}",
                                    i18n.get().t(keys::REPORT_YTD_SCOPE),
                                    i18n.get().t(keys::REPORT_GRAND_TOTAL)
                                )
                            } else {
                                format!(
                                    "{} {}",
                                    i18n.get().t(keys::REPORT_YEAR_SCOPE),
                                    i18n.get().t(keys::REPORT_GRAND_TOTAL)
                                )
                            };
                            view! {
                                <div class="report-pie-section">
                                    <svg class="report-pie-chart" viewBox="0 0 260 260" role="img" aria-label=move || i18n.get().t(keys::USER_REPORT)>
                                        {nodes}
                                    </svg>
                                    <div class="report-totals-grid report-pie-totals-grid">
                                        {slices.into_iter().map(|slice| {
                                            let number = format_aligned_hours(slice.minutes, i18n.get().decimal_separator, i18n.get().thousands_separator);
                                            let title_text = format_hours_wdh(slice.minutes as f64 / 60.0, hours_per_day, hours_per_week);
                                            view! {
                                                <div class="report-total-item">
                                                    <span class={format!("report-filter-swatch {}", slice.class_name)}></span>
                                                    <span class="report-total-label">{slice.label}</span>
                                                    <span class="report-total-number" title={title_text}>
                                                        <span class="report-total-int">{number.int_part}</span>
                                                        <span class="report-total-sep">{number.sep_part}</span>
                                                        <span class="report-total-frac">{number.frac_part}</span>
                                                    </span>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                        <div class="report-total-item">
                                            <span class="report-filter-swatch report-color-nonbillable-pto"></span>
                                            <span class="report-total-label">{move || i18n.get().t(keys::PLANNED_TIME_OFF)}</span>
                                            <span class="report-total-number" title={format_hours_wdh(pto_total as f64 / 60.0, hours_per_day, hours_per_week)}>
                                                <span class="report-total-int">{pto.int_part}</span>
                                                <span class="report-total-sep">{pto.sep_part}</span>
                                                <span class="report-total-frac">{pto.frac_part}</span>
                                            </span>
                                        </div>
                                        <div class="report-total-divider"></div>
                                        <div class="report-total-item report-total-item-grand">
                                            <span class="report-filter-swatch report-filter-swatch-placeholder"></span>
                                            <span class="report-total-label">{work_total_label}</span>
                                            <span class="report-total-number" title={format_hours_wdh(work_total_scope as f64 / 60.0, hours_per_day, hours_per_week)}>
                                                <span class="report-total-int">{work_total.int_part}</span>
                                                <span class="report-total-sep">{work_total.sep_part}</span>
                                                <span class="report-total-frac">{work_total.frac_part}</span>
                                            </span>
                                        </div>
                                        <div class="report-total-item">
                                            <span class="report-filter-swatch report-filter-swatch-placeholder"></span>
                                            <span class="report-total-label">{move || i18n.get().t(keys::REPORT_GRAND_TOTAL)}</span>
                                            <span class="report-total-number" title={format_hours_wdh(annual_work_total as f64 / 60.0, hours_per_day, hours_per_week)}>
                                                <span class="report-total-int">{grand.int_part}</span>
                                                <span class="report-total-sep">{grand.sep_part}</span>
                                                <span class="report-total-frac">{grand.frac_part}</span>
                                            </span>
                                        </div>
                                    </div>
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>
            </div>
        </div>
    }
}
