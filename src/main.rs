use anyhow::Result;

mod app;
mod db;

#[cfg(unix)]
mod socket_server;

fn main() -> Result<()> {
    app::run()
}
