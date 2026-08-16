use std::time::Duration;

use aether_data_contracts::DataLayerError;
use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use tracing::warn;

use crate::data::GatewayDataState;

use super::{
    system_config_string, DB_MAINTENANCE_HOUR, DB_MAINTENANCE_MINUTE, DB_MAINTENANCE_WEEKDAY,
    DB_MAINTENANCE_WEEKLY_INTERVAL, MAINTENANCE_DEFAULT_TIMEZONE, STATS_DAILY_AGGREGATION_HOUR,
    STATS_DAILY_AGGREGATION_MINUTE, STATS_HOURLY_AGGREGATION_MINUTE,
};

pub(super) fn maintenance_timezone() -> Tz {
    let configured = std::env::var("APP_TIMEZONE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| MAINTENANCE_DEFAULT_TIMEZONE.to_string());
    configured.parse().unwrap_or_else(|_| {
        warn!(
            timezone = %configured,
            fallback = MAINTENANCE_DEFAULT_TIMEZONE,
            "gateway maintenance timezone invalid; falling back"
        );
        MAINTENANCE_DEFAULT_TIMEZONE
            .parse()
            .expect("default maintenance timezone should parse")
    })
}

pub(super) fn duration_until_next_db_maintenance_run(
    now_utc: DateTime<Utc>,
    timezone: Tz,
) -> Duration {
    next_db_maintenance_run_after(now_utc, timezone)
        .signed_duration_since(now_utc)
        .to_std()
        .unwrap_or_default()
}

pub(super) fn next_db_maintenance_run_after(now_utc: DateTime<Utc>, timezone: Tz) -> DateTime<Utc> {
    let local_now = now_utc.with_timezone(&timezone);
    for day_offset in 0..=7 {
        let candidate_date = local_now.date_naive() + chrono::Duration::days(day_offset);
        if candidate_date.weekday() != DB_MAINTENANCE_WEEKDAY {
            continue;
        }
        let Some(candidate_naive) =
            candidate_date.and_hms_opt(DB_MAINTENANCE_HOUR, DB_MAINTENANCE_MINUTE, 0)
        else {
            continue;
        };
        let candidate_local = resolve_local_scheduled_time(timezone, candidate_naive);
        let Some(candidate_local) = candidate_local else {
            continue;
        };
        if candidate_local > local_now {
            return candidate_local.with_timezone(&Utc);
        }
    }

    let fallback_date = local_now.date_naive() + DB_MAINTENANCE_WEEKLY_INTERVAL;
    let fallback_naive = fallback_date
        .and_hms_opt(DB_MAINTENANCE_HOUR, DB_MAINTENANCE_MINUTE, 0)
        .expect("db maintenance fallback time should be valid");
    let fallback_local = resolve_local_scheduled_time(timezone, fallback_naive)
        .expect("db maintenance fallback local datetime should resolve");
    fallback_local.with_timezone(&Utc)
}

pub(super) fn duration_until_next_daily_run(
    now_utc: DateTime<Utc>,
    timezone: Tz,
    hour: u32,
    minute: u32,
) -> Duration {
    next_daily_run_after(now_utc, timezone, hour, minute)
        .signed_duration_since(now_utc)
        .to_std()
        .unwrap_or_default()
}

pub(super) fn duration_until_next_stats_aggregation_run(now_utc: DateTime<Utc>) -> Duration {
    next_stats_aggregation_run_after(now_utc)
        .signed_duration_since(now_utc)
        .to_std()
        .unwrap_or_default()
}

pub(super) fn duration_until_next_stats_hourly_aggregation_run(now_utc: DateTime<Utc>) -> Duration {
    next_stats_hourly_aggregation_run_after(now_utc)
        .signed_duration_since(now_utc)
        .to_std()
        .unwrap_or_default()
}

pub(super) fn next_daily_run_after(
    now_utc: DateTime<Utc>,
    timezone: Tz,
    hour: u32,
    minute: u32,
) -> DateTime<Utc> {
    let local_now = now_utc.with_timezone(&timezone);
    for day_offset in 0..=1 {
        let candidate_date = local_now.date_naive() + chrono::Duration::days(day_offset);
        let Some(candidate_naive) = candidate_date.and_hms_opt(hour, minute, 0) else {
            continue;
        };
        let candidate_local = resolve_local_scheduled_time(timezone, candidate_naive);
        let Some(candidate_local) = candidate_local else {
            continue;
        };
        if candidate_local > local_now {
            return candidate_local.with_timezone(&Utc);
        }
    }

    let fallback_date = local_now.date_naive() + chrono::Duration::days(1);
    let fallback_naive = fallback_date
        .and_hms_opt(hour, minute, 0)
        .expect("daily fallback time should be valid");
    let fallback_local = resolve_local_scheduled_time(timezone, fallback_naive)
        .expect("daily fallback local datetime should resolve");
    fallback_local.with_timezone(&Utc)
}

pub(super) fn next_stats_aggregation_run_after(now_utc: DateTime<Utc>) -> DateTime<Utc> {
    next_daily_run_after(
        now_utc,
        chrono_tz::UTC,
        STATS_DAILY_AGGREGATION_HOUR,
        STATS_DAILY_AGGREGATION_MINUTE,
    )
}

pub(super) fn next_stats_hourly_aggregation_run_after(now_utc: DateTime<Utc>) -> DateTime<Utc> {
    let current_hour_slot = Utc.from_utc_datetime(
        &now_utc
            .date_naive()
            .and_hms_opt(now_utc.hour(), STATS_HOURLY_AGGREGATION_MINUTE, 0)
            .expect("stats hourly aggregation slot should be valid"),
    );
    if current_hour_slot > now_utc {
        return current_hour_slot;
    }

    let next_hour = now_utc + chrono::Duration::hours(1);
    Utc.from_utc_datetime(
        &next_hour
            .date_naive()
            .and_hms_opt(next_hour.hour(), STATS_HOURLY_AGGREGATION_MINUTE, 0)
            .expect("stats hourly aggregation next slot should be valid"),
    )
}

pub(super) fn stats_aggregation_target_day(now_utc: DateTime<Utc>) -> DateTime<Utc> {
    let previous_day = now_utc - chrono::Duration::days(1);
    Utc.from_utc_datetime(
        &previous_day
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("stats aggregation target day should be valid"),
    )
}

pub(super) fn stats_hourly_aggregation_target_hour(now_utc: DateTime<Utc>) -> DateTime<Utc> {
    let previous_hour = now_utc - chrono::Duration::hours(1);
    Utc.from_utc_datetime(
        &previous_hour
            .date_naive()
            .and_hms_opt(previous_hour.hour(), 0, 0)
            .expect("stats hourly aggregation target hour should be valid"),
    )
}

pub(super) fn resolve_local_scheduled_time(
    timezone: Tz,
    naive: chrono::NaiveDateTime,
) -> Option<chrono::DateTime<Tz>> {
    match timezone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(value) => Some(value),
        chrono::LocalResult::Ambiguous(first, second) => Some(first.min(second)),
        chrono::LocalResult::None => None,
    }
}

pub(super) fn parse_hhmm_time(value: &str) -> Option<(u32, u32)> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour = hour.parse::<u32>().ok()?;
    let minute = minute.parse::<u32>().ok()?;
    (hour <= 23 && minute <= 59).then_some((hour, minute))
}
