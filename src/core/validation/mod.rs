//! Validation module for custom validation rules

pub mod engine;
pub mod rules;

pub use engine::ValidationEngine;
pub use rules::{RuleResult, RuleType, Severity, ValidationRule, ValidationRules};
