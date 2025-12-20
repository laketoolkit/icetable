//! Extended partition filtering for maintenance operations
//!
//! Supports multiple filter syntaxes:
//! - Exact match: `date=2024-01-01`
//! - Wildcard: `date=2024-01-*` or `date=2024-*`
//! - Multiple filters (AND): `date=2024-01-01,region=us`
//! - Range operators: `date>=2024-01-01`, `date<2024-02-01`
//! - Combined range: `date>=2024-01-01,date<2024-02-01`

use regex::Regex;

/// Partition filter that supports extended filtering syntax
#[derive(Debug, Clone)]
pub struct PartitionFilter {
    /// Raw filter string
    raw: String,
    /// Parsed filter conditions
    conditions: Vec<FilterCondition>,
}

/// Comparison operator for range filters
#[derive(Debug, Clone, Copy, PartialEq)]
enum CompareOp {
    GreaterThanOrEqual, // >=
    GreaterThan,        // >
    LessThanOrEqual,    // <=
    LessThan,           // <
}

#[derive(Debug, Clone)]
enum FilterCondition {
    /// Exact match: key=value
    Exact { key: String, value: String },
    /// Wildcard match: key=value* or key=*value or key=va*ue
    Wildcard { key: String, pattern: Regex },
    /// Key exists check
    Exists { key: String },
    /// Range comparison: key>=value, key<value, etc.
    Range {
        key: String,
        op: CompareOp,
        value: String,
    },
}

impl PartitionFilter {
    /// Parse a filter string into a PartitionFilter
    ///
    /// Supported formats:
    /// - `key=value` - exact match
    /// - `key=value*` - prefix match (wildcard)
    /// - `key=*value` - suffix match (wildcard)
    /// - `key=*` - key exists
    /// - `key>=value` - greater than or equal
    /// - `key>value` - greater than
    /// - `key<=value` - less than or equal
    /// - `key<value` - less than
    /// - `key1=val1,key2=val2` - multiple conditions (AND)
    pub fn parse(filter: &str) -> Result<Self, String> {
        let conditions = filter
            .split(',')
            .map(|part| Self::parse_condition(part.trim()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            raw: filter.to_string(),
            conditions,
        })
    }

    fn parse_condition(part: &str) -> Result<FilterCondition, String> {
        // Check for range operators first (order matters: >= before >, <= before <)
        if let Some(idx) = part.find(">=") {
            let key = part[..idx].trim().to_string();
            let value = part[idx + 2..].trim().to_string();
            if key.is_empty() || value.is_empty() {
                return Err(format!("Invalid filter: '{}'", part));
            }
            return Ok(FilterCondition::Range {
                key,
                op: CompareOp::GreaterThanOrEqual,
                value,
            });
        }
        if let Some(idx) = part.find("<=") {
            let key = part[..idx].trim().to_string();
            let value = part[idx + 2..].trim().to_string();
            if key.is_empty() || value.is_empty() {
                return Err(format!("Invalid filter: '{}'", part));
            }
            return Ok(FilterCondition::Range {
                key,
                op: CompareOp::LessThanOrEqual,
                value,
            });
        }
        // Check single operators (but not inside a value after =)
        if let Some(idx) = part.find('>') {
            // Make sure it's not after an = sign
            if part.find('=').is_none_or(|eq_idx| idx < eq_idx) {
                let key = part[..idx].trim().to_string();
                let value = part[idx + 1..].trim().to_string();
                if key.is_empty() || value.is_empty() {
                    return Err(format!("Invalid filter: '{}'", part));
                }
                return Ok(FilterCondition::Range {
                    key,
                    op: CompareOp::GreaterThan,
                    value,
                });
            }
        }
        if let Some(idx) = part.find('<') {
            // Make sure it's not after an = sign
            if part.find('=').is_none_or(|eq_idx| idx < eq_idx) {
                let key = part[..idx].trim().to_string();
                let value = part[idx + 1..].trim().to_string();
                if key.is_empty() || value.is_empty() {
                    return Err(format!("Invalid filter: '{}'", part));
                }
                return Ok(FilterCondition::Range {
                    key,
                    op: CompareOp::LessThan,
                    value,
                });
            }
        }

        // Standard exact match or wildcard
        let eq_pos = part.find('=').ok_or_else(|| {
            format!(
                "Invalid filter format: '{}'. Expected 'key=value' or 'key>=value' format.",
                part
            )
        })?;

        let key = part[..eq_pos].trim().to_string();
        let value = part[eq_pos + 1..].trim().to_string();

        if key.is_empty() {
            return Err("Filter key cannot be empty".to_string());
        }

        // Check if it's a wildcard pattern
        if value.contains('*') {
            if value == "*" {
                // Key exists check
                return Ok(FilterCondition::Exists { key });
            }

            // Convert wildcard pattern to regex
            let regex_pattern = format!(
                "^{}$",
                value
                    .split('*')
                    .map(regex::escape)
                    .collect::<Vec<_>>()
                    .join(".*")
            );

            let pattern = Regex::new(&regex_pattern)
                .map_err(|e| format!("Invalid wildcard pattern: {}", e))?;

            return Ok(FilterCondition::Wildcard { key, pattern });
        }

        Ok(FilterCondition::Exact { key, value })
    }

