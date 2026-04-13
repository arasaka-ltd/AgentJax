use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    config::RuntimeConfig,
    domain::{BillingRecord, UsageRecord},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UsageSummary {
    pub records_total: usize,
    pub input_tokens_total: u64,
    pub output_tokens_total: u64,
    pub cached_tokens_total: u64,
    pub reasoning_tokens_total: u64,
    pub estimated_usd_total: String,
}

#[derive(Debug, Clone)]
pub struct LedgerStore {
    usage_path: PathBuf,
    billing_path: PathBuf,
}

impl LedgerStore {
    pub fn open(runtime_config: &RuntimeConfig) -> Result<Self> {
        let usage_dir = runtime_config.runtime_paths.state_root.join("usage");
        let billing_dir = runtime_config.runtime_paths.state_root.join("billing");
        fs::create_dir_all(&usage_dir).with_context(|| {
            format!(
                "failed to create usage state directory {}",
                usage_dir.display()
            )
        })?;
        fs::create_dir_all(&billing_dir).with_context(|| {
            format!(
                "failed to create billing state directory {}",
                billing_dir.display()
            )
        })?;
        Ok(Self {
            usage_path: usage_dir.join("ledger.json"),
            billing_path: billing_dir.join("ledger.json"),
        })
    }

    pub fn list_usage(&self) -> Result<Vec<UsageRecord>> {
        read_json_file(&self.usage_path)
    }

    pub fn append_usage(&self, record: UsageRecord) -> Result<UsageRecord> {
        let mut records = self.list_usage()?;
        records.push(record.clone());
        write_json_file(&self.usage_path, &records)?;
        Ok(record)
    }

    pub fn list_billing(&self) -> Result<Vec<BillingRecord>> {
        read_json_file(&self.billing_path)
    }

    pub fn append_billing(&self, record: BillingRecord) -> Result<BillingRecord> {
        let mut records = self.list_billing()?;
        records.push(record.clone());
        write_json_file(&self.billing_path, &records)?;
        Ok(record)
    }

    pub fn usage_summary(&self) -> Result<UsageSummary> {
        let usage = self.list_usage()?;
        let billing = self.list_billing()?;
        let input_tokens_total = usage.iter().filter_map(|record| record.input_tokens).sum();
        let output_tokens_total = usage.iter().filter_map(|record| record.output_tokens).sum();
        let cached_tokens_total = usage.iter().filter_map(|record| record.cached_tokens).sum();
        let reasoning_tokens_total = usage
            .iter()
            .filter_map(|record| record.reasoning_tokens)
            .sum();
        let estimated_total = billing.iter().fold(0.0_f64, |acc, record| {
            acc + record.amount.parse::<f64>().unwrap_or(0.0)
        });

        Ok(UsageSummary {
            records_total: usage.len(),
            input_tokens_total,
            output_tokens_total,
            cached_tokens_total,
            reasoning_tokens_total,
            estimated_usd_total: format!("{estimated_total:.8}"),
        })
    }
}

fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: serde::de::DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read ledger file {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to decode ledger file {}", path.display()))
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let content = serde_json::to_string_pretty(value).context("failed to encode ledger json")?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}
