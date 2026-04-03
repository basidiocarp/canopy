use glob::Pattern;

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
}
