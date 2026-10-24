//! Screenshots and clips for the overlay's gallery and viewer: find the newest ones, make video
//! thumbnails and preview frames, trim a clip without re-encoding, and copy things to the clipboard.
//! Video work uses Windows' own Media Foundation, so nothing extra has to be downloaded.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct MediaItem {
    pub path: PathBuf,
    pub video: bool,
    pub modified: std::time::SystemTime,
}

/// Flux screenshots (Pictures\Flux).
pub fn screenshots_dir() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Pictures").join("Flux")
}

/// Where Windows Game Bar saves clips (Videos\Captures).
pub fn clips_dir() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Videos").join("Captures")
}

/// The newest screenshots and clips, newest first.
pub fn recent(limit: usize) -> Vec<MediaItem> {
    let mut items = Vec::new();
    for (dir, video) in [(screenshots_dir(), false), (clips_dir(), true)] {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let ok = if video { ext == "mp4" } else { matches!(ext.as_str(), "png" | "jpg" | "jpeg") };
            if !ok {
                continue;
            }
            if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                items.push(MediaItem { path, video, modified });
            }
        }
    }
    items.sort_by(|a, b| b.modified.cmp(&a.modified));
    items.truncate(limit);
    items
}

fn cache_dir() -> PathBuf {
    let base = crate::LAUNCHER_DIR.get().cloned().unwrap_or_else(std::env::temp_dir);
    base.join("temp").join("media-thumbs")
}

/// A cached PNG of a video's frame at `secs` (made on first use).
pub fn frame(video: &Path, secs: f64) -> Result<PathBuf, String> {
    let stamp = std::fs::metadata(video).and_then(|m| m.modified()).ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let key = format!("{}-{stamp}-{}", video.display(), (secs * 10.0).round() as i64);
    let hash = key.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211));
    let out = cache_dir().join(format!("{hash:016x}.png"));
    if out.exists() {
        return Ok(out);
    }
    std::fs::create_dir_all(cache_dir()).map_err(|e| e.to_string())?;
    let (w, h, pixels) = imp::decode_frame(video, secs)?;
    let mut image = image::RgbaImage::from_raw(w, h, pixels).ok_or("bad frame")?;
    // Small enough for a gallery tile or the viewer.
    if w > 1280 {
        image = image::imageops::resize(&image, 1280, (1280 * h / w).max(1), image::imageops::FilterType::Triangle);
    }
    image.save(&out).map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn duration(video: &Path) -> Option<f64> {
    imp::duration(video)
}

/// Keep `start..end` seconds of a clip, as "<name> (trimmed).mp4" next to it. No re-encoding, so it's
/// quick and loses nothing; the cut lands on the keyframe at or before `start`.
pub fn trim(video: &Path, start: f64, end: f64) -> Result<PathBuf, String> {
    let stem = video.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "clip".into());
    let dir = video.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out = dir.join(format!("{stem} (trimmed).mp4"));
    let mut n = 2;
    while out.exists() {
        out = dir.join(format!("{stem} (trimmed {n}).mp4"));
        n += 1;
    }
    imp::trim(video, &out, start, end)?;
    Ok(out)
}

/// Put a file on the clipboard, ready to paste into Discord, Explorer, ...
pub fn copy_file(path: &Path) -> Result<(), String> {
    imp::copy_file(path)
}

pub fn show_in_folder(path: &Path) {
    _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
}

/// Open with the default app (the Photos/Media Player).
pub fn open(path: &Path) {
    _ = std::process::Command::new("explorer").arg(path).spawn();
}

/// What the clip player shows right now.
#[derive(Default)]
pub struct PlayerState {
    /// The newest frame: width, height, BGRA pixels; `seq` goes up with each new one.
    pub frame: Option<(u32, u32, Vec<u8>)>,
    pub seq: u64,
    pub time: f64,
    pub duration: f64,
    pub paused: bool,
    pub ended: bool,
    pub error: Option<String>,
}

