//! Scenario/store name validation (hoisted from the grouped store so both
//! the runtime stores and the Insta assertion facade share one validator
//! without a dependency cycle).

/// Invalid scenario name: rejected before any path is built from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidName(pub String);

impl std::fmt::Display for InvalidName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid snapshot name: {}", self.0)
    }
}

impl std::error::Error for InvalidName {}

/// Names are relative `/`-separated paths: no absolute paths, no `..` or
/// `.` segments, no empty segments, no backslashes. Anything else would
/// escape the store roots or fail to round-trip through recursive listing.
///
/// # Errors
///
/// Returns [`InvalidName`] when the name is empty, absolute, contains
/// backslashes, or has an empty/`.`/`..` segment.
pub fn validate_name(name: &str) -> Result<(), InvalidName> {
    use std::path::Path;
    let bad = |m: &str| InvalidName(format!("{name:?}: {m}"));
    if name.is_empty() {
        return Err(bad("empty name"));
    }
    if name.starts_with('/') || Path::new(name).is_absolute() {
        return Err(bad("absolute paths are not allowed"));
    }
    if name.contains('\\') {
        return Err(bad("backslashes are not allowed (use `/` separators)"));
    }
    for seg in name.split('/') {
        if seg.is_empty() {
            return Err(bad("empty path segment"));
        }
        if seg == ".." {
            return Err(bad("`..` segments are not allowed"));
        }
        if seg == "." {
            return Err(bad("`.` segments are not allowed"));
        }
    }
    Ok(())
}
