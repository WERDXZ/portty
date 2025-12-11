use std::sync::{Arc, RwLock};

use tracing::{info, warn};
use zbus::connection::Builder;

use crate::config::Config;
use crate::daemon_socket::{DaemonSocket, DaemonState};
use crate::portal::{TtyFileChooser, TtyScreenshot};
use portty_ipc::portal::file_chooser::FileChooserPortal;
use portty_ipc::portal::screenshot::ScreenshotPortal;

const SERVICE_NAME: &str = "org.freedesktop.impl.portal.desktop.tty";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

pub struct Server {
    config: Arc<Config>,
    state: Arc<RwLock<DaemonState>>,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            state: Arc::new(RwLock::new(DaemonState::new())),
        }
    }

    pub async fn run(self) -> Result<(), zbus::Error> {
        // Start daemon socket in background thread
        match DaemonSocket::new(Arc::clone(&self.state)) {
            Ok(daemon_socket) => {
                daemon_socket.spawn();
                info!("Daemon socket started");
            }
            Err(e) => {
                warn!("Failed to create daemon socket: {e}");
            }
        }

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
        let file_chooser = TtyFileChooser::new(
            Arc::clone(&self.config),
            Arc::clone(&self.state),
        );
        let builder = builder
            .serve_at(OBJECT_PATH, FileChooserPortal::from(file_chooser))?;

        info!("Registering Screenshot portal");
        let screenshot = TtyScreenshot::new(
            Arc::clone(&self.config),
            Arc::clone(&self.state),
        );
        let builder = builder
            .serve_at(OBJECT_PATH, ScreenshotPortal::from(screenshot))?;

        Ok(builder)
    }
}
