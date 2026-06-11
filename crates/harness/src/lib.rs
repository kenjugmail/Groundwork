//! Groundwork's open agentic extraction harness (M2).
//!
//! Turns unstructured local-news documents into typed, provenance-mandatory
//! signals. Built ground-up in the open; reuses *patterns* (prefix-caching
//! discipline, record/replay, drift gates) from closed prior work, none of
//! the code. Binding design rules live in harness/README.md at the repo root.

pub mod anthropic;
pub mod config;
pub mod extract;
pub mod gates;
pub mod geo_places;
pub mod rss;
pub mod verify;

pub use extract::{ExtractedSignal, ExtractedSignals, ExtractionModel, MockModel, ModelOutput, PromptPrefix};
