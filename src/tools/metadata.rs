/// Metadata annotation for tools registered in canopy's tool registry.
///
/// This struct drives dispatch gate decisions independently of the tool's name.
/// It complements the MCP schema annotations and can be converted to `ToolAnnotations`
/// for policy evaluation.
#[derive(Debug, Clone, Default)]
pub struct ToolMetadata {
    /// Tool is safe to call multiple times with the same arguments.
    /// Idempotent tools can be safely retried without side effects.
    pub idempotent: bool,
    /// Tool only reads state; never mutates coordination records.
    pub read_only: bool,
    /// Tool may delete or overwrite data irreversibly.
    pub destructive: bool,
    /// Human-readable criteria shown in approval gate prompts for sensitive operations.
    pub acceptance_criteria: Option<String>,
}

impl ToolMetadata {
    /// Create metadata for a read-only tool.
    ///
    /// # Example
    /// ```
    /// # use canopy::tools::ToolMetadata;
    /// let meta = ToolMetadata::read_only();
    /// assert!(meta.read_only);
    /// assert!(!meta.destructive);
    /// ```
    #[must_use]
    pub fn read_only() -> Self {
        ToolMetadata {
            read_only: true,
            ..Default::default()
        }
    }

    /// Create metadata for a destructive tool with acceptance criteria.
    ///
    /// # Example
    /// ```
    /// # use canopy::tools::ToolMetadata;
    /// let meta = ToolMetadata::destructive("Verify task ID and all subtasks are complete");
    /// assert!(meta.destructive);
    /// assert_eq!(meta.acceptance_criteria, Some("Verify task ID and all subtasks are complete".to_string()));
    /// ```
    #[must_use]
    pub fn destructive(criteria: impl Into<String>) -> Self {
        ToolMetadata {
            destructive: true,
            acceptance_criteria: Some(criteria.into()),
            ..Default::default()
        }
    }
}

/// Shared registry: maps `&'static str` tool names to their `ToolAnnotations`.
///
/// Populated at startup via [`register_tool_metadata`]; read by
/// [`lookup_tool_annotations`] on every dispatch. A `Mutex<Vec<_>>` is fine
/// here because the critical path is dominated by `SQLite` I/O, and the registry
/// is only written during single-threaded startup.
static TOOL_METADATA_REGISTRY: std::sync::Mutex<
    Vec<(&'static str, super::policy::ToolAnnotations)>,
> = std::sync::Mutex::new(Vec::new());

/// Look up `ToolAnnotations` for a tool name via the metadata registry.
///
/// Returns `None` when no metadata has been registered for `tool_name`.
/// `annotations_for_tool` in `policy.rs` falls back to its hardcoded entries
/// when this returns `None`, so existing behavior is fully preserved.
///
/// # Panics
///
/// Panics if the internal registry lock is poisoned (another thread panicked
/// while holding it). This cannot happen in normal operation.
#[must_use]
pub fn lookup_tool_annotations(tool_name: &str) -> Option<super::policy::ToolAnnotations> {
    let guard = TOOL_METADATA_REGISTRY
        .lock()
        .expect("tool metadata registry lock poisoned");
    guard
        .iter()
        .find(|(name, _)| *name == tool_name)
        .map(|(_, ann)| *ann)
}

/// Register `ToolMetadata` for a named tool before dispatch begins.
///
/// First registration wins; subsequent calls for the same `name` are ignored.
/// Intended for startup initialization in `main.rs` or test setup.
///
/// # Panics
///
/// Panics if the internal registry lock is poisoned (another thread panicked
/// while holding it). This cannot happen in normal operation.
pub fn register_tool_metadata(name: &'static str, metadata: ToolMetadata) {
    let mut guard = TOOL_METADATA_REGISTRY
        .lock()
        .expect("tool metadata registry lock poisoned");
    if guard.iter().any(|(n, _)| *n == name) {
        return; // first wins
    }
    guard.push((name, metadata.into()));
}

impl From<ToolMetadata> for super::policy::ToolAnnotations {
    fn from(m: ToolMetadata) -> Self {
        super::policy::ToolAnnotations {
            read_only_hint: m.read_only,
            destructive_hint: m.destructive,
            idempotent_hint: m.idempotent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_metadata() {
        let meta = ToolMetadata::read_only();
        assert!(meta.read_only);
        assert!(!meta.destructive);
        assert!(!meta.idempotent);
        assert!(meta.acceptance_criteria.is_none());
    }

    #[test]
    fn destructive_metadata() {
        let meta = ToolMetadata::destructive("Verify all evidence is archived");
        assert!(!meta.read_only);
        assert!(meta.destructive);
        assert!(!meta.idempotent);
        assert_eq!(
            meta.acceptance_criteria,
            Some("Verify all evidence is archived".to_string())
        );
    }

    #[test]
    fn default_metadata_is_inert() {
        let meta = ToolMetadata::default();
        assert!(!meta.read_only);
        assert!(!meta.destructive);
        assert!(!meta.idempotent);
        assert!(meta.acceptance_criteria.is_none());
    }

    #[test]
    fn custom_metadata() {
        let meta = ToolMetadata {
            idempotent: true,
            read_only: false,
            destructive: false,
            acceptance_criteria: None,
        };
        assert!(!meta.read_only);
        assert!(!meta.destructive);
        assert!(meta.idempotent);
    }
}
