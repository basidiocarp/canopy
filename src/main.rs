use anyhow::Result;

mod app;
mod db;

fn main() -> Result<()> {
    app::run()
}
