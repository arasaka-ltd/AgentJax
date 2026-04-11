use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    config::RuntimeConfig,
    domain::{Task, TaskCheckpoint, TaskPhase, TaskTimelineEntry},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredTaskRecord {
    pub task: Task,
    pub timeline: Vec<TaskTimelineEntry>,
    pub checkpoints: Vec<TaskCheckpoint>,
}

#[derive(Debug, Clone)]
pub struct TaskStore {
    tasks_dir: PathBuf,
    checkpoints_dir: PathBuf,
}

impl TaskStore {
    pub fn open(runtime_config: &RuntimeConfig) -> Result<Self> {
        fs::create_dir_all(&runtime_config.runtime_paths.tasks_dir).with_context(|| {
            format!(
                "failed to create task state directory {}",
                runtime_config.runtime_paths.tasks_dir.display()
            )
        })?;
        fs::create_dir_all(&runtime_config.runtime_paths.checkpoints_dir).with_context(|| {
            format!(
                "failed to create checkpoint state directory {}",
                runtime_config.runtime_paths.checkpoints_dir.display()
            )
        })?;
        Ok(Self {
            tasks_dir: runtime_config.runtime_paths.tasks_dir.clone(),
            checkpoints_dir: runtime_config.runtime_paths.checkpoints_dir.clone(),
        })
    }

    pub fn list(&self) -> Result<Vec<StoredTaskRecord>> {
        let mut records = Vec::new();
        if !self.tasks_dir.exists() {
            return Ok(records);
        }

        for path in json_files(&self.tasks_dir)? {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read task record {}", path.display()))?;
            records.push(
                serde_json::from_str(&content)
                    .with_context(|| format!("failed to decode task record {}", path.display()))?,
            );
        }

        records.sort_by(|left, right| {
            left.task
                .meta
                .created_at
                .cmp(&right.task.meta.created_at)
                .then_with(|| left.task.task_id.cmp(&right.task.task_id))
        });
        Ok(records)
    }

    pub fn get(&self, task_id: &str) -> Result<Option<StoredTaskRecord>> {
        let path = self.task_path(task_id);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read task record {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to decode task record {}", path.display()))
            .map(Some)
    }

    pub fn upsert(&self, record: StoredTaskRecord) -> Result<StoredTaskRecord> {
        let path = self.task_path(&record.task.task_id);
        let content =
            serde_json::to_string_pretty(&record).context("failed to encode task record json")?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write task record {}", path.display()))?;
        Ok(record)
    }

    pub fn append_timeline(
        &self,
        task_id: &str,
        entry: TaskTimelineEntry,
    ) -> Result<StoredTaskRecord> {
        let mut record = self
            .get(task_id)?
            .with_context(|| format!("task record not found: {task_id}"))?;
        record.timeline.push(entry);
        self.upsert(record)
    }

    pub fn write_checkpoint(
        &self,
        task_id: &str,
        checkpoint: TaskCheckpoint,
    ) -> Result<StoredTaskRecord> {
        let checkpoint_path = self.checkpoint_path(&checkpoint.checkpoint_id);
        let content = serde_json::to_string_pretty(&checkpoint)
            .context("failed to encode task checkpoint json")?;
        fs::write(&checkpoint_path, content)
            .with_context(|| format!("failed to write checkpoint {}", checkpoint_path.display()))?;

        let mut record = self
            .get(task_id)?
            .with_context(|| format!("task record not found: {task_id}"))?;
        record.task.checkpoint_ref = Some(checkpoint.checkpoint_id.clone());
        record.task.meta.updated_at = Utc::now();
        record.checkpoints.push(checkpoint);
        self.upsert(record)
    }

    pub fn update_task(&self, task: Task) -> Result<StoredTaskRecord> {
        let mut record = self
            .get(&task.task_id)?
            .with_context(|| format!("task record not found: {}", task.task_id))?;
        record.task = task;
        self.upsert(record)
    }

    fn task_path(&self, task_id: &str) -> PathBuf {
        self.tasks_dir.join(format!("{task_id}.json"))
    }

    fn checkpoint_path(&self, checkpoint_id: &str) -> PathBuf {
        self.checkpoints_dir.join(format!("{checkpoint_id}.json"))
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

pub fn initial_task_record(task: Task) -> StoredTaskRecord {
    StoredTaskRecord {
        timeline: vec![TaskTimelineEntry {
            entry_id: format!("timeline_{}_created", task.task_id),
            task_id: task.task_id.clone(),
            phase: TaskPhase::Created,
            status: task.status.clone(),
            turn_id: None,
            event_id: None,
            note: "task created".into(),
            recorded_at: task.meta.created_at,
        }],
        checkpoints: Vec::new(),
        task,
    }
}
