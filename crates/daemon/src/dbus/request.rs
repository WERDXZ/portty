use futures_util::future::AbortHandle;

/// A portal Request object
///
/// Each portal request creates a Request object at a unique path.
/// This allows the caller to cancel the request before it completes
/// by calling the Close method.
pub struct Request {
    abort_handle: AbortHandle,
}

impl Request {
    pub fn new(abort_handle: AbortHandle) -> Self {
        Self { abort_handle }
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl Request {
    /// Close the request
    ///
    /// Called by the application to cancel an in-progress request.
    async fn close(&self) {
        self.abort_handle.abort();
    }
}
