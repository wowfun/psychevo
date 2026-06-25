use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, Local, LocalResult, NaiveDate, NaiveTime,
    TimeZone,
};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AutomationSchedule {
    Interval {
        #[serde(rename = "everyMinutes")]
        every_minutes: u32,
    },
    Delay {
        #[serde(rename = "afterMinutes")]
        after_minutes: u32,
    },
    Once {
        at: String,
    },
    Daily {
        time: String,
    },
    Weekly {
        weekdays: Vec<u8>,
        time: String,
    },
}

pub fn latest_due_at_ms(
    schedule: &AutomationSchedule,
    created_at_ms: i64,
    last_run_at_ms: Option<i64>,
    now_ms: i64,
) -> Result<Option<i64>> {
    if now_ms < created_at_ms {
        return Ok(None);
    }
    let after_ms = last_run_at_ms.unwrap_or(created_at_ms.saturating_sub(1));
    let candidate = match schedule {
        AutomationSchedule::Interval { every_minutes } => {
            latest_interval_due(*every_minutes, created_at_ms, now_ms)?
        }
        AutomationSchedule::Delay { after_minutes } => latest_one_shot_due(
            delay_due_at(*after_minutes, created_at_ms)?,
            last_run_at_ms,
            now_ms,
        )?,
        AutomationSchedule::Once { at } => {
            latest_one_shot_due(parse_once_at_ms(at)?, last_run_at_ms, now_ms)?
        }
        AutomationSchedule::Daily { time } => {
            latest_calendar_due(&parse_local_time(time)?, &[], created_at_ms, now_ms)?
        }
        AutomationSchedule::Weekly { weekdays, time } => {
            validate_weekdays(weekdays)?;
            latest_calendar_due(&parse_local_time(time)?, weekdays, created_at_ms, now_ms)?
        }
    };
    Ok(candidate.filter(|value| *value > after_ms && *value >= created_at_ms))
}

pub fn next_run_at_ms(
    schedule: &AutomationSchedule,
    created_at_ms: i64,
    last_run_at_ms: Option<i64>,
    now_ms: i64,
) -> Result<Option<i64>> {
    let after_ms = now_ms.max(last_run_at_ms.unwrap_or(created_at_ms));
    let next = match schedule {
        AutomationSchedule::Interval { every_minutes } => {
            next_interval_due(*every_minutes, created_at_ms, after_ms)
        }
        AutomationSchedule::Delay { after_minutes } => {
            return next_one_shot_due(
                delay_due_at(*after_minutes, created_at_ms)?,
                created_at_ms,
                last_run_at_ms,
            );
        }
        AutomationSchedule::Once { at } => {
            return next_one_shot_due(parse_once_at_ms(at)?, created_at_ms, last_run_at_ms);
        }
        AutomationSchedule::Daily { time } => {
            next_calendar_due(&parse_local_time(time)?, &[], created_at_ms, after_ms)
        }
        AutomationSchedule::Weekly { weekdays, time } => {
            validate_weekdays(weekdays)?;
            next_calendar_due(&parse_local_time(time)?, weekdays, created_at_ms, after_ms)
        }
    }?;
    Ok(Some(next))
}

fn latest_interval_due(every_minutes: u32, created_at_ms: i64, now_ms: i64) -> Result<Option<i64>> {
    let period_ms = interval_ms(every_minutes)?;
    if now_ms < created_at_ms.saturating_add(period_ms) {
        return Ok(None);
    }
    let elapsed = now_ms.saturating_sub(created_at_ms);
    let ticks = elapsed / period_ms;
    Ok(Some(
        created_at_ms.saturating_add(ticks.saturating_mul(period_ms)),
    ))
}

fn next_interval_due(every_minutes: u32, created_at_ms: i64, after_ms: i64) -> Result<i64> {
    let period_ms = interval_ms(every_minutes)?;
    if after_ms < created_at_ms.saturating_add(period_ms) {
        return Ok(created_at_ms.saturating_add(period_ms));
    }
    let elapsed = after_ms.saturating_sub(created_at_ms);
    let ticks = elapsed / period_ms + 1;
    Ok(created_at_ms.saturating_add(ticks.saturating_mul(period_ms)))
}

