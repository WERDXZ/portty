mod config;
mod portal;
mod server;
mod session;

use config::Config;
use futures_lite::future;
use server::Server;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("pttd=info".parse()?))
        .init();

    future::block_on(async {
        info!("Starting xdg-desktop-portal-tty...");

        let config = Config::load();
        info!(?config, "Config loaded");

        Server::new(config).run().await?;

        Ok(())
    })
}
