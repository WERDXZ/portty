use futures_util::future::abortable;
use zbus::zvariant::{DeserializeDict, ObjectPath, SerializeDict, Type};

use crate::request::Request;

/// File filter: (name, patterns)
/// D-Bus signature: (sa(us))
/// Example: ("Images", [(0, "*.png"), (1, "image/png")])
/// Pattern type: 0 = glob, 1 = mime type
#[derive(Debug, Clone, Type, serde::Serialize, serde::Deserialize)]
#[zvariant(signature = "(sa(us))")]
pub struct FileFilter(String, Vec<(u32, String)>);

impl FileFilter {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into(), Vec::new())
    }

    pub fn glob(mut self, pattern: impl Into<String>) -> Self {
        self.1.push((0, pattern.into()));
        self
    }

    pub fn mime_type(mut self, mime: impl Into<String>) -> Self {
        self.1.push((1, mime.into()));
        self
    }

    pub fn name(&self) -> &str {
        &self.0
    }

    pub fn patterns(&self) -> impl Iterator<Item = FilterPattern<'_>> {
        self.1.iter().map(|(t, s)| match t {
            0 => FilterPattern::Glob(s),
            _ => FilterPattern::MimeType(s),
        })
    }
}

#[derive(Debug, Clone)]
pub enum FilterPattern<'a> {
    Glob(&'a str),
    MimeType(&'a str),
}

/// Choice option for dialogs
/// D-Bus signature: (ssa(ss)s)
/// (id, label, [(option_id, option_label), ...], default_option_id)
#[derive(Debug, Clone, Type, serde::Serialize, serde::Deserialize)]
#[zvariant(signature = "(ssa(ss)s)")]
pub struct Choice(String, String, Vec<(String, String)>, String);

impl Choice {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self(id.into(), label.into(), Vec::new(), String::new())
    }

    pub fn option(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.2.push((id.into(), label.into()));
        self
    }

    pub fn default(mut self, id: impl Into<String>) -> Self {
        self.3 = id.into();
        self
    }

    pub fn id(&self) -> &str {
        &self.0
    }

    pub fn label(&self) -> &str {
        &self.1
    }

    pub fn options(&self) -> &[(String, String)] {
        &self.2
    }

    pub fn default_option(&self) -> &str {
        &self.3
    }
}

/// Options for OpenFile request
#[derive(Debug, Clone, Default, DeserializeDict, Type)]
#[zvariant(signature = "dict")]
pub struct OpenFileOptions {
    accept_label: Option<String>,
    modal: Option<bool>,
    multiple: Option<bool>,
    directory: Option<bool>,
    filters: Option<Vec<FileFilter>>,
    current_filter: Option<FileFilter>,
    choices: Option<Vec<Choice>>,
    current_folder: Option<Vec<u8>>,
}

impl OpenFileOptions {
    pub fn accept_label(&self) -> Option<&str> {
        self.accept_label.as_deref()
    }

    pub fn modal(&self) -> Option<bool> {
        self.modal
    }

    pub fn multiple(&self) -> Option<bool> {
        self.multiple
    }

    pub fn directory(&self) -> Option<bool> {
        self.directory
    }

    pub fn filters(&self) -> &[FileFilter] {
        self.filters.as_deref().unwrap_or_default()
    }

    pub fn current_filter(&self) -> Option<&FileFilter> {
        self.current_filter.as_ref()
    }

    pub fn choices(&self) -> &[Choice] {
        self.choices.as_deref().unwrap_or_default()
    }

    pub fn current_folder(&self) -> Option<&[u8]> {
        self.current_folder.as_deref()
    }
}

/// Options for SaveFile request
#[derive(Debug, Clone, Default, DeserializeDict, Type)]
#[zvariant(signature = "dict")]
pub struct SaveFileOptions {
    accept_label: Option<String>,
    modal: Option<bool>,
    filters: Option<Vec<FileFilter>>,
    current_filter: Option<FileFilter>,
    choices: Option<Vec<Choice>>,
    current_name: Option<String>,
    current_folder: Option<Vec<u8>>,
    current_file: Option<Vec<u8>>,
}

