use anyhow::{Context, Result};
use serde::Serialize;
use spore::logging::{SpanContext, subprocess_span, tool_span, workflow_span};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tracing::warn;

/// Report on whether a handoff document meets completion criteria.
#[derive(Debug, Clone, Serialize)]
pub struct CompletenessReport {
    pub is_complete: bool,
    pub total_checkboxes: usize,
    pub checked_checkboxes: usize,
    pub empty_paste_markers: Vec<usize>,
    pub has_verify_script: bool,
    pub verify_script_path: Option<PathBuf>,
    /// Set when a `## Residual Work` section is present but contains only the
    /// template placeholder row — meaning no findings have been logged.
    /// This is a warning, not a hard block: an empty section is valid when
    /// every Stage 1 and Stage 2 finding was fixed.
    pub residual_work_warning: Option<String>,
}

/// Result of running a verification script.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub success: bool,
    pub passed: usize,
    pub failed: usize,
    pub output: String,
    pub timed_out: bool,
}

/// Parse a handoff markdown document and determine whether it meets completion
/// criteria: all checkboxes checked, all paste markers filled, and a paired
/// verification script exists.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn check_completeness(handoff_path: &Path) -> Result<CompletenessReport> {
    let span_context = span_context_for_handoff(handoff_path);
    let _tool_span = tool_span("handoff_check_completeness", &span_context).entered();
    let _workflow_span = workflow_span("handoff_check_completeness", &span_context).entered();
    let content =
        std::fs::read_to_string(handoff_path).context("failed to read handoff document")?;

    let (total_checkboxes, checked_checkboxes) = count_checkboxes(&content);
    let empty_paste_markers = find_empty_paste_markers(&content);
    let residual_work_warning = check_residual_work_section(&content);

    let verify_script = derive_verify_script_path(handoff_path);
    let has_verify_script = verify_script.exists();
    let verify_script_path = has_verify_script.then_some(verify_script);

    let is_complete = total_checkboxes > 0
        && total_checkboxes == checked_checkboxes
        && empty_paste_markers.is_empty();

    if is_complete {
        warn!(
            path = %handoff_path.display(),
            total_checkboxes,
            checked_checkboxes,
            has_verify_script,
            residual_work_warning = residual_work_warning.is_some(),
            "handoff completeness check passed"
        );
    } else {
        warn!(
            path = %handoff_path.display(),
            total_checkboxes,
            checked_checkboxes,
            empty_paste_markers = empty_paste_markers.len(),
            has_verify_script,
            "handoff completeness check found outstanding work"
        );
    }

    Ok(CompletenessReport {
        is_complete,
        total_checkboxes,
        checked_checkboxes,
        empty_paste_markers,
        has_verify_script,
        verify_script_path,
        residual_work_warning,
    })
}

/// Format a human-readable report of what remains incomplete.
#[must_use]
pub fn format_incomplete_report(report: &CompletenessReport) -> String {
    let mut parts = Vec::new();

    let unchecked = report.total_checkboxes - report.checked_checkboxes;
    if unchecked > 0 {
        parts.push(format!(
            "{unchecked} of {} checklist items remain unchecked",
            report.total_checkboxes
        ));
    }

    if !report.empty_paste_markers.is_empty() {
        let markers: Vec<String> = report
            .empty_paste_markers
            .iter()
            .map(|line| format!("line {line}"))
            .collect();
        parts.push(format!(
            "{} paste marker(s) have no content: {}",
            report.empty_paste_markers.len(),
            markers.join(", ")
        ));
    }

    if parts.is_empty() {
        return "handoff appears complete".to_string();
    }

    format!("Handoff incomplete: {}", parts.join("; "))
}

const VERIFY_SCRIPT_TIMEOUT: Duration = Duration::from_secs(30);

