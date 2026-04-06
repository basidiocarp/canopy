use glob::Pattern;
use serde::{Deserialize, Serialize};

/// Check if two scope lists have overlapping paths.
///
/// Returns a list of human-readable overlap descriptions.
#[must_use]
pub fn scope_overlaps(scope_a: &[String], scope_b: &[String]) -> Vec<String> {
    let mut overlaps = Vec::new();

    for path_a in scope_a {
        for path_b in scope_b {
            if paths_overlap(path_a, path_b) {
                overlaps.push(format!("{path_a} <-> {path_b}"));
            }
        }
    }

    overlaps
}

/// Describes a scope gap detected between the active work item and declared scope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeGap {
    Blocking { description: String },
    NonBlocking { description: String },
}

/// Classify a work item against the handoff scope.
///
/// Returns `None` when the work item is in scope or does not reference any
/// concrete out-of-scope files. Returns a blocking gap when the out-of-scope
/// work is required to proceed, and non-blocking when the work is additive.
#[must_use]
pub fn classify_scope_gap(work_item: &str, handoff_scope: &[String]) -> Option<ScopeGap> {
    let gap_paths = scope_gap_paths(work_item, handoff_scope);
    if gap_paths.is_empty() {
        return None;
    }

    let description = format_scope_gap_description(work_item, &gap_paths);
    if is_non_blocking_scope_gap(work_item) && !is_blocking_scope_gap(work_item) {
        Some(ScopeGap::NonBlocking { description })
    } else {
        Some(ScopeGap::Blocking { description })
    }
}

/// Determines whether two path specifications overlap.
///
/// Handles exact matches, directory containment, and glob patterns.
fn paths_overlap(a: &str, b: &str) -> bool {
    // Exact match
    if a == b {
        return true;
    }

    // Directory containment: treat paths without extensions or ending with /
    // as directories
    if is_directory_path(a) {
        let dir = a.trim_end_matches('/');
        if b.starts_with(dir) && b[dir.len()..].starts_with('/') {
            return true;
        }
    }
    if is_directory_path(b) {
        let dir = b.trim_end_matches('/');
        if a.starts_with(dir) && a[dir.len()..].starts_with('/') {
            return true;
        }
    }

    // Glob matching
    if let Ok(pattern) = Pattern::new(a) {
        if pattern.matches(b) {
            return true;
        }
    }
    if let Ok(pattern) = Pattern::new(b) {
        if pattern.matches(a) {
            return true;
        }
    }

    false
}

fn scope_gap_paths(work_item: &str, handoff_scope: &[String]) -> Vec<String> {
    extract_step_scope(work_item)
        .into_iter()
        .filter(|path| !is_path_covered_by_scope(path, handoff_scope))
        .collect()
}

fn is_path_covered_by_scope(path: &str, handoff_scope: &[String]) -> bool {
    handoff_scope
        .iter()
        .any(|scope_path| paths_overlap(path, scope_path))
}

fn format_scope_gap_description(work_item: &str, gap_paths: &[String]) -> String {
    let summary = work_item.lines().next().unwrap_or(work_item).trim();
    if gap_paths.is_empty() {
        summary.to_string()
    } else {
        format!("{summary} [gap: {}]", gap_paths.join(", "))
    }
}