fn interval_ms(every_minutes: u32) -> Result<i64> {
    if every_minutes == 0 {
        return Err(Error::Message(
            "automation interval must be at least one minute".to_string(),
        ));
    }
    Ok(i64::from(every_minutes).saturating_mul(60_000))
}

fn delay_due_at(after_minutes: u32, created_at_ms: i64) -> Result<i64> {
    if after_minutes == 0 {
        return Err(Error::Message(
            "automation delay must be at least one minute".to_string(),
        ));
    }
    Ok(created_at_ms.saturating_add(i64::from(after_minutes).saturating_mul(60_000)))
}

fn latest_one_shot_due(
    due_at_ms: i64,
    last_run_at_ms: Option<i64>,
    now_ms: i64,
) -> Result<Option<i64>> {
    if last_run_at_ms.is_some() || now_ms < due_at_ms {
        return Ok(None);
    }
    Ok(Some(due_at_ms))
}

fn next_one_shot_due(
    due_at_ms: i64,
    created_at_ms: i64,
    last_run_at_ms: Option<i64>,
) -> Result<Option<i64>> {
    if last_run_at_ms.is_some() || due_at_ms < created_at_ms {
        return Ok(None);
    }
    Ok(Some(due_at_ms))
}

fn latest_calendar_due(
    time: &NaiveTime,
    weekdays: &[u8],
    created_at_ms: i64,
    now_ms: i64,
) -> Result<Option<i64>> {
    let now = local_datetime(now_ms)?;
    for day_offset in 0..=8 {
        let date = now
            .date_naive()
            .checked_sub_days(chrono::Days::new(day_offset))
            .ok_or_else(|| Error::Message("automation schedule date underflow".to_string()))?;
        if !weekday_matches(date, weekdays) {
            continue;
        }
        let Some(candidate) = resolve_local(date, *time) else {
            continue;
        };
        let candidate_ms = candidate.timestamp_millis();
        if candidate_ms <= now_ms && candidate_ms >= created_at_ms {
            return Ok(Some(candidate_ms));
        }
    }
    Ok(None)
}

fn next_calendar_due(
    time: &NaiveTime,
    weekdays: &[u8],
    created_at_ms: i64,
    after_ms: i64,
) -> Result<i64> {
    let after = local_datetime(after_ms.max(created_at_ms))?;
    for day_offset in 0..=14 {
        let date = after
            .date_naive()
            .checked_add_days(chrono::Days::new(day_offset))
            .ok_or_else(|| Error::Message("automation schedule date overflow".to_string()))?;
        if !weekday_matches(date, weekdays) {
            continue;
        }
        let Some(candidate) = resolve_local(date, *time) else {
            continue;
        };
        let candidate_ms = candidate.timestamp_millis();
        if candidate_ms > after_ms && candidate_ms >= created_at_ms {
            return Ok(candidate_ms);
        }
    }
    Err(Error::Message(
        "could not resolve next automation schedule".to_string(),
    ))
}

fn local_datetime(ms: i64) -> Result<DateTime<Local>> {
    match Local.timestamp_millis_opt(ms) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(early, _) => Ok(early),
        LocalResult::None => Err(Error::Message(format!(
            "invalid automation timestamp: {ms}"
        ))),
    }
}

fn resolve_local(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Local>> {
    let mut naive = date.and_time(time);
    for _ in 0..=180 {
        match Local.from_local_datetime(&naive) {
            LocalResult::Single(value) => return Some(value),
            LocalResult::Ambiguous(early, _) => return Some(early),
            LocalResult::None => {
                naive += ChronoDuration::minutes(1);
            }
        }
    }
    None
}

fn parse_local_time(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value.trim(), "%H:%M")
        .map_err(|_| Error::Message(format!("invalid automation local time: {value}")))
}

fn parse_once_at_ms(value: &str) -> Result<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::Message(
            "automation once schedule requires an at timestamp".to_string(),
        ));
    }
    if let Ok(value) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(value.timestamp_millis());
    }
    for format in ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, format) {
            return match Local.from_local_datetime(&naive) {
                LocalResult::Single(value) => Ok(value.timestamp_millis()),
                LocalResult::Ambiguous(early, _) => Ok(early.timestamp_millis()),
                LocalResult::None => Err(Error::Message(format!(
                    "invalid automation local timestamp: {trimmed}"
                ))),
            };
        }
    }
    Err(Error::Message(format!(
        "invalid automation once timestamp: {trimmed}"
    )))
}