/// Execute the paired verification script and parse its results.
///
/// If the report has no verify script path, returns a warning result without
/// blocking. Enforces a 30-second timeout.
///
/// # Errors
///
/// Returns an error if the script cannot be executed.
pub fn run_verify_script(report: &CompletenessReport) -> Result<VerifyResult> {
    let Some(script_path) = &report.verify_script_path else {
        warn!("no verify script found; skipping verification");
        return Ok(VerifyResult {
            success: true,
            passed: 0,
            failed: 0,
            output: "no verify script found; skipping".to_string(),
            timed_out: false,
        });
    };

    let span_context = span_context_for_script(script_path);
    let _tool_span = tool_span("handoff_verify_script", &span_context).entered();

    let child = {
        let _subprocess_span = subprocess_span("verify-script", &span_context).entered();
        verify_script_command(script_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("failed to spawn verify script")?
    };

    let output = wait_with_timeout(child, VERIFY_SCRIPT_TIMEOUT, &span_context, script_path)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    let (passed, failed) = parse_results_line(&combined);
    let success = output.status.success() && failed == 0;

    Ok(VerifyResult {
        success,
        passed,
        failed,
        output: combined,
        timed_out: false,
    })
}

fn verify_script_command(script_path: &Path) -> Command {
    #[cfg(windows)]
    {
        if matches!(
            script_path.extension().and_then(|ext| ext.to_str()),
            Some("cmd" | "bat")
        ) {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(script_path);
            return command;
        }
    }

    let mut command = Command::new("bash");
    command.arg(script_path);
    command
}

/// Wait for a child process with a timeout, killing it if exceeded.
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
    span_context: &SpanContext,
    script_path: &Path,
) -> Result<std::process::Output> {
    use std::io::Read;

    let _workflow_span = workflow_span("verify_script_wait", span_context).entered();
    let start = std::time::Instant::now();
    let mut next_progress_log = Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    out.read_to_end(&mut stdout)?;
                }
                let mut stderr = Vec::new();
                if let Some(mut err) = child.stderr.take() {
                    err.read_to_end(&mut stderr)?;
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                let elapsed = start.elapsed();
                if elapsed >= next_progress_log {
                    warn!(
                        script = %script_path.display(),
                        elapsed_secs = elapsed.as_secs(),
                        timeout_secs = timeout.as_secs(),
                        "waiting for verify script to finish"
                    );
                    next_progress_log += Duration::from_secs(5);
                }
                if elapsed > timeout {
                    let _ = child.kill();
                    warn!(
                        script = %script_path.display(),
                        elapsed_secs = elapsed.as_secs(),
                        timeout_secs = timeout.as_secs(),
                        "verify script timed out"
                    );
                    anyhow::bail!("verify script timed out after {}s", timeout.as_secs());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn span_context_for_handoff(handoff_path: &Path) -> SpanContext {
    let context = SpanContext::for_app("canopy").with_tool("handoff_check_completeness");
    match workspace_root_from_handoff_path(handoff_path) {
        Some(workspace_root) => context.with_workspace_root(workspace_root.display().to_string()),
        None => context,
    }
}

fn span_context_for_script(script_path: &Path) -> SpanContext {
    let context = SpanContext::for_app("canopy").with_tool("handoff_verify_script");
    match workspace_root_from_handoff_path(script_path) {
        Some(workspace_root) => context.with_workspace_root(workspace_root.display().to_string()),
        None => context,
    }
}

fn workspace_root_from_handoff_path(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if ancestor.file_name().and_then(|value| value.to_str()) == Some(".handoffs") {
            return ancestor.parent().map(Path::to_path_buf);
        }
    }

    path.parent().map(Path::to_path_buf)
}

/// Check whether the `## Residual Work` section (if present) has been filled.
///
/// Returns a warning string if the section exists but contains only the template
/// placeholder row — meaning no findings have been explicitly logged or dismissed.
/// Returns `None` if the section is absent (all findings fixed) or properly filled.
fn check_residual_work_section(content: &str) -> Option<String> {
    let section_start = content.find("## Residual Work")?;
    // Slice from the section heading to the next same-level heading or end-of-doc.
    let after_heading = &content[section_start + "## Residual Work".len()..];
    let section_end = after_heading
        .find("\n## ")
        .unwrap_or(after_heading.len());
    let section = &after_heading[..section_end];

    // A "real" table row: starts with `|`, has 3+ pipes, is not a separator row,
    // is not the column-header row (always the first pipe row), and is not the
    // template example placeholder. We skip the column-header structurally rather
    // than by matching the word "Finding" — which would also match real findings.
    let mut header_skipped = false;
    let has_real_entry = section
        .lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter(|line| !line.contains("------"))
        .filter(|_line| {
            if !header_skipped {
                header_skipped = true;
                return false; // skip the column-header row, whatever it says
            }
            true
        })
        .filter(|line| !line.trim_start().starts_with("| _(example:"))
        .any(|line| line.chars().filter(|&c| c == '|').count() >= 3);

    if has_real_entry {
        return None;
    }

    Some(
        "Residual Work section has no logged findings. \
         If every Stage 1 and Stage 2 finding was fixed, this is fine. \
         If any finding was accepted but not fixed, add it here with a disposition \
         (follow-up handoff, filed ticket, or accepted-with-note)."
            .to_string(),
    )
}

/// Count total and checked markdown checkboxes in the content.
/// Returns (total, checked).
fn count_checkboxes(content: &str) -> (usize, usize) {
    let mut total = 0;
    let mut checked = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [ ]") {
            total += 1;
        } else if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            total += 1;
            checked += 1;
        }
    }
    (total, checked)
}