fn is_non_blocking_scope_gap(work_item: &str) -> bool {
    let lower = work_item.to_ascii_lowercase();
    [
        "optional",
        "follow-up",
        "follow up",
        "nice to have",
        "stretch",
        "if time",
        "later",
        "deferred",
        "additive",
        "bonus",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

fn is_blocking_scope_gap(work_item: &str) -> bool {
    let lower = work_item.to_ascii_lowercase();
    [
        "blocked",
        "cannot proceed",
        "can't proceed",
        "can't continue",
        "need",
        "required",
        "must",
        "depends on",
        "before continuing",
        "out of scope",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

/// Heuristic: a path is a directory if it ends with `/` or has no file extension.
fn is_directory_path(path: &str) -> bool {
    if path.ends_with('/') {
        return true;
    }
    // If the last component has no `.`, treat as directory
    let last = path.rsplit('/').next().unwrap_or(path);
    !last.contains('.')
}

/// Extract file paths from backtick-quoted references in step content.
///
/// Looks for lines containing `path/to/file` where the backticked text
/// looks like a file path (contains `/` or `.`).
#[must_use]
pub fn extract_step_scope(step_content: &str) -> Vec<String> {
    let mut files = Vec::new();

    for line in step_content.lines() {
        for path in extract_backtick_paths(line) {
            if path.contains('/') || path.contains('.') {
                files.push(path.to_string());
            }
        }
    }

    files.sort();
    files.dedup();
    files
}

/// Extract all backtick-quoted paths from a single line.
fn extract_backtick_paths(line: &str) -> Vec<&str> {
    let mut paths = Vec::new();
    let mut remaining = line;

    while let Some(start_idx) = remaining.find('`') {
        let after_tick = &remaining[start_idx + 1..];
        if let Some(end_idx) = after_tick.find('`') {
            let candidate = &after_tick[..end_idx];
            // Must look like a path (has extension or directory separator)
            // and not look like code (no spaces, no parentheses)
            if (candidate.contains('/') || candidate.contains('.'))
                && !candidate.contains(' ')
                && !candidate.contains('(')
            {
                paths.push(candidate);
            }
            remaining = &after_tick[end_idx + 1..];
        } else {
            break;
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_path_match() {
        let a = vec!["src/auth.rs".to_string()];
        let b = vec!["src/auth.rs".to_string()];
        let overlaps = scope_overlaps(&a, &b);
        assert_eq!(overlaps.len(), 1);
        assert!(overlaps[0].contains("src/auth.rs"));
    }

    #[test]
    fn directory_containment() {
        let a = vec!["src/hooks".to_string()];
        let b = vec!["src/hooks/pre_tool_use.rs".to_string()];
        let overlaps = scope_overlaps(&a, &b);
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn directory_containment_trailing_slash() {
        let a = vec!["src/hooks/".to_string()];
        let b = vec!["src/hooks/pre_tool_use.rs".to_string()];
        let overlaps = scope_overlaps(&a, &b);
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn glob_match() {
        let a = vec!["src/**".to_string()];
        let b = vec!["src/auth.rs".to_string()];
        let overlaps = scope_overlaps(&a, &b);
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn no_overlap() {
        let a = vec!["src/auth.rs".to_string()];
        let b = vec!["tests/auth_test.rs".to_string()];
        let overlaps = scope_overlaps(&a, &b);
        assert!(overlaps.is_empty());
    }

    #[test]
    fn multiple_overlaps() {
        let a = vec!["src/auth.rs".to_string(), "src/middleware.rs".to_string()];
        let b = vec!["src/auth.rs".to_string(), "src/routes.rs".to_string()];
        let overlaps = scope_overlaps(&a, &b);
        assert_eq!(overlaps.len(), 1); // only auth.rs overlaps
    }

    #[test]
    fn empty_scope_no_overlap() {
        let a: Vec<String> = Vec::new();
        let b = vec!["src/auth.rs".to_string()];
        let overlaps = scope_overlaps(&a, &b);
        assert!(overlaps.is_empty());
    }

    #[test]
    fn extract_step_scope_parses_file_paths() {
        let content = r#"
**File:** `canopy/src/models.rs`

Add the scope field:

```rust
pub scope: Vec<String>,
```

**File:** `canopy/src/store.rs`
"#;
        let scope = extract_step_scope(content);
        assert_eq!(scope, vec!["canopy/src/models.rs", "canopy/src/store.rs"]);
    }

    #[test]
    fn extract_step_scope_ignores_non_paths() {
        let content = "`cargo test` should pass and `Vec<String>` is a type";
        let scope = extract_step_scope(content);
        assert!(scope.is_empty());
    }

    #[test]
    fn extract_backtick_paths_multiple() {
        let line = "Modify `src/auth.rs` and `src/middleware.rs` for this change";
        let paths = extract_backtick_paths(line);
        assert_eq!(paths, vec!["src/auth.rs", "src/middleware.rs"]);
    }

    #[test]
    fn classify_scope_gap_returns_none_when_in_scope() {
        let scope = vec!["canopy/src/models.rs".to_string()];
        let work_item = "Update `canopy/src/models.rs` and keep the API aligned";
        assert!(classify_scope_gap(work_item, &scope).is_none());
    }

    #[test]
    fn classify_scope_gap_marks_blocking_out_of_scope_paths() {
        let scope = vec!["canopy/src/models.rs".to_string()];
        let work_item = "Need to update `canopy/src/runtime.rs` before continuing";
        match classify_scope_gap(work_item, &scope) {
            Some(ScopeGap::Blocking { description }) => {
                assert!(description.contains("canopy/src/runtime.rs"));
            }
            other => panic!("expected blocking scope gap, got {other:?}"),
        }
    }

    #[test]
    fn classify_scope_gap_marks_non_blocking_additive_paths() {
        let scope = vec!["canopy/src/models.rs".to_string()];
        let work_item = "Optional follow-up: draft `canopy/docs/scope-gap.md` later";
        match classify_scope_gap(work_item, &scope) {
            Some(ScopeGap::NonBlocking { description }) => {
                assert!(description.contains("canopy/docs/scope-gap.md"));
            }
            other => panic!("expected non-blocking scope gap, got {other:?}"),
        }
    }
}
