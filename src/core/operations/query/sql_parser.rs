//! SQL parsing utilities for extracting file paths from queries

use regex::Regex;
use std::sync::OnceLock;

use crate::error::{Error, Result};

/// Reference to a file in SQL query
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReference {
    /// Original file path from SQL
    pub path: String,

    /// Optional alias (AS clause or implicit)
    pub alias: Option<String>,

    /// Table name to use in DataFusion (alias or derived from path)
    pub table_name: String,
}

impl FileReference {
    /// Create a new file reference
    pub fn new(path: String, alias: Option<String>) -> Self {
        let table_name = if let Some(ref a) = alias {
            // User provided explicit alias - use it
            a.clone()
        } else {
            // No alias - extract filename without extension
            // Example: "data/flights.parquet" -> "flights"
            use std::path::Path;
            let path_obj = Path::new(&path);
            path_obj
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("table")
                .to_string()
        };

        Self {
            path,
            alias,
            table_name,
        }
    }
}

/// Extracts file paths from SQL queries
pub struct SqlPathExtractor;

impl SqlPathExtractor {
    /// Get the compiled regex pattern (cached)
    fn pattern() -> &'static Regex {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| {
            // Pattern matches:
            // - Case-insensitive FROM or JOIN
            // - Followed by whitespace
            // - Single or double quoted file path
            // - Optional AS keyword with alias OR just identifier (capture both)
            Regex::new(r#"(?i)(FROM|JOIN)\s+['"]([^'"]+)['"]\s*(?:AS\s+)?(\w+)?"#)
                .expect("Failed to compile SQL path extraction regex")
        })
    }

    /// Extract all file references from a SQL query
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let refs = SqlPathExtractor::extract_file_paths(
    ///     "SELECT * FROM 'data/flights.parquet' f WHERE year > 2020"
    /// )?;
    /// assert_eq!(refs[0].path, "data/flights.parquet");
    /// assert_eq!(refs[0].alias, Some("f".to_string()));
    /// ```
    pub fn extract_file_paths(sql: &str) -> Result<Vec<FileReference>> {
        let pattern = Self::pattern();
        let mut references = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();

        for cap in pattern.captures_iter(sql) {
            // cap[1] = FROM or JOIN (ignored)
            // cap[2] = file path
            // cap[3] = optional alias

            let path = cap
                .get(2)
                .ok_or_else(|| Error::parse("Failed to extract file path from SQL"))?
                .as_str()
                .to_string();

            let alias = cap.get(3).map(|m| m.as_str().to_string());

            // Only add unique paths (same path can appear multiple times in complex queries)
            if !seen_paths.contains(&path) {
                seen_paths.insert(path.clone());
                references.push(FileReference::new(path, alias));
            }
        }

        if references.is_empty() {
            return Err(Error::parse(
                "No file paths found in SQL query. File paths must be quoted (e.g., 'data.parquet')",
            ));
        }

        Ok(references)
    }

    /// Validate that all file paths in SQL are quoted
    ///
    /// This helps provide better error messages to users who forget to quote paths
    pub fn validate_quoted_paths(sql: &str) -> Result<()> {
        // Check for common mistakes: FROM tablename or JOIN tablename without quotes
        let unquoted_pattern = Regex::new(r"(?i)(FROM|JOIN)\s+([a-zA-Z_][\w./]*)")
            .expect("Failed to compile unquoted path detection regex");

        if let Some(cap) = unquoted_pattern.captures(sql) {
            let keyword = cap.get(1).unwrap().as_str();
            let identifier = cap.get(2).unwrap().as_str();

            // Check if it looks like a file path (contains / or .)
            if identifier.contains('/') || identifier.contains('.') {
                return Err(Error::parse(format!(
                    "File path '{}' after {} must be quoted. Use {} '{}' instead",
                    identifier, keyword, keyword, identifier
                )));
            }
        }

        Ok(())
    }

    /// Rewrite SQL query to replace quoted file paths with table names
    ///
    /// Takes the original SQL and a list of file references, and replaces
    /// each quoted file path with its corresponding table name.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let sql = "SELECT * FROM 'data/flights.parquet' f WHERE year > 2020";
    /// let refs = vec![FileReference::new("data/flights.parquet".to_string(), Some("f".to_string()))];
    /// let rewritten = SqlPathExtractor::rewrite_sql(sql, &refs);
    /// // Result: "SELECT * FROM f f WHERE year > 2020"
    /// // Or without alias: "SELECT * FROM flights WHERE year > 2020"
    /// ```
    pub fn rewrite_sql(sql: &str, file_refs: &[FileReference]) -> String {
        let mut rewritten = sql.to_string();

        // Sort by path length descending to handle longer paths first
        // This prevents partial replacements
        let mut sorted_refs = file_refs.to_vec();
        sorted_refs.sort_by(|a, b| b.path.len().cmp(&a.path.len()));

        for file_ref in sorted_refs {
            // Create patterns for both single and double quotes
            let patterns = [
                format!("'{}'", file_ref.path),
                format!("\"{}\"", file_ref.path),
            ];

            for pattern in &patterns {
                // Replace quoted path with table name
                rewritten = rewritten.replace(pattern, &file_ref.table_name);
            }
        }

        rewritten
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_single_file() {
        let sql = "SELECT * FROM 'data/flights.parquet'";
        let refs = SqlPathExtractor::extract_file_paths(sql).unwrap();

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "data/flights.parquet");
        assert_eq!(refs[0].alias, None);
        assert_eq!(refs[0].table_name, "flights");
    }

    #[test]
    fn test_extract_with_alias() {
        let sql = "SELECT * FROM 'data/flights.parquet' f";
        let refs = SqlPathExtractor::extract_file_paths(sql).unwrap();

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "data/flights.parquet");
        assert_eq!(refs[0].alias, Some("f".to_string()));
        assert_eq!(refs[0].table_name, "f");
    }

    #[test]
    fn test_extract_with_as_alias() {
        let sql = "SELECT * FROM 'data/flights.parquet' AS flights";
        let refs = SqlPathExtractor::extract_file_paths(sql).unwrap();

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "data/flights.parquet");
        assert_eq!(refs[0].alias, Some("flights".to_string()));
        assert_eq!(refs[0].table_name, "flights");
    }

    #[test]
    fn test_extract_join() {
        let sql = r#"
            SELECT f.*, a.name
            FROM "data/flights.parquet" f
            JOIN 'data/airlines.csv' AS a
            ON f.airline_id = a.id
        "#;
        let refs = SqlPathExtractor::extract_file_paths(sql).unwrap();

        assert_eq!(refs.len(), 2);

        assert_eq!(refs[0].path, "data/flights.parquet");
        assert_eq!(refs[0].alias, Some("f".to_string()));

        assert_eq!(refs[1].path, "data/airlines.csv");
        assert_eq!(refs[1].alias, Some("a".to_string()));
    }

    #[test]
    fn test_case_insensitive() {
        let sql = "select * from 'data.parquet' where year > 2020";
        let refs = SqlPathExtractor::extract_file_paths(sql).unwrap();

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "data.parquet");
    }

    #[test]
    fn test_paths_with_spaces() {
        let sql = r#"SELECT * FROM "data files/my data.parquet" AS t"#;
        let refs = SqlPathExtractor::extract_file_paths(sql).unwrap();

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "data files/my data.parquet");
        assert_eq!(refs[0].alias, Some("t".to_string()));
    }

    #[test]
    fn test_no_paths_error() {
        let sql = "SELECT 1 + 1";
        let result = SqlPathExtractor::extract_file_paths(sql);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No file paths found")
        );
    }

    #[test]
    fn test_duplicate_paths() {
        // Same path used twice should only appear once in results
        let sql = r#"
            SELECT * FROM 'data.parquet' t1
            UNION ALL
            SELECT * FROM 'data.parquet' t2
        "#;
        let refs = SqlPathExtractor::extract_file_paths(sql).unwrap();

        // Should only get one reference even though path appears twice
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "data.parquet");
    }

    #[test]
    fn test_validate_quoted_paths() {
        // Unquoted path with slash should error
        let sql = "SELECT * FROM data/file.parquet";
        let result = SqlPathExtractor::validate_quoted_paths(sql);
        assert!(result.is_err());

        // Quoted path should be OK
        let sql = "SELECT * FROM 'data/file.parquet'";
        let result = SqlPathExtractor::validate_quoted_paths(sql);
        assert!(result.is_ok());

        // Unquoted identifier (table name) without special chars is OK
        let sql = "SELECT * FROM mytable";
        let result = SqlPathExtractor::validate_quoted_paths(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rewrite_sql_single_file() {
        let sql = "SELECT * FROM 'data/flights.parquet' WHERE year > 2020";
        let refs = vec![FileReference::new("data/flights.parquet".to_string(), None)];
        let rewritten = SqlPathExtractor::rewrite_sql(sql, &refs);
        assert_eq!(rewritten, "SELECT * FROM flights WHERE year > 2020");
    }

    #[test]
    fn test_rewrite_sql_with_alias() {
        let sql = "SELECT * FROM 'data/flights.parquet' f WHERE f.year > 2020";
        let refs = vec![FileReference::new(
            "data/flights.parquet".to_string(),
            Some("f".to_string()),
        )];
        let rewritten = SqlPathExtractor::rewrite_sql(sql, &refs);
        assert_eq!(rewritten, "SELECT * FROM f f WHERE f.year > 2020");
    }

    #[test]
    fn test_rewrite_sql_join() {
        let sql = r#"SELECT f.*, a.name FROM 'data/flights.parquet' f JOIN "data/airlines.csv" a ON f.id = a.id"#;
        let refs = vec![
            FileReference::new("data/flights.parquet".to_string(), Some("f".to_string())),
            FileReference::new("data/airlines.csv".to_string(), Some("a".to_string())),
        ];
        let rewritten = SqlPathExtractor::rewrite_sql(sql, &refs);
        assert_eq!(
            rewritten,
            "SELECT f.*, a.name FROM f f JOIN a a ON f.id = a.id"
        );
    }
}