impl SaveFileOptions {
    pub fn accept_label(&self) -> Option<&str> {
        self.accept_label.as_deref()
    }

    pub fn modal(&self) -> Option<bool> {
        self.modal
    }

    pub fn filters(&self) -> &[FileFilter] {
        self.filters.as_deref().unwrap_or_default()
    }

    pub fn current_filter(&self) -> Option<&FileFilter> {
        self.current_filter.as_ref()
    }

    pub fn choices(&self) -> &[Choice] {
        self.choices.as_deref().unwrap_or_default()
    }

    pub fn current_name(&self) -> Option<&str> {
        self.current_name.as_deref()
    }

    pub fn current_folder(&self) -> Option<&[u8]> {
        self.current_folder.as_deref()
    }

    pub fn current_file(&self) -> Option<&[u8]> {
        self.current_file.as_deref()
    }
}

/// Options for SaveFiles request
#[derive(Debug, Clone, Default, DeserializeDict, Type)]
#[zvariant(signature = "dict")]
pub struct SaveFilesOptions {
    accept_label: Option<String>,
    modal: Option<bool>,
    choices: Option<Vec<Choice>>,
    current_folder: Option<Vec<u8>>,
    files: Option<Vec<Vec<u8>>>,
}

impl SaveFilesOptions {
    pub fn accept_label(&self) -> Option<&str> {
        self.accept_label.as_deref()
    }

    pub fn modal(&self) -> Option<bool> {
        self.modal
    }

    pub fn choices(&self) -> &[Choice] {
        self.choices.as_deref().unwrap_or_default()
    }

    pub fn current_folder(&self) -> Option<&[u8]> {
        self.current_folder.as_deref()
    }

    pub fn files(&self) -> &[Vec<u8>] {
        self.files.as_deref().unwrap_or_default()
    }
}

/// Response codes from portal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ResponseCode {
    Success = 0,
    Cancelled = 1,
    Other = 2,
}

impl From<u32> for ResponseCode {
    fn from(v: u32) -> Self {
        match v {
            0 => ResponseCode::Success,
            1 => ResponseCode::Cancelled,
            _ => ResponseCode::Other,
        }
    }
}

impl From<ResponseCode> for u32 {
    fn from(v: ResponseCode) -> Self {
        v as u32
    }
}

/// Result from file chooser operations
#[derive(Debug, Clone, Default, SerializeDict, Type)]
#[zvariant(signature = "dict")]
pub struct FileChooserResult {
    uris: Vec<String>,
    choices: Option<Vec<(String, String)>>,
    current_filter: Option<FileFilter>,
    writable: Option<bool>,
}

impl FileChooserResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uris.push(uri.into());
        self
    }

    pub fn uris(mut self, uris: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.uris.extend(uris.into_iter().map(Into::into));
        self
    }

    pub fn choice(mut self, id: impl Into<String>, value: impl Into<String>) -> Self {
        self.choices
            .get_or_insert_with(Vec::new)
            .push((id.into(), value.into()));
        self
    }

    pub fn current_filter(mut self, filter: FileFilter) -> Self {
        self.current_filter = Some(filter);
        self
    }

    pub fn writable(mut self, writable: bool) -> Self {
        self.writable = Some(writable);
        self
    }
}

/// Handler trait for FileChooser operations
///
/// Implement this trait to provide the actual file choosing logic.
/// The types crate handles D-Bus serialization, you handle the UI/interaction.
pub trait FileChooserHandler: Send + Sync + 'static {
    /// Handle an OpenFile request
    fn open_file(
        &self,
        handle: String,
        app_id: String,
        parent_window: String,
        title: String,
        options: OpenFileOptions,
    ) -> impl std::future::Future<Output = Result<FileChooserResult, FileChooserError>> + Send;

    /// Handle a SaveFile request
    fn save_file(
        &self,
        handle: String,
        app_id: String,
        parent_window: String,
        title: String,
        options: SaveFileOptions,
    ) -> impl std::future::Future<Output = Result<FileChooserResult, FileChooserError>> + Send;

    /// Handle a SaveFiles request (save multiple files to a directory)
    fn save_files(
        &self,
        handle: String,
        app_id: String,
        parent_window: String,
        title: String,
        options: SaveFilesOptions,
    ) -> impl std::future::Future<Output = Result<FileChooserResult, FileChooserError>> + Send;
}

