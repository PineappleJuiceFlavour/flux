use std::sync::Arc;

use schema::curseforge::{CurseforgeFile, CurseforgeProject};

use crate::notify_signal::{KeepAliveNotifySignal, KeepAliveNotifySignalHandle};

#[derive(Clone, Debug)]
pub struct ManualCurseforgeDownload {
    pub project_id: u32,
    pub file_id: u32,
    pub name: Arc<str>,
    pub filename: Arc<str>,
    pub sha1: [u8; 20],
    pub size: u64,
    pub page_url: Arc<str>,
}

impl ManualCurseforgeDownload {
    pub fn new(file: &CurseforgeFile, project: &CurseforgeProject, hash: [u8; 20]) -> Self {
        let mut page_url = project.links.website_url.to_string();
        if !page_url.ends_with('/') {
            page_url.push('/');
        }

        use std::fmt::Write;
        write!(&mut page_url, "download/{}", file.id).unwrap();

        Self {
            project_id: file.mod_id,
            file_id: file.id,
            name: project.name.clone(),
            filename: file.file_name.clone(),
            sha1: hash,
            size: file.file_length,
            page_url: page_url.into(),
        }
    }
}

#[derive(Debug)]
pub struct ManualCurseforgeDownloadRequest {
    pub initial_files: Arc<[ManualCurseforgeDownload]>,
    pub download_dir_send: tokio::sync::oneshot::Sender<Arc<std::path::Path>>,
    pub add_files_recv: tokio::sync::mpsc::UnboundedReceiver<Vec<ManualCurseforgeDownload>>,
    pub finished_recv: tokio::sync::mpsc::UnboundedReceiver<[u8; 20]>,
    pub frontend_alive: KeepAliveNotifySignal,
    pub backend_alive: KeepAliveNotifySignalHandle,
}
