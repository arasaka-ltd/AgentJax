use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{config::RuntimeConfig, domain::Schedule};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredScheduleRecord {
    pub schedule: Schedule,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_task_id: Option<String>,
    pub run_count: u64,
}

#[derive(Debug, Clone)]
pub struct ScheduleStore {
    schedules_dir: PathBuf,
}

impl ScheduleStore {
    pub fn open(runtime_config: &RuntimeConfig) -> Result<Self> {
        let schedules_dir = runtime_config.runtime_paths.state_root.join("schedules");
        fs::create_dir_all(&schedules_dir).with_context(|| {
            format!(
                "failed to create schedule state directory {}",
                schedules_dir.display()
            )
        })?;
        Ok(Self { schedules_dir })
    }

    pub fn list(&self) -> Result<Vec<StoredScheduleRecord>> {
        let mut records = Vec::new();
        if !self.schedules_dir.exists() {
            return Ok(records);
        }

        for path in json_files(&self.schedules_dir)? {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read schedule record {}", path.display()))?;
            records.push(
                serde_json::from_str(&content).with_context(|| {
                    format!("failed to decode schedule record {}", path.display())
                })?,
            );
        }

        records.sort_by(|left, right| left.schedule.schedule_id.cmp(&right.schedule.schedule_id));
        Ok(records)
    }

    pub fn get(&self, schedule_id: &str) -> Result<Option<StoredScheduleRecord>> {
        let path = self.schedule_path(schedule_id);
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read schedule record {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to decode schedule record {}", path.display()))
            .map(Some)
    }

    pub fn upsert(&self, schedule: Schedule) -> Result<StoredScheduleRecord> {
        let now = Utc::now();
        let mut record = self
            .get(&schedule.schedule_id)?
            .unwrap_or_else(|| StoredScheduleRecord {
                next_run_at: next_run_at(&schedule, None, now),
                last_run_at: None,
                last_task_id: None,
                run_count: 0,
                schedule: schedule.clone(),
            });
        record.schedule = schedule;
        if !record.schedule.enabled {
            record.next_run_at = None;
        } else if record.next_run_at.is_none() {
            record.next_run_at = next_run_at(&record.schedule, record.last_run_at, now);
        }
        self.write_record(&record)
    }

    pub fn delete(&self, schedule_id: &str) -> Result<Option<StoredScheduleRecord>> {
        let existing = self.get(schedule_id)?;
        if existing.is_some() {
            fs::remove_file(self.schedule_path(schedule_id)).with_context(|| {
                format!(
                    "failed to delete schedule {}",
                    self.schedule_path(schedule_id).display()
                )
            })?;
        }
        Ok(existing)
    }

    pub fn due_records(&self, now: DateTime<Utc>) -> Result<Vec<StoredScheduleRecord>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|record| {
                record.schedule.enabled
                    && record
                        .next_run_at
                        .is_some_and(|next_run_at| next_run_at <= now)
            })
            .collect())
    }

    pub fn mark_triggered(
        &self,
        schedule_id: &str,
        task_id: Option<&str>,
        triggered_at: DateTime<Utc>,
    ) -> Result<StoredScheduleRecord> {
        let mut record = self
            .get(schedule_id)?
            .with_context(|| format!("schedule record not found: {schedule_id}"))?;
        record.last_run_at = Some(triggered_at);
        record.last_task_id = task_id.map(str::to_string);
        record.run_count = record.run_count.saturating_add(1);
        record.next_run_at = next_run_at(&record.schedule, Some(triggered_at), triggered_at);
        self.write_record(&record)
    }

    fn write_record(&self, record: &StoredScheduleRecord) -> Result<StoredScheduleRecord> {
        let path = self.schedule_path(&record.schedule.schedule_id);
        let content =
            serde_json::to_string_pretty(record).context("failed to encode schedule json")?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write schedule record {}", path.display()))?;
        Ok(record.clone())
    }

    fn schedule_path(&self, schedule_id: &str) -> PathBuf {
        self.schedules_dir.join(format!("{schedule_id}.json"))
    }
}

fn next_run_at(
    schedule: &Schedule,
    last_run_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if !schedule.enabled {
        return None;
    }

    match schedule.trigger {
        crate::domain::TaskTrigger::Interval { seconds } => {
            let base = last_run_at.unwrap_or(now);
            Some(base + Duration::seconds(seconds as i64))
        }
        _ => None,
    }
}

fn json_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|item| item.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(files)
}
