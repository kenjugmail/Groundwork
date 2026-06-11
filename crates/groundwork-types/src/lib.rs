//! Shared domain types for Groundwork. Five entities, kept boring and typed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Resolution at which a signal or geo unit is resolved. Coarser-than-tract
/// signals are apportioned to tracts at fusion time, never silently upgraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ResolutionLevel {
    Tract,
    County,
    Place,
    State,
}

impl ResolutionLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolutionLevel::Tract => "tract",
            ResolutionLevel::County => "county",
            ResolutionLevel::Place => "place",
            ResolutionLevel::State => "state",
        }
    }
}

/// Typed signal categories. Each maps to an entry in the fusion weights file;
/// a type with no weight entry contributes nothing (and logs a warning).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    LayoffWarn,
    SurveyFoodInsufficiency,
    SnapEnrollmentChange,
    PantryCapacity,
}

impl SignalType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SignalType::LayoffWarn => "layoff_warn",
            SignalType::SurveyFoodInsufficiency => "survey_food_insufficiency",
            SignalType::SnapEnrollmentChange => "snap_enrollment_change",
            SignalType::PantryCapacity => "pantry_capacity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SignalStatus {
    Active,
    Superseded,
    Quarantined,
}

/// The atom of trust. Immutable once written; corrections insert a
/// superseding row. Always carries a provenance URL and the justifying
/// raw excerpt — a signal not groundable in a quotable span is dropped.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Signal {
    pub id: Uuid,
    pub source_id: String,
    pub geo_unit_id: String,
    pub signal_type: SignalType,
    pub observed_at: DateTime<Utc>,
    pub ingested_at: DateTime<Utc>,
    pub magnitude: f64,
    /// +1 = need rising / supply falling; -1 = the reverse.
    pub direction: i16,
    pub payload: serde_json::Value,
    pub provenance_url: String,
    pub raw_excerpt: String,
    pub raw_capture_id: Option<String>,
    pub resolution_level: ResolutionLevel,
    pub status: SignalStatus,
    pub coverage_flag: Option<String>,
    pub supersedes: Option<Uuid>,
    pub dedupe_key: String,
}

/// A signal as produced by an adapter, before the store assigns identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSignal {
    pub source_id: String,
    pub geo_unit_id: String,
    pub signal_type: SignalType,
    pub observed_at: DateTime<Utc>,
    pub magnitude: f64,
    pub direction: i16,
    pub payload: serde_json::Value,
    pub provenance_url: String,
    pub raw_excerpt: String,
    pub raw_capture_id: Option<String>,
    pub resolution_level: ResolutionLevel,
    /// Natural key for idempotent ingest (source_id + stable identifying fields).
    pub dedupe_key: String,
}

/// One term of the additive sum, stored verbatim on the nowcast row.
/// The attribution sentence is the product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsDecompositionEntry {
    pub signal_id: Uuid,
    pub signal_type: SignalType,
    pub weight: f64,
    pub gameability_discount: f64,
    pub recency_factor: f64,
    pub magnitude: f64,
    pub direction: i16,
    /// Extraction confidence for agentic signals; 1.0 for structured sources.
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    /// weight · discount · recency · confidence · magnitude · direction (after apportionment)
    pub contribution: f64,
}

fn default_confidence() -> f64 {
    1.0
}

/// Fast clock. A nowcast of where need appears to be moving — a signal, not proof.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapNowcast {
    pub geo_unit_id: String,
    pub as_of: DateTime<Utc>,
    pub baseline_gap: f64,
    pub nowcast_gap: f64,
    pub uncertainty: f64,
    pub coverage_score: f64,
    pub news_decomposition: Vec<NewsDecompositionEntry>,
    pub model_version: String,
    pub weights_version: String,
}

/// Slow clock. Verified outcomes over months. Schema only in v0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactRecord {
    pub id: Uuid,
    pub geo_unit_id: String,
    pub intervention_ref: String,
    pub measured_at: DateTime<Utc>,
    pub outcome_metric: String,
    pub value: f64,
    pub method: String,
    pub confidence: f64,
}
