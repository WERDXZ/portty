use std::sync::{Arc, RwLock};

use tracing::{info, warn};
use zbus::connection::Builder;

use crate::config::Config;
use crate::daemon_socket::{DaemonCtl, DaemonSocket, DaemonState};
use crate::dbus::file_chooser::FileChooserPortal;
use crate::dbus::screenshot::ScreenshotPortal;
use crate::portal::{TtyFileChooser, TtyScreenshot};

const SERVICE_NAME: &str = "org.freedesktop.impl.portal.desktop.tty";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

pub struct Daemon {
    config: Arc<Config>,
    state: Arc<RwLock<DaemonState>>,
}

impl Daemon {
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

        // Start daemon FIFO in background thread
        match DaemonCtl::new(Arc::clone(&self.state)) {
            Ok(daemon_ctl) => {
                daemon_ctl.spawn();
                info!("Daemon FIFO started");
            }
            Err(e) => {
                warn!("Failed to create daemon FIFO: {e}");
            }
        }

        let builder = Builder::session()?.name(SERVICE_NAME)?;

        // Register portals
        let builder = self.register_portals(builder)?;

        let _connection = builder.build().await?;

        info!(
            service = SERVICE_NAME,
            path = OBJECT_PATH,
            "Registered on D-Bus session bus"
        );
        info!("Waiting for requests...");

        // Keep running
        std::future::pending::<()>().await;

        Ok(())
    }

    fn register_portals(&self, builder: Builder<'static>) -> Result<Builder<'static>, zbus::Error> {
        info!("Registering FileChooser portal");
        let file_chooser = TtyFileChooser::new(Arc::clone(&self.config), Arc::clone(&self.state));
        let builder = builder.serve_at(OBJECT_PATH, FileChooserPortal::from(file_chooser))?;

        info!("Registering Screenshot portal");
        let screenshot = TtyScreenshot::new(Arc::clone(&self.config), Arc::clone(&self.state));
        let builder = builder.serve_at(OBJECT_PATH, ScreenshotPortal::from(screenshot))?;

        Ok(builder)
    }
}