fn validate_weekdays(weekdays: &[u8]) -> Result<()> {
    if weekdays.is_empty() {
        return Err(Error::Message(
            "weekly automation schedule requires at least one weekday".to_string(),
        ));
    }
    if weekdays.iter().any(|day| !(1..=7).contains(day)) {
        return Err(Error::Message(
            "weekly automation weekdays must be 1..7".to_string(),
        ));
    }
    Ok(())
}

fn weekday_matches(date: NaiveDate, weekdays: &[u8]) -> bool {
    weekdays.is_empty()
        || weekdays
            .iter()
            .any(|day| u32::from(*day) == date.weekday().number_from_monday())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_due_uses_latest_missed_run_once() {
        let schedule = AutomationSchedule::Interval { every_minutes: 10 };
        let created = 1_000;
        let now = created + 65 * 60_000;

        assert_eq!(
            latest_due_at_ms(&schedule, created, None, now).expect("due"),
            Some(created + 60 * 60_000)
        );
        assert_eq!(
            next_run_at_ms(&schedule, created, None, now).expect("next"),
            Some(created + 70 * 60_000)
        );
    }

    #[test]
    fn interval_due_respects_last_run() {
        let schedule = AutomationSchedule::Interval { every_minutes: 10 };
        let created = 1_000;
        let now = created + 65 * 60_000;
        let last = Some(created + 60 * 60_000);

        assert_eq!(
            latest_due_at_ms(&schedule, created, last, now).expect("due"),
            None
        );
    }

    #[test]
    fn delay_due_runs_once() {
        let schedule = AutomationSchedule::Delay { after_minutes: 15 };
        let created = 1_000;
        let due = created + 15 * 60_000;

        assert_eq!(
            next_run_at_ms(&schedule, created, None, created).expect("next"),
            Some(due)
        );
        assert_eq!(
            latest_due_at_ms(&schedule, created, None, due + 1).expect("due"),
            Some(due)
        );
        assert_eq!(
            next_run_at_ms(&schedule, created, Some(due), due + 1).expect("next after run"),
            None
        );
        assert_eq!(
            latest_due_at_ms(&schedule, created, Some(due), due + 60_000).expect("due after run"),
            None
        );
    }

    #[test]
    fn once_due_uses_absolute_timestamp_once() {
        let due = 1_772_360_400_000;
        let schedule = AutomationSchedule::Once {
            at: "2026-03-01T10:20:00Z".to_string(),
        };
        let created = due - 60_000;

        assert_eq!(
            next_run_at_ms(&schedule, created, None, created).expect("next"),
            Some(due)
        );
        assert_eq!(
            latest_due_at_ms(&schedule, created, None, due).expect("due"),
            Some(due)
        );
        assert_eq!(
            latest_due_at_ms(&schedule, created, Some(due), due + 1).expect("due after run"),
            None
        );
    }

    #[test]
    fn daily_due_and_next_use_local_time() {
        let time = NaiveTime::from_hms_opt(9, 0, 0).expect("time");
        let due = resolve_local(Local::now().date_naive(), time)
            .expect("daily local occurrence")
            .timestamp_millis();
        let schedule = AutomationSchedule::Daily {
            time: "09:00".to_string(),
        };
        let created = due - 60 * 60_000;
        let now = due + 60_000;

        assert_eq!(
            latest_due_at_ms(&schedule, created, None, now).expect("due"),
            Some(due)
        );
        let next = next_run_at_ms(&schedule, created, Some(due), now)
            .expect("next")
            .expect("next occurrence");
        assert!(next > now);
    }

    #[test]
    fn weekly_rejects_empty_weekdays() {
        let schedule = AutomationSchedule::Weekly {
            weekdays: Vec::new(),
            time: "09:00".to_string(),
        };

        assert!(latest_due_at_ms(&schedule, 0, None, 1).is_err());
    }
}