    /// Check if a partition key matches this filter
    ///
    /// The partition_key is expected to be in format: `key1=val1/key2=val2` or `key=value`
    pub fn matches(&self, partition_key: &str) -> bool {
        // Parse the partition key into a map
        let parts: std::collections::HashMap<&str, &str> = partition_key
            .split('/')
            .filter_map(|part| {
                let eq_pos = part.find('=')?;
                Some((&part[..eq_pos], &part[eq_pos + 1..]))
            })
            .collect();

        // All conditions must match (AND)
        self.conditions.iter().all(|cond| match cond {
            FilterCondition::Exact { key, value } => {
                parts.get(key.as_str()).is_some_and(|v| *v == value)
            }
            FilterCondition::Wildcard { key, pattern } => {
                parts.get(key.as_str()).is_some_and(|v| pattern.is_match(v))
            }
            FilterCondition::Exists { key } => parts.contains_key(key.as_str()),
            FilterCondition::Range { key, op, value } => parts
                .get(key.as_str())
                .is_some_and(|v| Self::compare_values(v, value, *op)),
        })
    }

    /// Compare two string values using the given operator
    /// Uses lexicographic comparison which works well for ISO dates and numeric strings
    fn compare_values(actual: &str, filter_value: &str, op: CompareOp) -> bool {
        match op {
            CompareOp::GreaterThanOrEqual => actual >= filter_value,
            CompareOp::GreaterThan => actual > filter_value,
            CompareOp::LessThanOrEqual => actual <= filter_value,
            CompareOp::LessThan => actual < filter_value,
        }
    }

    /// Get the raw filter string
    pub fn raw(&self) -> &str {
        &self.raw
    }
}

