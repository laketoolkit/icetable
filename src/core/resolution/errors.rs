//! Error helper functions for resolution operations

use crate::error::Error;

/// Create error for "no catalog specified"
///
/// Returns an error with helpful message. Use with `?` to propagate.
#[inline]
pub fn no_catalog_error() -> Error {
    Error::NoCatalog
}

/// Create error for "no namespace specified"
///
/// Returns an error with helpful message. Use with `?` to propagate.
#[inline]
pub fn no_namespace_error() -> Error {
    Error::NoNamespace
}

/// Create error for "no table specified"
///
/// Returns an error with helpful message. Use with `?` to propagate.
#[inline]
pub fn no_table_error() -> Error {
    Error::NoTable
}
