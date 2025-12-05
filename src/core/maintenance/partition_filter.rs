//! Extended partition filtering for maintenance operations
//!
//! Supports multiple filter syntaxes:
//! - Exact match: `date=2024-01-01`
//! - Wildcard: `date=2024-01-*` or `date=2024-*`
//! - Multiple filters (AND): `date=2024-01-01,region=us`
//! - Prefix match: `date>=2024-01-01` (coming soon)

use regex::Regex;

/// Partition filter that supports extended filtering syntax
#[derive(Debug, Clone)]
pub struct PartitionFilter {
    /// Raw filter string
    raw: String,
    /// Parsed filter conditions
    conditions: Vec<FilterCondition>,
}

#[derive(Debug, Clone)]
enum FilterCondition {
    /// Exact match: key=value
    Exact { key: String, value: String },
    /// Wildcard match: key=value* or key=*value or key=va*ue
    Wildcard { key: String, pattern: Regex },
    /// Key exists check
    Exists { key: String },
}

impl PartitionFilter {
    /// Parse a filter string into a PartitionFilter
    ///
    /// Supported formats:
    /// - `key=value` - exact match
    /// - `key=value*` - prefix match (wildcard)
    /// - `key=*value` - suffix match (wildcard)
    /// - `key=*` - key exists
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
        let eq_pos = part.find('=').ok_or_else(|| {
            format!(
                "Invalid filter format: '{}'. Expected 'key=value' format.",
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
        })
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
        Err(_) => {
            // Fallback to exact match for backwards compatibility
            partition_key == filter
        }
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
        assert!(matches_partition_filter("date=2024-01-01", "date=2024-01-*"));
        assert!(!matches_partition_filter("date=2024-02-01", "date=2024-01-*"));
    }

    #[test]
    fn test_backwards_compatibility() {
        // Old exact match style should still work
        assert!(matches_partition_filter(
            "date=2024-01-01",
            "date=2024-01-01"
        ));
    }
}
