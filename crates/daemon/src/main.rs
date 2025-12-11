#![feature(linux_pidfd)]
#![feature(unix_mkfifo)]

mod config;
mod daemon_socket;
mod dbus;
mod portal;
mod server;
mod session;

use config::Config;
use futures_lite::future;
use server::Daemon;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("porttyd=info".parse()?))
        .init();

    future::block_on(async {
        info!("Starting xdg-desktop-portal-tty...");

        let config = Config::load();
        info!(?config, "Config loaded");

        Daemon::new(config).run().await?;

        Ok(())
    })
}
