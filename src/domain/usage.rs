use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageRecord {
    pub usage_id: String,
    pub metric: String,
    pub value: u64,
    pub unit: String,
}