enum PlayerCmd {
    Play,
    Pause,
    Seek(f64),
}

/// Plays a clip with its sound (Windows' Media Engine) on its own thread; frames land in `state`.
/// Dropping it stops playback.
pub struct Player {
    cmds: std::sync::mpsc::Sender<PlayerCmd>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub state: std::sync::Arc<std::sync::Mutex<PlayerState>>,
}

impl Player {
    pub fn open(path: &Path, muted: bool) -> Self {
        let (cmds, rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let state = std::sync::Arc::new(std::sync::Mutex::new(PlayerState::default()));
        let (path, s, st) = (path.to_path_buf(), state.clone(), stop.clone());
        std::thread::Builder::new().name("flux-clip-player".into()).spawn(move || {
            if let Err(e) = imp::run_player(&path, muted, rx, &s, &st) {
                s.lock().unwrap().error = Some(e);
            }
        }).ok();
        Self { cmds, stop, state }
    }

    pub fn play(&self) {
        _ = self.cmds.send(PlayerCmd::Play);
    }

    pub fn pause(&self) {
        _ = self.cmds.send(PlayerCmd::Pause);
    }

    pub fn seek(&self, secs: f64) {
        _ = self.cmds.send(PlayerCmd::Seek(secs.max(0.0)));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(windows)]
mod player_imp {
    use windows::Win32::Media::MediaFoundation::{IMFMediaEngineNotify, IMFMediaEngineNotify_Impl};

    /// The Media Engine insists on a callback; Flux polls its state instead.
    #[windows::core::implement(IMFMediaEngineNotify)]
    pub struct Notify;

    impl IMFMediaEngineNotify_Impl for Notify_Impl {
        fn EventNotify(&self, _event: u32, _param1: usize, _param2: u32) -> windows::core::Result<()> {
            Ok(())
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::{
        Win32::{
            Foundation::{GlobalFree, HANDLE, HGLOBAL},
            Media::MediaFoundation::*,
            System::{
                Com::{COINIT_MULTITHREADED, CoInitializeEx},
                DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
                Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            },
            UI::Shell::DROPFILES,
        },
        Win32::System::{Com::StructuredStorage::PROPVARIANT, Variant::VT_I8},
        core::{GUID, HSTRING},
    };

    /// A 64-bit time (100 ns units) as the PROPVARIANT Media Foundation wants.
    fn time(v: i64) -> PROPVARIANT {
        let mut p = PROPVARIANT::default();
        unsafe {
            let inner = &mut *p.Anonymous.Anonymous;
            inner.vt = VT_I8;
            inner.Anonymous.hVal = v;
        }
        p
    }

    fn start() -> Result<(), String> {
        unsafe {
            _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| e.to_string())
        }
    }

    const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    const ANY_STREAM: u32 = MF_SOURCE_READER_ANY_STREAM.0 as u32;

    pub fn duration(video: &Path) -> Option<f64> {
        start().ok()?;
        unsafe {
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(video.as_os_str()), None).ok()?;
            let v = reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION).ok()?;
            let ticks = v.Anonymous.Anonymous.Anonymous.uhVal;
            Some(ticks as f64 / 10_000_000.0)
        }
    }

    /// BGRA -> RGBA pixels of the frame at `secs`.
    pub fn decode_frame(video: &Path, secs: f64) -> Result<(u32, u32, Vec<u8>), String> {
        start()?;
        unsafe {
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 1).map_err(|e| e.to_string())?;
            let attrs = attrs.ok_or("no attributes")?;
            attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).map_err(|e| e.to_string())?;
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(video.as_os_str()), &attrs).map_err(|e| e.to_string())?;
            let ty = MFCreateMediaType().map_err(|e| e.to_string())?;
            ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| e.to_string())?;
            ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).map_err(|e| e.to_string())?;
            reader.SetCurrentMediaType(FIRST_VIDEO, None, &ty).map_err(|e| e.to_string())?;
            let target = (secs.max(0.0) * 10_000_000.0) as i64;
            if target > 0 {
                _ = reader.SetCurrentPosition(&GUID::zeroed(), &time(target));
            }
            let current = reader.GetCurrentMediaType(FIRST_VIDEO).map_err(|e| e.to_string())?;
            let size = current.GetUINT64(&MF_MT_FRAME_SIZE).map_err(|e| e.to_string())?;
            let (w, h) = ((size >> 32) as u32, (size & 0xffff_ffff) as u32);
            let mut last = None;
            for _ in 0..600 {
                let (mut flags, mut ts, mut sample) = (0u32, 0i64, None);
                reader.ReadSample(FIRST_VIDEO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample)).map_err(|e| e.to_string())?;
                if let Some(s) = sample {
                    last = Some(s);
                    if ts >= target {
                        break;
                    }
                }
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    break;
                }
            }
            let sample = last.ok_or("no video frames")?;
            let buffer = sample.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
            let (mut ptr, mut len) = (std::ptr::null_mut(), 0u32);
            buffer.Lock(&mut ptr, None, Some(&mut len)).map_err(|e| e.to_string())?;
            let need = (w * h * 4) as usize;
            let mut pixels = std::slice::from_raw_parts(ptr, (len as usize).min(need)).to_vec();
            _ = buffer.Unlock();
            pixels.resize(need, 0);
            for px in pixels.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            Ok((w, h, pixels))
        }
    }

    pub fn trim(video: &Path, out: &Path, start_s: f64, end_s: f64) -> Result<(), String> {
        start()?;
        unsafe {
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(video.as_os_str()), None).map_err(|e| e.to_string())?;
            let writer = MFCreateSinkWriterFromURL(&HSTRING::from(out.as_os_str()), None, None).map_err(|e| e.to_string())?;
            // Every stream (video and audio) goes through as it is: no decoding, no encoding.
            let mut map = Vec::new();
            for i in 0..8u32 {
                let Ok(native) = reader.GetNativeMediaType(i, 0) else { break };
                reader.SetStreamSelection(i, true).map_err(|e| e.to_string())?;
                reader.SetCurrentMediaType(i, None, &native).map_err(|e| e.to_string())?;
                let index = writer.AddStream(&native).map_err(|e| format!("can't copy stream {i}: {e}"))?;
                writer.SetInputMediaType(index, &native, None).map_err(|e| e.to_string())?;
                map.push((i, index));
            }
            if map.is_empty() {
                return Err("no streams in this video".into());
            }
            writer.BeginWriting().map_err(|e| e.to_string())?;
            let start = (start_s.max(0.0) * 10_000_000.0) as i64;
            let end = (end_s * 10_000_000.0) as i64;
            _ = reader.SetCurrentPosition(&GUID::zeroed(), &time(start));
            let mut base: Option<i64> = None;
            let mut done = vec![false; map.len()];
            while done.iter().any(|d| !d) {
                let (mut actual, mut flags, mut ts, mut sample) = (0u32, 0u32, 0i64, None);
                reader.ReadSample(ANY_STREAM, 0, Some(&mut actual), Some(&mut flags), Some(&mut ts), Some(&mut sample))
                    .map_err(|e| e.to_string())?;
                let Some(slot) = map.iter().position(|(i, _)| *i == actual) else { continue };
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    done[slot] = true;
                    continue;
                }
                let Some(sample) = sample else { continue };
                if ts > end {
                    done[slot] = true;
                    continue;
                }
                let base = *base.get_or_insert(ts);
                sample.SetSampleTime((ts - base).max(0)).map_err(|e| e.to_string())?;
                writer.WriteSample(map[slot].1, &sample).map_err(|e| e.to_string())?;
            }
            writer.Finalize().map_err(|e| e.to_string())?;
            Ok(())
        }
    }

    pub fn run_player(path: &Path, muted: bool, cmds: std::sync::mpsc::Receiver<super::PlayerCmd>,
        state: &std::sync::Mutex<super::PlayerState>, stop: &std::sync::atomic::AtomicBool) -> Result<(), String>
    {
        use windows::Win32::{
            Foundation::RECT,
            Graphics::Imaging::{CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmap, IWICImagingFactory, WICBitmapCacheOnLoad, WICBitmapLockRead, WICRect},
            System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        };
        start()?;
        unsafe {
            let factory: IMFMediaEngineClassFactory = CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
            let notify: IMFMediaEngineNotify = super::player_imp::Notify.into();
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 2).map_err(|e| e.to_string())?;
            let attrs = attrs.ok_or("no attributes")?;
            attrs.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify).map_err(|e| e.to_string())?;
            // 87 = DXGI_FORMAT_B8G8R8A8_UNORM
            attrs.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, 87).map_err(|e| e.to_string())?;
            let engine = factory.CreateInstance(0, &attrs).map_err(|e| e.to_string())?;
            engine.SetMuted(muted).ok();
            engine.SetSource(&windows::core::BSTR::from(path.to_string_lossy().as_ref())).map_err(|e| e.to_string())?;
            engine.Play().map_err(|e| e.to_string())?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
            let mut bitmap: Option<(u32, u32, IWICBitmap)> = None;
            let mut last_pts = -1i64;
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                while let Ok(cmd) = cmds.try_recv() {
                    match cmd {
                        super::PlayerCmd::Play => {
                            if engine.IsEnded().as_bool() {
                                _ = engine.SetCurrentTime(0.0);
                            }
                            _ = engine.Play();
                        },
                        super::PlayerCmd::Pause => _ = engine.Pause(),
                        super::PlayerCmd::Seek(t) => _ = engine.SetCurrentTime(t),
                    }
                }
                if let Ok(pts) = engine.OnVideoStreamTick() && pts != last_pts {
                    let (mut w, mut h) = (0u32, 0u32);
                    if engine.GetNativeVideoSize(Some(&mut w), Some(&mut h)).is_ok() && w > 0 && h > 0 {
                        // Big clips are shown smaller: plenty for the overlay, and cheap to copy.
                        let (dw, dh) = if w > 1280 { (1280, (1280 * h / w).max(1)) } else { (w, h) };
                        if bitmap.as_ref().is_none_or(|b| b.0 != dw || b.1 != dh) {
                            bitmap = wic.CreateBitmap(dw, dh, &GUID_WICPixelFormat32bppBGRA, WICBitmapCacheOnLoad).ok().map(|b| (dw, dh, b));
                        }
                        if let Some((bw, bh, b)) = &bitmap {
                            let dst = RECT { left: 0, top: 0, right: *bw as i32, bottom: *bh as i32 };
                            if engine.TransferVideoFrame(b, None, &dst, None).is_ok() {
                                last_pts = pts;
                                let rect = WICRect { X: 0, Y: 0, Width: *bw as i32, Height: *bh as i32 };
                                if let Ok(lock) = b.Lock(&rect, WICBitmapLockRead.0 as u32) {
                                    let (mut size, mut ptr) = (0u32, std::ptr::null_mut());
                                    let stride = lock.GetStride().unwrap_or(*bw * 4) as usize;
                                    if lock.GetDataPointer(&mut size, &mut ptr).is_ok() && !ptr.is_null() {
                                        let data = std::slice::from_raw_parts(ptr, size as usize);
                                        let row = (*bw * 4) as usize;
                                        let mut pixels = Vec::with_capacity(row * *bh as usize);
                                        for y in 0..*bh as usize {
                                            pixels.extend_from_slice(&data[y * stride..y * stride + row]);
                                        }
                                        let mut s = state.lock().unwrap();
                                        s.frame = Some((*bw, *bh, pixels));
                                        s.seq += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                {
                    let mut s = state.lock().unwrap();
                    s.time = engine.GetCurrentTime();
                    let d = engine.GetDuration();
                    if d.is_finite() {
                        s.duration = d;
                    }
                    s.paused = engine.IsPaused().as_bool();
                    s.ended = engine.IsEnded().as_bool();
                    if let Ok(err) = engine.GetError() {
                        s.error = Some(format!("Couldn't play this clip (error {})", err.GetErrorCode()));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            _ = engine.Shutdown();
            Ok(())
        }
    }

    pub fn copy_file(path: &Path) -> Result<(), String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide_list();
        let header = std::mem::size_of::<DROPFILES>();
        let bytes = header + wide.len() * 2;
        unsafe {
            let mem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| e.to_string())?;
            let p = GlobalLock(mem) as *mut u8;
            if p.is_null() {
                _ = GlobalFree(Some(mem));
                return Err("out of memory".into());
            }
            let drop = DROPFILES { pFiles: header as u32, fWide: true.into(), ..Default::default() };
            std::ptr::write(p as *mut DROPFILES, drop);
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, p.add(header), wide.len() * 2);
            _ = GlobalUnlock(mem);
            OpenClipboard(None).map_err(|e| e.to_string())?;
            _ = EmptyClipboard();
            // 15 = CF_HDROP: a list of files.
            let result = SetClipboardData(15, Some(HANDLE(mem.0)));
            _ = CloseClipboard();
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    }

    trait WideList {
        fn encode_wide_list(&self) -> Vec<u16>;
    }
    impl WideList for std::ffi::OsStr {
        /// The path, then the empty string that ends the list.
        fn encode_wide_list(&self) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            self.encode_wide().chain([0, 0]).collect()
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;
    pub fn duration(_video: &Path) -> Option<f64> { None }
    pub fn decode_frame(_video: &Path, _secs: f64) -> Result<(u32, u32, Vec<u8>), String> { Err("Windows only".into()) }
    pub fn trim(_video: &Path, _out: &Path, _start: f64, _end: f64) -> Result<(), String> { Err("Windows only".into()) }
    pub fn copy_file(_path: &Path) -> Result<(), String> { Err("Windows only".into()) }
    pub fn run_player(_path: &Path, _muted: bool, _cmds: std::sync::mpsc::Receiver<super::PlayerCmd>,
        _state: &std::sync::Mutex<super::PlayerState>, _stop: &std::sync::atomic::AtomicBool) -> Result<(), String> { Err("Windows only".into()) }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// FLUX_TEST_CLIP=<an mp4>: frames, duration and trimming on a real clip.
    #[test]
    #[ignore]
    fn clip() {
        let Ok(path) = std::env::var("FLUX_TEST_CLIP") else { return };
        let path = PathBuf::from(path);
        let d = duration(&path).expect("duration");
        assert!(d > 1.0, "{d}");
        let (w, h, px) = imp::decode_frame(&path, 1.0).expect("frame");
        assert_eq!(px.len(), (w * h * 4) as usize);
        let out = trim(&path, 1.0, d - 1.0).expect("trim");
        let td = duration(&out).expect("trimmed duration");
        assert!(td > 0.5 && td < d, "{td} vs {d}");
        std::fs::remove_file(out).ok();
        // Playback (muted): frames arrive and time moves on.
        let player = Player::open(&path, true);
        std::thread::sleep(std::time::Duration::from_millis(2500));
        let s = player.state.lock().unwrap();
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(s.seq > 10, "only {} frames", s.seq);
        assert!(s.time > 1.0, "time {}", s.time);
        let (w, h, px) = s.frame.as_ref().unwrap();
        assert_eq!(px.len(), (w * h * 4) as usize);
        let seq = s.seq;
        drop(s);
        // Skimming while paused: the frame at the new spot still arrives.
        player.pause();
        std::thread::sleep(std::time::Duration::from_millis(300));
        player.seek(0.5);
        std::thread::sleep(std::time::Duration::from_millis(600));
        let s = player.state.lock().unwrap();
        assert!(s.paused, "should stay paused");
        assert!((s.time - 0.5).abs() < 0.3, "time {}", s.time);
        assert!(s.seq > seq, "no new frame after seeking while paused");
    }
}
