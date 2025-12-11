use futures_util::future::abortable;
use zbus::zvariant::{DeserializeDict, ObjectPath, SerializeDict, Type};

use crate::dbus::request::Request;

/// Options for Screenshot request
#[derive(Debug, Clone, Default, DeserializeDict, Type)]
#[zvariant(signature = "dict")]
#[allow(unused)]
pub struct ScreenshotOptions {
    modal: Option<bool>,
    interactive: Option<bool>,
    permission_store_checked: Option<bool>,
}

impl ScreenshotOptions {
    pub fn modal(&self) -> Option<bool> {
        self.modal
    }

    pub fn interactive(&self) -> Option<bool> {
        self.interactive
    }
}

/// Options for PickColor request
#[derive(Debug, Clone, Default, DeserializeDict, Type)]
#[zvariant(signature = "dict")]
pub struct PickColorOptions {}

/// Response codes from portal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ResponseCode {
    Success = 0,
    Cancelled = 1,
    Other = 2,
}

impl From<ResponseCode> for u32 {
    fn from(v: ResponseCode) -> Self {
        v as u32
    }
}

/// Result from screenshot operation
#[derive(Debug, Clone, Default, SerializeDict, Type)]
#[zvariant(signature = "dict")]
pub struct ScreenshotResult {
    uri: String,
}

impl ScreenshotResult {
    pub fn new(uri: impl Into<String>) -> Self {
        Self { uri: uri.into() }
    }
}

/// Result from pick color operation
#[derive(Debug, Clone, Default, SerializeDict, Type)]
#[zvariant(signature = "dict")]
pub struct PickColorResult {
    color: (f64, f64, f64),
}

impl PickColorResult {
    pub fn new(color: (f64, f64, f64)) -> Self {
        Self { color }
    }
}

/// Error type for Screenshot operations
#[derive(Debug, Clone)]
pub enum ScreenshotError {
    Cancelled,
    Other(String),
}

/// Handler trait for Screenshot operations
pub trait ScreenshotHandler: Send + Sync + 'static {
    /// Handle a Screenshot request
    fn screenshot(
        &self,
        handle: String,
        app_id: String,
        parent_window: String,
        options: ScreenshotOptions,
    ) -> impl std::future::Future<Output = Result<ScreenshotResult, ScreenshotError>> + Send;

    /// Handle a PickColor request
    fn pick_color(
        &self,
        handle: String,
        app_id: String,
        parent_window: String,
        options: PickColorOptions,
    ) -> impl std::future::Future<Output = Result<PickColorResult, ScreenshotError>> + Send;
}

/// The Screenshot portal implementation wrapper
pub struct ScreenshotPortal<H> {
    handler: H,
}

impl<H> ScreenshotPortal<H> {
    pub fn new(handler: H) -> Self {
        Self { handler }
    }
}

impl<H: ScreenshotHandler> From<H> for ScreenshotPortal<H> {
    fn from(handler: H) -> Self {
        Self::new(handler)
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl<H: ScreenshotHandler> ScreenshotPortal<H> {
    async fn screenshot(
        &self,
        #[zbus(object_server)] server: &zbus::ObjectServer,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: ScreenshotOptions,
    ) -> zbus::fdo::Result<(u32, ScreenshotResult)> {
        let fut = self.handler.screenshot(
            handle.to_string(),
            app_id.to_string(),
            parent_window.to_string(),
            options,
        );
        let (abortable_fut, abort_handle) = abortable(fut);

        let request = Request::new(abort_handle);
        server.at(handle.as_ref(), request).await?;

        let result = abortable_fut.await;
        let _ = server.remove::<Request, _>(handle.as_ref()).await;

        match result {
            Ok(Ok(result)) => Ok((ResponseCode::Success.into(), result)),
            Ok(Err(ScreenshotError::Cancelled)) | Err(_) => {
                Ok((ResponseCode::Cancelled.into(), ScreenshotResult::default()))
            }
            Ok(Err(ScreenshotError::Other(msg))) => Err(zbus::fdo::Error::Failed(msg)),
        }
    }

    async fn pick_color(
        &self,
        #[zbus(object_server)] server: &zbus::ObjectServer,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: PickColorOptions,
    ) -> zbus::fdo::Result<(u32, PickColorResult)> {
        let fut = self.handler.pick_color(
            handle.to_string(),
            app_id.to_string(),
            parent_window.to_string(),
            options,
        );
        let (abortable_fut, abort_handle) = abortable(fut);

        let request = Request::new(abort_handle);
        server.at(handle.as_ref(), request).await?;

        let result = abortable_fut.await;
        let _ = server.remove::<Request, _>(handle.as_ref()).await;

        match result {
            Ok(Ok(result)) => Ok((ResponseCode::Success.into(), result)),
            Ok(Err(ScreenshotError::Cancelled)) | Err(_) => {
                Ok((ResponseCode::Cancelled.into(), PickColorResult::default()))
            }
            Ok(Err(ScreenshotError::Other(msg))) => Err(zbus::fdo::Error::Failed(msg)),
        }
    }
}
