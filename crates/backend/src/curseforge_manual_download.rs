use std::{path::Path, sync::{Arc, atomic::{AtomicUsize, Ordering}}};

use bridge::{manual_download::{ManualCurseforgeDownload, ManualCurseforgeDownloadRequest}, message::MessageToFrontend, notify_signal::{KeepAliveNotifySignal, KeepAliveNotifySignalHandle}};
use parking_lot::RwLock;
use rustc_hash::{FxHashMap, FxHashSet};
use sha1::Sha1;
use sha2::Digest;
use tokio::sync::Notify;

use crate::{BackendState, BackendStateFileWatching, WatchTarget};

static DOWNLOAD_SESSION_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Default)]
pub struct ManualCurseforgeDownloadSession(Arc<RwLock<Option<ManualCurseforgeDownloadSessionInner>>>);

pub struct ManualCurseforgeDownloadSessionInner {
    id: usize,
    file_lengths: FxHashSet<u64>,
    sent_hashes: FxHashSet<[u8; 20]>,
    remaining_files_by_hash: FxHashMap<[u8; 20], ManualCurseforgeDownload>,
    stopped_notify: Arc<Notify>,
    add_files_send: tokio::sync::mpsc::UnboundedSender<Vec<ManualCurseforgeDownload>>,
    finished_send: tokio::sync::mpsc::UnboundedSender<[u8; 20]>,
    watching_directory: Option<Arc<Path>>,
    content_library_dir: Arc<Path>,
    frontend_alive: KeepAliveNotifySignalHandle,
    file_watching: Arc<RwLock<BackendStateFileWatching>>,
    _backend_alive: KeepAliveNotifySignal,
}

#[derive(PartialEq, Eq)]
pub enum SessionState {
    StillActive,
    Inactive
}

impl ManualCurseforgeDownloadSession {
    pub fn process_candidate(&self, source: &Path, metadata: std::fs::Metadata, id: usize, remove: bool) -> SessionState {
        if !metadata.is_file() {
            return SessionState::StillActive;
        }
        if let Some(source_extension) = source.extension() {
            if matches!(source_extension.as_encoded_bytes(), b"crdownload" | b"part" | b"tmp" | b"new") {
                return SessionState::StillActive;
            }
        }

        // Acquire lock temporarily to check file_lengths
        if let Some(inner) = &*self.0.read() {
            if inner.id != id {
                return SessionState::Inactive;
            }

            if !inner.file_lengths.contains(&metadata.len()) {
                return SessionState::StillActive;
            }
        } else {
            return SessionState::Inactive;
        }

        // Read and hash file (without lock)
        let Ok(bytes) = std::fs::read(&source) else {
            return SessionState::StillActive;
        };

        let mut hasher = Sha1::new();
        hasher.update(&bytes);
        let hash: [u8; 20] = hasher.finalize().try_into().expect("expected sha1 hash to be 20 bytes");

        // Acquire lock for the rest of the function
        let mut lock = self.0.write();
        let Some(inner) = &mut *lock else {
            return SessionState::Inactive;
        };

        if inner.id != id {
            return SessionState::Inactive;
        }

        let Some(file) = inner.remaining_files_by_hash.get(&hash) else {
            return SessionState::StillActive;
        };
        let extension = Path::new(&*file.filename).extension();
        let destination = crate::fs::create_content_library_path_osstrext(&inner.content_library_dir, hash, extension);

        // Write into content library
        if destination != source {
            if crate::fs::write_safe(&destination, &bytes).is_err() {
                return SessionState::StillActive;
            } else if remove {
                _ = std::fs::remove_file(&source);
            }
        }

        // Update state
        inner.remaining_files_by_hash.remove(&hash);
        _ = inner.finished_send.send(hash);

        if inner.remaining_files_by_hash.is_empty() {
            inner.stop();
            *lock = None;

            SessionState::Inactive
        } else {
            SessionState::StillActive
        }
    }