/// Error type for FileChooser operations
#[derive(Debug, Clone)]
pub enum FileChooserError {
    Cancelled,
    Other(String),
}

/// The FileChooser portal implementation wrapper
///
/// Wraps a handler and exposes it as a D-Bus interface.
pub struct FileChooserPortal<H> {
    handler: H,
}

impl<H> FileChooserPortal<H> {
    pub fn new(handler: H) -> Self {
        Self { handler }
    }
}

impl<H: FileChooserHandler> From<H> for FileChooserPortal<H> {
    fn from(handler: H) -> Self {
        Self::new(handler)
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl<H: FileChooserHandler> FileChooserPortal<H> {
    async fn open_file(
        &self,
        #[zbus(object_server)] server: &zbus::ObjectServer,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        title: &str,
        options: OpenFileOptions,
    ) -> zbus::fdo::Result<(u32, FileChooserResult)> {
        // Create abortable future
        let fut = self.handler.open_file(
            handle.to_string(),
            app_id.to_string(),
            parent_window.to_string(),
            title.to_string(),
            options,
        );
        let (abortable_fut, abort_handle) = abortable(fut);

        // Register Request object for cancellation
        let request = Request::new(abort_handle);
        server.at(handle.as_ref(), request).await?;

        // Run the handler
        let result = abortable_fut.await;

        // Unregister Request object
        let _ = server.remove::<Request, _>(handle.as_ref()).await;

        // Handle result
        match result {
            Ok(Ok(result)) => Ok((ResponseCode::Success.into(), result)),
            Ok(Err(FileChooserError::Cancelled)) | Err(_) => {
                Ok((ResponseCode::Cancelled.into(), FileChooserResult::new()))
            }
            Ok(Err(FileChooserError::Other(msg))) => Err(zbus::fdo::Error::Failed(msg)),
        }
    }

    async fn save_file(
        &self,
        #[zbus(object_server)] server: &zbus::ObjectServer,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        title: &str,
        options: SaveFileOptions,
    ) -> zbus::fdo::Result<(u32, FileChooserResult)> {
        let fut = self.handler.save_file(
            handle.to_string(),
            app_id.to_string(),
            parent_window.to_string(),
            title.to_string(),
            options,
        );
        let (abortable_fut, abort_handle) = abortable(fut);

        let request = Request::new(abort_handle);
        server.at(handle.as_ref(), request).await?;

        let result = abortable_fut.await;
        let _ = server.remove::<Request, _>(handle.as_ref()).await;

        match result {
            Ok(Ok(result)) => Ok((ResponseCode::Success.into(), result)),
            Ok(Err(FileChooserError::Cancelled)) | Err(_) => {
                Ok((ResponseCode::Cancelled.into(), FileChooserResult::new()))
            }
            Ok(Err(FileChooserError::Other(msg))) => Err(zbus::fdo::Error::Failed(msg)),
        }
    }

    async fn save_files(
        &self,
        #[zbus(object_server)] server: &zbus::ObjectServer,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        title: &str,
        options: SaveFilesOptions,
    ) -> zbus::fdo::Result<(u32, FileChooserResult)> {
        let fut = self.handler.save_files(
            handle.to_string(),
            app_id.to_string(),
            parent_window.to_string(),
            title.to_string(),
            options,
        );
        let (abortable_fut, abort_handle) = abortable(fut);

        let request = Request::new(abort_handle);
        server.at(handle.as_ref(), request).await?;

        let result = abortable_fut.await;
        let _ = server.remove::<Request, _>(handle.as_ref()).await;

        match result {
            Ok(Ok(result)) => Ok((ResponseCode::Success.into(), result)),
            Ok(Err(FileChooserError::Cancelled)) | Err(_) => {
                Ok((ResponseCode::Cancelled.into(), FileChooserResult::new()))
            }
            Ok(Err(FileChooserError::Other(msg))) => Err(zbus::fdo::Error::Failed(msg)),
        }
    }
}
