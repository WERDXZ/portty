use std::sync::Arc;

use tracing::info;
use zbus::connection::Builder;

use crate::config::Config;
use crate::portal::TtyFileChooser;
use portty_types::portal::file_chooser::FileChooserPortal;

const SERVICE_NAME: &str = "org.freedesktop.impl.portal.desktop.tty";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

pub struct Server {
    config: Arc<Config>,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub async fn run(self) -> Result<(), zbus::Error> {
        let builder = Builder::session()?
            .name(SERVICE_NAME)?;

        // Register portals
        let builder = self.register_portals(builder)?;

        let _connection = builder.build().await?;

        info!(service = SERVICE_NAME, path = OBJECT_PATH, "Registered on D-Bus session bus");
        info!("Waiting for requests...");

        // Keep running
        std::future::pending::<()>().await;

        Ok(())
    }

    fn register_portals(&self, builder: Builder<'static>) -> Result<Builder<'static>, zbus::Error> {
        info!("Registering FileChooser portal");
        let builder = builder
            .serve_at(OBJECT_PATH, FileChooserPortal::from(TtyFileChooser::new(self.config.clone())))?;

        // Add more portals here:
        // let builder = builder.serve_at(OBJECT_PATH, ScreenshotPortal::from(...))?;
        // let builder = builder.serve_at(OBJECT_PATH, NotificationPortal::from(...))?;

        Ok(builder)
    }
}
