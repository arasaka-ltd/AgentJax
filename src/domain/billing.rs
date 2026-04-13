use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BillingMode {
    Estimated,
    ProviderReported,
    Reconciled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BillingConfidence {
    Low,
    Medium,
    High,
    Exact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BillingBreakdownItem {
    pub item_type: String,
    pub quantity: String,
    pub unit_price: Option<String>,
    pub subtotal: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BillingRecord {
    pub billing_id: String,
    pub usage_id: String,
    pub amount: String,
    pub currency: String,
    pub mode: BillingMode,
    pub rule_id: Option<String>,
    pub confidence: BillingConfidence,
    pub breakdown: Vec<BillingBreakdownItem>,
    pub generated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BillingReconciliation {
    pub reconciliation_id: String,
    pub usage_id: String,
    pub local_estimate_amount: String,
    pub provider_reported_amount: Option<String>,
    pub reconciled_amount: Option<String>,
    pub delta_amount: Option<String>,
    pub currency: String,
    pub last_reconciled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub provider_reference: Option<String>,
    pub note: Option<String>,
}