/// Find paste marker blocks that have no content between START and END.
/// Returns 1-based line numbers of the PASTE START markers.
fn find_empty_paste_markers(content: &str) -> Vec<usize> {
    let mut empty_markers = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains("<!-- PASTE START -->") {
            let start_line = i + 1; // 1-based line number of the marker
            // Find the matching PASTE END
            let mut j = i + 1;
            let mut has_content = false;
            while j < lines.len() {
                if lines[j].contains("<!-- PASTE END -->") {
                    break;
                }
                if !lines[j].trim().is_empty() {
                    has_content = true;
                }
                j += 1;
            }
            if !has_content {
                empty_markers.push(start_line);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    empty_markers
}

/// Derive the expected verify script path from a handoff document path.
///
/// Handles two formats:
/// - Directory envelope: `<slug>/handoff.md` → `<slug>/verify.sh`
/// - Flat: `<slug>.md` → `verify-<slug>.sh` in the same directory
fn derive_verify_script_path(handoff_path: &Path) -> PathBuf {
    if handoff_path
        .file_name()
        .is_some_and(|n| n == "handoff.md")
    {
        return handoff_path.with_file_name("verify.sh");
    }
    let stem = handoff_path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("handoff");
    handoff_path.with_file_name(format!("verify-{stem}.sh"))
}

/// Resolve a handoff slug or path to the canonical `.md` file.
///
/// Accepts either a flat `.md` path or a directory envelope path (a directory
/// containing `handoff.md`). Returns an error if both exist (ambiguous) or
/// neither exists.
///
/// # Errors
///
/// Returns an error if both `<input>.md` and `<input>/handoff.md` exist
/// (ambiguous) or if neither exists.
pub fn resolve_handoff_path(input: &Path) -> Result<PathBuf> {
    // If input already names an existing .md file, use it directly.
    if input.extension().is_some_and(|e| e == "md") && input.is_file() {
        return Ok(input.to_path_buf());
    }
    // Try directory envelope: <input>/handoff.md
    let dir_path = input.join("handoff.md");
    // Try flat: <input>.md — suppressed when input already carries .md to avoid
    // double-extension expansion (foo.md → foo.md.md).
    let flat_path = if input.extension().is_none() {
        let mut p = input.to_path_buf();
        p.set_extension("md");
        Some(p)
    } else {
        None
    };
    let dir_exists = dir_path.is_file();
    let flat_exists = flat_path.as_ref().is_some_and(|p| p.is_file());
    match (dir_exists, flat_exists, flat_path) {
        (true, true, Some(flat)) => anyhow::bail!(
            "Ambiguous handoff: both {} and {} exist",
            dir_path.display(),
            flat.display()
        ),
        (true, _, _) => Ok(dir_path),
        (false, true, Some(flat)) => Ok(flat),
        _ => anyhow::bail!("No handoff found at {}", input.display()),
    }
}

/// Parse "Results: N passed, M failed" from script output.
fn parse_results_line(output: &str) -> (usize, usize) {
    for line in output.lines().rev() {
        if let Some(rest) = line.strip_prefix("Results: ") {
            let parts: Vec<&str> = rest.split(',').collect();
            let passed = parts
                .first()
                .and_then(|s| s.trim().strip_suffix(" passed"))
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(0);
            let failed = parts
                .get(1)
                .and_then(|s| s.trim().strip_suffix(" failed"))
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(0);
            return (passed, failed);
        }
    }
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_handoff(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn fully_complete_handoff() {
        let dir = TempDir::new().unwrap();
        let content = r"# Test Handoff

- [x] First item done
- [x] Second item done

<!-- PASTE START -->
some output here
<!-- PASTE END -->
";
        let handoff_path = write_handoff(&dir, "test-handoff.md", content);

        // Create the verify script
        let verify_path = dir.path().join("verify-test-handoff.sh");
        fs::write(
            &verify_path,
            "#!/bin/bash\necho 'Results: 1 passed, 0 failed'",
        )
        .unwrap();

        let report = check_completeness(&handoff_path).unwrap();
        assert!(report.is_complete);
        assert_eq!(report.total_checkboxes, 2);
        assert_eq!(report.checked_checkboxes, 2);
        assert!(report.empty_paste_markers.is_empty());
        assert!(report.has_verify_script);
    }

    #[test]
    fn partially_complete_handoff() {
        let dir = TempDir::new().unwrap();
        let content = r"# Test Handoff

- [x] First item done
- [ ] Second item NOT done

<!-- PASTE START -->

<!-- PASTE END -->

<!-- PASTE START -->
filled output
<!-- PASTE END -->
";
        let handoff_path = write_handoff(&dir, "partial.md", content);

        let report = check_completeness(&handoff_path).unwrap();
        assert!(!report.is_complete);
        assert_eq!(report.total_checkboxes, 2);
        assert_eq!(report.checked_checkboxes, 1);
        assert_eq!(report.empty_paste_markers.len(), 1);
        assert!(!report.has_verify_script);
    }

    #[test]
    fn empty_no_checkboxes() {
        let dir = TempDir::new().unwrap();
        let content = "# Empty handoff\n\nNo checkboxes here.\n";
        let handoff_path = write_handoff(&dir, "empty.md", content);

        let report = check_completeness(&handoff_path).unwrap();
        assert!(!report.is_complete);
        assert_eq!(report.total_checkboxes, 0);
        assert_eq!(report.checked_checkboxes, 0);
        assert!(report.empty_paste_markers.is_empty());
    }

    #[test]
    fn format_report_unchecked_and_paste() {
        let report = CompletenessReport {
            is_complete: false,
            total_checkboxes: 5,
            checked_checkboxes: 3,
            empty_paste_markers: vec![10, 25],
            has_verify_script: false,
            verify_script_path: None,
            residual_work_warning: None,
        };
        let msg = format_incomplete_report(&report);
        assert!(msg.contains("2 of 5 checklist items remain unchecked"));
        assert!(msg.contains("2 paste marker(s) have no content"));
    }

    #[test]
    fn parse_results_line_valid() {
        let output = "=== Test ===\n  PASS: foo\nResults: 3 passed, 1 failed\n";
        let (passed, failed) = parse_results_line(output);
        assert_eq!(passed, 3);
        assert_eq!(failed, 1);
    }

    #[test]
    fn parse_results_line_missing() {
        let (passed, failed) = parse_results_line("no results here");
        assert_eq!(passed, 0);
        assert_eq!(failed, 0);
    }

    #[test]
    fn verify_script_missing_warns_but_succeeds() {
        let report = CompletenessReport {
            is_complete: true,
            total_checkboxes: 1,
            checked_checkboxes: 1,
            empty_paste_markers: vec![],
            has_verify_script: false,
            verify_script_path: None,
            residual_work_warning: None,
        };
        let result = run_verify_script(&report).unwrap();
        assert!(result.success);
        assert!(result.output.contains("no verify script"));
    }

    #[test]
    fn verify_script_execution() {
        let dir = TempDir::new().unwrap();
        #[cfg(windows)]
        let script = dir.path().join("verify.cmd");
        #[cfg(not(windows))]
        let script = dir.path().join("verify.sh");

        #[cfg(windows)]
        fs::write(&script, "@echo off\r\necho Results: 5 passed, 0 failed\r\n").unwrap();
        #[cfg(not(windows))]
        fs::write(&script, "#!/bin/bash\necho 'Results: 5 passed, 0 failed'\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let report = CompletenessReport {
            is_complete: true,
            total_checkboxes: 1,
            checked_checkboxes: 1,
            empty_paste_markers: vec![],
            has_verify_script: true,
            verify_script_path: Some(script),
            residual_work_warning: None,
        };
        let result = run_verify_script(&report).unwrap();
        assert!(result.success);
        assert_eq!(result.passed, 5);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn residual_work_absent_returns_none() {
        let content = "## Completion\n\n- **Disposition:** keep\n";
        assert!(check_residual_work_section(content).is_none());
    }

    #[test]
    fn residual_work_with_real_entry_returns_none() {
        let content = "## Residual Work\n\n\
            | Finding | Disposition | Link / Note |\n\
            |---------|-------------|-------------|\n\
            | hash gap in assign_task | Follow-up handoff | .handoffs/canopy/assign-hash.md |\n\
            \n## Completion\n";
        assert!(check_residual_work_section(content).is_none());
    }

    #[test]
    fn residual_work_placeholder_only_returns_warning() {
        let content = "## Residual Work\n\n\
            | Finding | Disposition | Link / Note |\n\
            |---------|-------------|-------------|\n\
            | _(example: unused import in foo.rs)_ | Follow-up handoff | `.handoffs/canopy/cleanup.md` |\n\
            \n## Completion\n";
        let warning = check_residual_work_section(content);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("no logged findings"));
    }

    #[test]
    fn residual_work_empty_table_returns_warning() {
        let content = "## Residual Work\n\n\
            | Finding | Disposition | Link / Note |\n\
            |---------|-------------|-------------|\n\
            \n## Completion\n";
        assert!(check_residual_work_section(content).is_some());
    }

    #[test]
    fn derive_verify_script_path_directory_envelope() {
        let path = PathBuf::from(".handoffs/canopy/my-feature/handoff.md");
        let verify = derive_verify_script_path(&path);
        assert_eq!(verify, PathBuf::from(".handoffs/canopy/my-feature/verify.sh"));
    }

    #[test]
    fn derive_verify_script_path_flat_format() {
        let path = PathBuf::from(".handoffs/canopy/my-feature.md");
        let verify = derive_verify_script_path(&path);
        assert_eq!(
            verify,
            PathBuf::from(".handoffs/canopy/verify-my-feature.sh")
        );
    }

    #[test]
    fn resolve_handoff_path_flat() {
        let dir = TempDir::new().unwrap();
        let flat = dir.path().join("my-feature.md");
        fs::write(&flat, "# Test").unwrap();
        let resolved = resolve_handoff_path(&dir.path().join("my-feature")).unwrap();
        assert_eq!(resolved, flat);
    }

    #[test]
    fn resolve_handoff_path_directory_envelope() {
        let dir = TempDir::new().unwrap();
        let envelope_dir = dir.path().join("my-feature");
        fs::create_dir(&envelope_dir).unwrap();
        let handoff_md = envelope_dir.join("handoff.md");
        fs::write(&handoff_md, "# Test").unwrap();
        let resolved = resolve_handoff_path(&envelope_dir).unwrap();
        assert_eq!(resolved, handoff_md);
    }

    #[test]
    fn resolve_handoff_path_ambiguous_returns_error() {
        let dir = TempDir::new().unwrap();
        // flat
        let flat = dir.path().join("my-feature.md");
        fs::write(&flat, "# Flat").unwrap();
        // directory envelope
        let envelope_dir = dir.path().join("my-feature");
        fs::create_dir(&envelope_dir).unwrap();
        fs::write(envelope_dir.join("handoff.md"), "# Envelope").unwrap();
        let result = resolve_handoff_path(&dir.path().join("my-feature"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Ambiguous handoff"));
    }

    #[test]
    fn resolve_handoff_path_missing_returns_error() {
        let dir = TempDir::new().unwrap();
        let result = resolve_handoff_path(&dir.path().join("nonexistent"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No handoff found"));
    }

    #[test]
    fn resolve_handoff_path_direct_md_file() {
        let dir = TempDir::new().unwrap();
        let flat = dir.path().join("my-feature.md");
        fs::write(&flat, "# Test").unwrap();
        let resolved = resolve_handoff_path(&flat).unwrap();
        assert_eq!(resolved, flat);
    }

    #[test]
    fn workspace_root_is_inferred_from_handoffs_parent() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path().join("workspace");
        let handoff_dir = project_root.join(".handoffs").join("canopy");
        fs::create_dir_all(&handoff_dir).unwrap();
        let handoff_path = handoff_dir.join("demo.md");

        let workspace_root = workspace_root_from_handoff_path(&handoff_path).unwrap();
        assert_eq!(workspace_root, project_root);
    }
}
