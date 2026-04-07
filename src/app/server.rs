use anyhow::Result;
use canopy::mcp;
use canopy::store::Store;

pub(super) fn run(
    store: &Store,
    agent_id: &str,
    project: Option<&str>,
    worktree: &str,
) -> Result<()> {
    mcp::server::run_server(store, agent_id, project, Some(worktree))
}
