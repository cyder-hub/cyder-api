use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cost::meter::{ChargeKind, CostUnit, MeterKey};

pub const COST_SNAPSHOT_SCHEMA_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostDetailLine {
    pub meter_key: MeterKey,
    pub quantity: i64,
    pub unit: CostUnit,
    pub charge_kind: ChargeKind,
    pub amount_nanos: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_price_nanos: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_version_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostRatingResult {
    pub total_cost_nanos: i64,
    pub currency: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_lines: Vec<CostDetailLine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_items: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSnapshot {
    pub schema_version: u32,
    pub cost_catalog_id: i64,
    pub cost_catalog_version_id: i64,
    pub total_cost_nanos: i64,
    pub currency: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_lines: Vec<CostDetailLine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_items: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}
