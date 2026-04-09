use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BillingRecord {
    pub billing_id: String,
    pub usage_id: Option<String>,
    pub amount_micros: u64,
    pub currency: String,
}
