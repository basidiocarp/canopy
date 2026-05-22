use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use ulid::Ulid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DodItem {
    pub description: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SprintContract {
    pub contract_id: String,
    pub task_id: String,
    pub handoff_path: String,
    pub repo: String,
    pub scope: Vec<String>,
    pub dod: Vec<DodItem>,
    pub non_goals: Vec<String>,
    pub verification_commands: Vec<String>,
    pub created_at: String,
}

impl SprintContract {
    #[must_use]
    pub fn from_handoff(task_id: &str, handoff_path: &str, content: &str) -> Self {
        Self {
            contract_id: Ulid::new().to_string(),
            task_id: task_id.to_string(),
            handoff_path: handoff_path.to_string(),
            repo: extract_inline_field(content, "**Owning repo:**"),
            scope: extract_inline_field_list(content, "**Allowed write scope:**"),
            dod: extract_checklist(content),
            non_goals: extract_section_bullets(content, "Non-goals"),
            verification_commands: extract_code_blocks_after_verification(content),
            created_at: Utc::now().to_rfc3339(),
        }
    }

    /// Write contract to `contracts_dir/<task_id>.sprint-contract.json` atomically.
    /// Returns the path of the written file.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created, the JSON serialization fails,
    /// or the file cannot be written.
    pub fn write_to_dir(&self, contracts_dir: &Path) -> Result<PathBuf> {
        fs::create_dir_all(contracts_dir)
            .with_context(|| format!("create contracts directory {}", contracts_dir.display()))?;
        let filename = format!("{}.sprint-contract.json", self.task_id);
        let target = contracts_dir.join(&filename);
        let tmp = contracts_dir.join(format!("{filename}.tmp"));
        let json = serde_json::to_string_pretty(self).context("serialize sprint contract")?;
        fs::write(&tmp, &json)
            .with_context(|| format!("write temp contract file {}", tmp.display()))?;
        fs::rename(&tmp, &target)
            .with_context(|| format!("rename contract file to {}", target.display()))?;
        Ok(target)
    }
}

// --- Heuristic extraction helpers ---

/// Extract the first inline value after a bold label on its line.
/// e.g. `**Owning repo:** canopy` → `"canopy"` or `- **Owning repo:** canopy` → `"canopy"`
fn extract_inline_field(content: &str, label: &str) -> String {
    content
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            // Skip leading list markers (-, *)
            let content_part = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .unwrap_or(trimmed)
                .trim();
            if let Some(rest) = content_part.strip_prefix(label) {
                let value = rest.trim();
                if value.is_empty() {
                    None
                } else {
                    Some(value.to_owned())
                }
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Extract a comma-separated or single-value inline field.
fn extract_inline_field_list(content: &str, label: &str) -> Vec<String> {
    let raw = extract_inline_field(content, label);
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(',')
        .map(|s| s.trim().trim_matches(['`', '"', '\'']).to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Extract `- [ ]` / `- [x]` checklist items from the `DoD` section only.
///
/// Recognises `## DoD`, `## Definition of Done`, and `## Checklist` headers.
fn extract_checklist(content: &str) -> Vec<DodItem> {
    let dod_headers = ["dod", "definition of done", "checklist"];
    let mut in_section = false;
    let mut results = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("##") || trimmed.starts_with("---") {
            if in_section {
                break;
            }
            let lower = trimmed.to_lowercase();
            if dod_headers.iter().any(|h| lower.contains(h)) {
                in_section = true;
            }
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            results.push(DodItem {
                description: rest.trim().to_owned(),
                verified: false,
            });
        } else if let Some(rest) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            results.push(DodItem {
                description: rest.trim().to_owned(),
                verified: true,
            });
        }
    }
    results
}

/// Extract bullet points from a named section (stops at next `##` or `---`).
fn extract_section_bullets(content: &str, section_name: &str) -> Vec<String> {
    let mut in_section = false;
    let mut results = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("##") || trimmed.starts_with("---") {
            if in_section {
                break;
            }
            if trimmed.contains(section_name) {
                in_section = true;
            }
            continue;
        }
        if in_section {
            if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                let cleaned = item.trim_matches(['`', '"', '\'']).trim();
                if !cleaned.is_empty() {
                    results.push(cleaned.to_owned());
                }
            }
        }
    }
    results
}

/// Extract fenced code blocks that appear inside a `### Verification` section.
fn extract_code_blocks_after_verification(content: &str) -> Vec<String> {
    let mut in_verification = false;
    let mut in_code = false;
    let mut current = String::new();
    let mut results = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("###") {
            in_verification = trimmed.contains("Verification");
            continue;
        }
        if trimmed.starts_with("##") && !trimmed.starts_with("###") {
            in_verification = false;
        }
        if !in_verification {
            continue;
        }
        if trimmed.starts_with("```") {
            if in_code {
                let cmd = current.trim().to_owned();
                if !cmd.is_empty() {
                    results.push(cmd);
                }
                current = String::new();
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r"
# canopy: Some Handoff

## Handoff Metadata

- **Owning repo:** canopy
- **Allowed write scope:** `canopy/src/contract.rs`, `canopy/src/cli.rs`

## What needs doing

Some task description.

## Non-goals

- No auto-verification in Phase 1
- No septa schema yet

### Step 2: Verification

```bash
(cd canopy && cargo build 2>&1 | tail -5)
(cd canopy && cargo test sprint_contract 2>&1 | tail -15)
```

## DoD

- [ ] SprintContract serializes to valid JSON
- [ ] Checklist items extracted correctly
- [x] Already done item
";

    #[test]
    fn extracts_repo() {
        let c = SprintContract::from_handoff("task-1", "path/to/handoff.md", SAMPLE);
        assert_eq!(c.repo, "canopy");
    }

    #[test]
    fn extracts_scope() {
        let c = SprintContract::from_handoff("task-1", "path/to/handoff.md", SAMPLE);
        assert_eq!(c.scope, vec!["canopy/src/contract.rs", "canopy/src/cli.rs"]);
    }

    #[test]
    fn extracts_non_goals() {
        let c = SprintContract::from_handoff("task-1", "path/to/handoff.md", SAMPLE);
        assert_eq!(c.non_goals.len(), 2);
        assert!(c.non_goals[0].contains("auto-verification"));
    }

    #[test]
    fn extracts_dod_checklist() {
        let c = SprintContract::from_handoff("task-1", "path/to/handoff.md", SAMPLE);
        assert_eq!(c.dod.len(), 3);
        assert!(!c.dod[0].verified);
        assert!(c.dod[2].verified);
    }

    #[test]
    fn extracts_verification_commands() {
        let c = SprintContract::from_handoff("task-1", "path/to/handoff.md", SAMPLE);
        assert_eq!(c.verification_commands.len(), 1);
        assert!(c.verification_commands[0].contains("cargo build"));
    }

    #[test]
    fn writes_to_dir_and_reads_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let c = SprintContract::from_handoff("task-abc", "some/handoff.md", SAMPLE);
        let path = c.write_to_dir(dir.path()).expect("write");
        assert!(path.exists());
        let json = std::fs::read_to_string(&path).expect("read");
        let parsed: SprintContract = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed.task_id, "task-abc");
    }
}