    pub async fn start(&self, mut files: Vec<ManualCurseforgeDownload>, backend: &BackendState) {
        if files.is_empty() {
            return;
        }

        let id = DOWNLOAD_SESSION_ID.fetch_add(1, Ordering::Relaxed);

        let (download_dir_recv, frontend_alive) = {
            let mut session_write = self.0.write();
            if let Some(existing) = &mut *session_write {
                files.retain(|file| existing.sent_hashes.contains(&file.sha1));

                if files.is_empty() {
                    return;
                }

                if existing.add_files_send.send(files.clone()).is_err() {
                    return;
                }

                for file in files.iter() {
                    existing.file_lengths.insert(file.size);
                    existing.remaining_files_by_hash.insert(file.sha1, file.clone());
                }

                return;
            }

            let files: Arc<[ManualCurseforgeDownload]> = files.into();
            let (add_files_send, add_files_recv) = tokio::sync::mpsc::unbounded_channel();
            let (finished_send, finished_recv) = tokio::sync::mpsc::unbounded_channel();
            let (download_dir_send, download_dir_recv) = tokio::sync::oneshot::channel();

            let frontend_keep_alive = KeepAliveNotifySignal::new();
            let backend_keep_alive = KeepAliveNotifySignal::new();
            let frontend_alive = frontend_keep_alive.create_handle();
            let backend_alive = backend_keep_alive.create_handle();

            let mut file_lengths = FxHashSet::default();
            let mut sent_hashes = FxHashSet::default();
            let mut remaining_files_by_hash = FxHashMap::default();

            for file in files.iter() {
                file_lengths.insert(file.size);
                sent_hashes.insert(file.sha1);
                remaining_files_by_hash.insert(file.sha1, file.clone());
            }

            let session = ManualCurseforgeDownloadSessionInner {
                id,
                file_lengths,
                sent_hashes,
                remaining_files_by_hash,
                stopped_notify: Arc::new(Notify::default()),
                add_files_send,
                finished_send,
                watching_directory: None,
                content_library_dir: backend.directories.content_library_dir.clone(),
                frontend_alive: frontend_alive.clone(),
                file_watching: backend.file_watching.clone(),
                _backend_alive: backend_keep_alive,
            };

            *session_write = Some(session);

            backend.send.send(MessageToFrontend::ManualCurseforgeDownloadsRequired {
                request: ManualCurseforgeDownloadRequest {
                    initial_files: files,
                    download_dir_send,
                    add_files_recv,
                    finished_recv,
                    frontend_alive: frontend_keep_alive,
                    backend_alive,
                },
            });

            (download_dir_recv, frontend_alive)
        };

        // Wait for frontend to confirm download directory
        let download_dir = tokio::select! {
            download_dir = download_dir_recv => {
                download_dir.ok()
            },
            _ = frontend_alive.await_notification() => {
                None
            }
        };

        let (frontend_alive, stopped_notify) = {
            let mut lock = self.0.write();
            let Some(inner) = &mut *lock else {
                return;
            };

            if inner.id != id {
                return;
            }

            let Some(download_dir) = download_dir else {
                *lock = None;
                return;
            };

            if !download_dir.is_dir() {
                *lock = None;
                return;
            }

            // Start watching downloads directory for changes
            inner.file_watching.write().watch_filesystem(download_dir.clone(), WatchTarget::ManualCurseForgeDownloadDirectory { session_id: id });
            inner.watching_directory = Some(download_dir.clone());

            // Check files already in downloads directory
            let session = self.clone();
            tokio::task::spawn_blocking(move || {
                let Ok(mut read_dir) = std::fs::read_dir(&download_dir) else {
                    return;
                };
                while let Some(Ok(entry)) = read_dir.next() {
                    let Ok(metadata) = entry.metadata() else {
                        continue;
                    };

                    if session.process_candidate(&entry.path(), metadata, id, false) == SessionState::Inactive {
                        break;
                    }
                }
            });

            (inner.frontend_alive.clone(), inner.stopped_notify.clone())
        };

        tokio::select! {
            _ = frontend_alive.await_notification() => {},
            _ = stopped_notify.notified() => {}
        }

        let mut lock = self.0.write();
        let Some(inner) = &mut *lock else {
            return;
        };

        if inner.id != id {
            return;
        }

        inner.stop();
        *lock = None;
    }
}

impl ManualCurseforgeDownloadSessionInner {
    pub fn stop(&mut self) {
        if let Some(watch) = self.watching_directory.take() {
            let mut file_watching = self.file_watching.write();
            if let Some(WatchTarget::ManualCurseForgeDownloadDirectory { session_id }) = file_watching.get_target(&watch) {
                if *session_id == self.id {
                    file_watching.remove(&watch);
                }
            }
        }
        self.stopped_notify.notify_one();
    }
}