/// Helper function to check if a partition matches a filter string
pub fn matches_partition_filter(partition_key: &str, filter: &str) -> bool {
    match PartitionFilter::parse(filter) {
        Ok(f) => f.matches(partition_key),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let filter = PartitionFilter::parse("date=2024-01-01").unwrap();
        assert!(filter.matches("date=2024-01-01"));
        assert!(!filter.matches("date=2024-01-02"));
        assert!(!filter.matches("date=2024-01"));
    }

    #[test]
    fn test_wildcard_prefix() {
        let filter = PartitionFilter::parse("date=2024-01-*").unwrap();
        assert!(filter.matches("date=2024-01-01"));
        assert!(filter.matches("date=2024-01-15"));
        assert!(filter.matches("date=2024-01-31"));
        assert!(!filter.matches("date=2024-02-01"));
    }

    #[test]
    fn test_wildcard_suffix() {
        let filter = PartitionFilter::parse("region=*-west").unwrap();
        assert!(filter.matches("region=us-west"));
        assert!(filter.matches("region=eu-west"));
        assert!(!filter.matches("region=us-east"));
    }

    #[test]
    fn test_wildcard_middle() {
        let filter = PartitionFilter::parse("region=us-*-1").unwrap();
        assert!(filter.matches("region=us-east-1"));
        assert!(filter.matches("region=us-west-1"));
        assert!(!filter.matches("region=us-east-2"));
    }

    #[test]
    fn test_exists_check() {
        let filter = PartitionFilter::parse("date=*").unwrap();
        assert!(filter.matches("date=2024-01-01"));
        assert!(filter.matches("date=anything"));
        assert!(!filter.matches("region=us"));
    }

    #[test]
    fn test_multiple_conditions() {
        let filter = PartitionFilter::parse("date=2024-01-01,region=us").unwrap();
        assert!(filter.matches("date=2024-01-01/region=us"));
        assert!(!filter.matches("date=2024-01-01/region=eu"));
        assert!(!filter.matches("date=2024-01-02/region=us"));
        assert!(!filter.matches("date=2024-01-01"));
    }

    #[test]
    fn test_multiple_with_wildcard() {
        let filter = PartitionFilter::parse("date=2024-01-*,region=us").unwrap();
        assert!(filter.matches("date=2024-01-01/region=us"));
        assert!(filter.matches("date=2024-01-15/region=us"));
        assert!(!filter.matches("date=2024-02-01/region=us"));
        assert!(!filter.matches("date=2024-01-01/region=eu"));
    }

    #[test]
    fn test_helper_function() {
        assert!(matches_partition_filter(
            "date=2024-01-01",
            "date=2024-01-*"
        ));
        assert!(!matches_partition_filter(
            "date=2024-02-01",
            "date=2024-01-*"
        ));
    }

    #[test]
    fn test_exact_match_helper() {
        assert!(matches_partition_filter(
            "date=2024-01-01",
            "date=2024-01-01"
        ));
    }

    #[test]
    fn test_range_greater_than_or_equal() {
        let filter = PartitionFilter::parse("date>=2024-01-15").unwrap();
        assert!(filter.matches("date=2024-01-15")); // Equal
        assert!(filter.matches("date=2024-01-16")); // Greater
        assert!(filter.matches("date=2024-02-01")); // Greater
        assert!(!filter.matches("date=2024-01-14")); // Less
        assert!(!filter.matches("date=2024-01-01")); // Less
    }

    #[test]
    fn test_range_less_than() {
        let filter = PartitionFilter::parse("date<2024-02-01").unwrap();
        assert!(filter.matches("date=2024-01-31")); // Less
        assert!(filter.matches("date=2024-01-01")); // Less
        assert!(!filter.matches("date=2024-02-01")); // Equal
        assert!(!filter.matches("date=2024-02-15")); // Greater
    }

    #[test]
    fn test_range_combined() {
        // Range: January 2024
        let filter = PartitionFilter::parse("date>=2024-01-01,date<2024-02-01").unwrap();
        assert!(filter.matches("date=2024-01-01")); // Start of range
        assert!(filter.matches("date=2024-01-15")); // Middle
        assert!(filter.matches("date=2024-01-31")); // End of month
        assert!(!filter.matches("date=2023-12-31")); // Before
        assert!(!filter.matches("date=2024-02-01")); // After
    }

    #[test]
    fn test_range_with_other_conditions() {
        // Range + exact match
        let filter = PartitionFilter::parse("date>=2024-01-01,date<2024-02-01,region=us").unwrap();
        assert!(filter.matches("date=2024-01-15/region=us"));
        assert!(!filter.matches("date=2024-01-15/region=eu")); // Wrong region
        assert!(!filter.matches("date=2024-02-15/region=us")); // Out of range
    }

    #[test]
    fn test_range_greater_than() {
        let filter = PartitionFilter::parse("date>2024-01-15").unwrap();
        assert!(!filter.matches("date=2024-01-15")); // Not strictly greater
        assert!(filter.matches("date=2024-01-16")); // Greater
    }

    #[test]
    fn test_range_less_than_or_equal() {
        let filter = PartitionFilter::parse("date<=2024-01-15").unwrap();
        assert!(filter.matches("date=2024-01-15")); // Equal
        assert!(filter.matches("date=2024-01-14")); // Less
        assert!(!filter.matches("date=2024-01-16")); // Greater
    }
}
