use crate::openhuman::cron::{ActiveHours, Schedule};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, NaiveTime, Timelike, Utc};
use cron::Schedule as CronExprSchedule;
use std::str::FromStr;

pub fn next_run_for_schedule(schedule: &Schedule, from: DateTime<Utc>) -> Result<DateTime<Utc>> {
    match schedule {
        Schedule::Cron {
            expr,
            tz,
            active_hours,
        } => {
            let normalized = normalize_expression(expr)?;
            let cron = CronExprSchedule::from_str(&normalized)
                .with_context(|| format!("Invalid cron expression: {expr}"))?;
            let timezone = ScheduleTimeZone::parse(tz.as_deref())?;
            // Parsing is cheap; validated at job-creation time via validate_schedule.
            let active_window = active_hours.as_ref().map(ActiveWindow::parse).transpose()?;

            let mut current_from = from;
            for _ in 0..100_000 {
                let next_utc = timezone.next_after(&cron, current_from, expr)?;

                if let Some(active) = &active_window {
                    let local_t = timezone.local_time_of_day(next_utc);
                    if active.contains(local_t) {
                        return Ok(next_utc);
                    }
                    tracing::debug!(
                        "[cron] next_run candidate {} outside active window {}–{}, advancing",
                        next_utc,
                        active.start,
                        active.end
                    );
                    current_from = next_utc;
                } else {
                    return Ok(next_utc);
                }
            }
            tracing::warn!(
                "[cron] no occurrence found within active_hours for expr={} after 100,000 candidates",
                expr
            );
            anyhow::bail!("No future occurrence found within active hours after 100,000 attempts")
        }
        Schedule::At { at } => Ok(*at),
        Schedule::Every { every_ms } => {
            if *every_ms == 0 {
                anyhow::bail!("Invalid schedule: every_ms must be > 0");
            }
            let ms = i64::try_from(*every_ms).context("every_ms is too large")?;
            let delta = ChronoDuration::milliseconds(ms);
            from.checked_add_signed(delta)
                .ok_or_else(|| anyhow::anyhow!("every_ms overflowed DateTime"))
        }
    }
}

pub fn validate_schedule(schedule: &Schedule, now: DateTime<Utc>) -> Result<()> {
    match schedule {
        Schedule::Cron {
            expr,
            tz,
            active_hours,
        } => {
            let _ = normalize_expression(expr)?;
            if let Some(active) = active_hours {
                let _ = ActiveWindow::parse(active)?;
            }
            let _ = ScheduleTimeZone::parse(tz.as_deref())?;
            let _ = next_run_for_schedule(schedule, now)?;
            Ok(())
        }
        Schedule::At { at } => {
            if *at <= now {
                anyhow::bail!("Invalid schedule: 'at' must be in the future");
            }
            Ok(())
        }
        Schedule::Every { every_ms } => {
            if *every_ms == 0 {
                anyhow::bail!("Invalid schedule: every_ms must be > 0");
            }
            Ok(())
        }
    }
}

pub fn schedule_cron_expression(schedule: &Schedule) -> Option<String> {
    match schedule {
        Schedule::Cron { expr, .. } => Some(expr.clone()),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum ScheduleTimeZone {
    Local,
    Named(chrono_tz::Tz),
}

impl ScheduleTimeZone {
    fn parse(tz: Option<&str>) -> Result<Self> {
        match tz {
            Some(tz_name) => chrono_tz::Tz::from_str(tz_name)
                .map(Self::Named)
                .with_context(|| format!("Invalid IANA timezone: {tz_name}")),
            None => Ok(Self::Local),
        }
    }

    fn next_after(
        self,
        cron: &CronExprSchedule,
        from: DateTime<Utc>,
        expr: &str,
    ) -> Result<DateTime<Utc>> {
        match self {
            Self::Named(timezone) => {
                let localized_from = from.with_timezone(&timezone);
                let next_local = cron.after(&localized_from).next().ok_or_else(|| {
                    anyhow::anyhow!("No future occurrence for expression: {expr}")
                })?;
                Ok(next_local.with_timezone(&Utc))
            }
            Self::Local => {
                let localized_from = from.with_timezone(&chrono::Local);
                let next_local = cron.after(&localized_from).next().ok_or_else(|| {
                    anyhow::anyhow!("No future occurrence for expression: {expr}")
                })?;
                Ok(next_local.with_timezone(&Utc))
            }
        }
    }

    fn local_time_of_day(self, time: DateTime<Utc>) -> NaiveTime {
        match self {
            Self::Named(timezone) => {
                let localized = time.with_timezone(&timezone);
                NaiveTime::from_hms_opt(localized.hour(), localized.minute(), 0)
                    .expect("hour() and minute() from a valid DateTime are always in-range")
            }
            Self::Local => {
                let localized = time.with_timezone(&chrono::Local);
                NaiveTime::from_hms_opt(localized.hour(), localized.minute(), 0)
                    .expect("hour() and minute() from a valid DateTime are always in-range")
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ActiveWindow {
    start: NaiveTime,
    end: NaiveTime,
}

impl ActiveWindow {
    fn parse(active: &ActiveHours) -> Result<Self> {
        let start = NaiveTime::parse_from_str(&active.start, "%H:%M")
            .with_context(|| format!("Invalid active_hours.start: {}", active.start))?;
        let end = NaiveTime::parse_from_str(&active.end, "%H:%M")
            .with_context(|| format!("Invalid active_hours.end: {}", active.end))?;
        Ok(Self { start, end })
    }

    fn contains(self, time: NaiveTime) -> bool {
        if self.start <= self.end {
            time >= self.start && time <= self.end
        } else {
            // Window spans midnight (e.g. 22:00 to 06:00).
            time >= self.start || time <= self.end
        }
    }
}

pub fn normalize_expression(expression: &str) -> Result<String> {
    let expression = expression.trim();
    let field_count = expression.split_whitespace().count();

    match field_count {
        // standard crontab syntax: minute hour day month weekday
        5 => Ok(format!("0 {expression}")),
        // crate-native syntax includes seconds (+ optional year)
        6 | 7 => Ok(expression.to_string()),
        _ => anyhow::bail!(
            "Invalid cron expression: {expression} (expected 5, 6, or 7 fields, got {field_count})"
        ),
    }
}

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
