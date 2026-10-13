//! SK8V (github.com/becaring/SK8V, Apache-2.0): Skate 3's skating in GTA V Legacy Story Mode, a
//! fan mod by becaring. Flux installs it the way the release's own setup window does, without the
//! window:
//!
//! 1. the latest full release from GitHub, checked against the SHA256SUMS.txt inside it;
//! 2. the Python its setup runs on, from the pins in its tools/get-python.ps1 (python.org and PyPI,
//!    each checked against its SHA-256), unpacked the way that script does, without PowerShell;
//! 3. ScriptHookV from its publisher (dev-c.com, the archive SK8V's external.lock.json pins), when
//!    the GTA folder doesn't have it yet (SK8V's setup expects it to be there);
//! 4. the release's `tools/prepare.py --install` with the player's own Skate 3 (Xbox 360) disc and
//!    GTA V Legacy folder. Its `== stage: what it does` and `@progress <percent>` lines become the
//!    progress Flux shows. A protected GTA folder (Program Files) runs this step as administrator.
//!
//! Everything SK8V needs is supplied by the player's own game copies; nothing of either game is
//! downloaded. Flux only ever starts GTA V for SK8V in Story Mode (see the frontend's sk8v.rs).

use std::{io::{Read, Write}, path::{Path, PathBuf}, sync::Arc};

use bridge::modal_action::{ModalAction, ProgressTrackerFinishType};
use sha2::Digest;

use crate::BackendState;

const REPO: &str = "becaring/SK8V";
/// What Flux wrote after the last successful install (version, folders, stance).
pub const INSTALLED_FILE: &str = "flux-installed.json";

type Res<T> = Result<T, Arc<str>>;

fn err(text: impl Into<String>) -> Arc<str> {
    Arc::from(text.into().as_str())
}

pub fn sk8v_dir(launcher_dir: &Path) -> PathBuf {
    launcher_dir.join("sk8v")
}

/// The Python pins in SK8V's tools/get-python.ps1: (url, sha256) pairs.
pub fn python_pins(script: &str) -> Vec<(String, String)> {
    let quoted = |line: &str, key: &str| -> Option<String> {
        let rest = line.split(key).nth(1)?;
        let start = rest.find('\'')? + 1;
        let end = rest[start..].find('\'')? + start;
        Some(rest[start..end].to_string())
    };
    let mut pins = Vec::new();
    let mut url = None;
    for line in script.lines() {
        if let Some(u) = quoted(line, "Url =") {
            url = Some(u);
        }
        if let (Some(u), Some(sha)) = (url.as_ref(), quoted(line, "Sha =")) {
            let ok_host = u.starts_with("https://www.python.org/") || u.starts_with("https://files.pythonhosted.org/");
            if ok_host && sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
                pins.push((u.clone(), sha.to_lowercase()));
            }
            url = None;
        }
    }
    pins
}

/// `<sha256>  <path>` lines of a SHA256SUMS.txt (a `*` before the path marks binary mode).
pub fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|line| {
        let (hash, path) = line.trim().split_once(char::is_whitespace)?;
        let path = path.trim().trim_start_matches('*').trim();
        (hash.len() == 64 && !path.is_empty()).then(|| (hash.to_lowercase(), path.replace('\\', "/")))
    }).collect()
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hex::encode(hasher.finalize()))
}

/// Unpack a zip from disk into `into`; `strip` drops that many leading folders from each path.
fn unzip(archive: &Path, into: &Path, only: Option<&dyn Fn(&str) -> bool>) -> Res<()> {
    let file = std::fs::File::open(archive).map_err(|e| err(format!("Couldn't open {}: {e}", archive.display())))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| err("The download isn't a valid zip"))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|_| err("Corrupt zip entry"))?;
        // enclosed_name rejects absolute paths and `..`, so nothing can land outside the folder.
        let Some(relative) = entry.enclosed_name() else { continue };
        if only.is_some_and(|keep| !keep(&relative.to_string_lossy().replace('\\', "/"))) {
            continue;
        }
        let out = into.join(relative);
        if entry.is_dir() {
            _ = std::fs::create_dir_all(&out);
            continue;
        }
        if let Some(parent) = out.parent() {
            _ = std::fs::create_dir_all(parent);
        }
        let mut target = std::fs::File::create(&out).map_err(|e| err(format!("Couldn't write {}: {e}", out.display())))?;
        std::io::copy(&mut entry, &mut target).map_err(|e| err(format!("Couldn't write {}: {e}", out.display())))?;
    }
    Ok(())
}

/// Can a file be created in `folder`? (Windows' read-only flag doesn't tell: its permissions do.)
pub fn writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".flux-write-test-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            _ = std::fs::remove_file(&probe);
            true
        },
        Err(_) => false,
    }
}

/// A path as a quoted cmd.exe argument (`%` doubled so cmd doesn't expand it).
fn cmd_quote(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().trim_end_matches('\\').replace('%', "%%"))
}

pub struct Sk8vInstall {
    pub skate: PathBuf,
    pub gta: PathBuf,
    pub goofy: bool,
}

impl BackendState {
    pub async fn install_sk8v(&self, request: Sk8vInstall, modal_action: ModalAction) {
        match self.install_sk8v_inner(&request, &modal_action).await {
            Ok(version) => {
                self.send.send_success(format!("SK8V {version} is installed: press PLAY on its page"));
                modal_action.set_finished();
            },
            Err(error) => {
                log::error!("SK8V install failed: {error}");
                modal_action.set_finished_with_error(error);
            },
        }
    }

    async fn download_to(&self, url: &str, to: &Path, title: String, modal_action: &ModalAction, headers: &[(&str, &str)]) -> Res<()> {
        let tracker = modal_action.push_tracker(Arc::from(title.as_str()));
        let mut request = self.http_client_provider.redirecting().get(url);
        for (k, v) in headers {
            request = request.header(*k, *v);
        }
        let mut response = request.send().await.and_then(|r| r.error_for_status())
            .map_err(|e| err(format!("Couldn't download {url}: {e}")))?;
        tracker.set_total(response.content_length().unwrap_or(0) as usize);
        let part = to.with_extension("part");
        let mut file = std::io::BufWriter::new(std::fs::File::create(&part).map_err(|e| err(format!("Couldn't save the download: {e}")))?);
        while let Some(chunk) = response.chunk().await.map_err(|_| err("The download was interrupted"))? {
            if modal_action.has_requested_cancel() {
                drop(file);
                _ = std::fs::remove_file(&part);
                return Err(err("Cancelled"));
            }
            file.write_all(&chunk).map_err(|_| err("Couldn't save the download (is the disk full?)"))?;
            tracker.add_count(chunk.len());
        }
        file.flush().map_err(|_| err("Couldn't save the download"))?;
        drop(file);
        std::fs::rename(&part, to).map_err(|e| err(format!("Couldn't save the download: {e}")))?;
        tracker.set_finished(ProgressTrackerFinishType::Normal);
        Ok(())
    }

    async fn install_sk8v_inner(&self, request: &Sk8vInstall, modal_action: &ModalAction) -> Res<String> {
        let gta = &request.gta;
        if !gta.join("GTA5.exe").is_file() {
            return Err(err("Pick the GTA V Legacy folder (the one with GTA5.exe)"));
        }
        if !request.skate.exists() {
            return Err(err("Your Skate 3 disc image wasn't found"));
        }
        let dir = sk8v_dir(&self.directories.root_launcher_dir);
        std::fs::create_dir_all(&dir).map_err(|e| err(format!("Couldn't create {}: {e}", dir.display())))?;

        // ---- 1. The latest release.
        let tracker = modal_action.push_tracker(Arc::from("Finding the latest SK8V"));
        tracker.set_total(1);
        let client = self.http_client_provider.redirecting();
        let release: serde_json::Value = client.get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
            .header("Accept", "application/vnd.github+json")
            .send().await.map_err(|_| err("Couldn't reach GitHub"))?
            .json().await.map_err(|_| err("Unexpected answer from GitHub"))?;
        let tag = release["tag_name"].as_str().ok_or_else(|| err("SK8V has no release yet"))?.to_string();
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
            return Err(err("Unexpected SK8V release name"));
        }
        let wanted = format!("SK8V-{tag}.zip");
        let asset = release["assets"].as_array().into_iter().flatten()
            .find(|a| a["name"].as_str() == Some(wanted.as_str()))
            .ok_or_else(|| err(format!("SK8V {tag} has no {wanted} to download")))?;
        let asset_url = asset["browser_download_url"].as_str().ok_or_else(|| err("Unexpected answer from GitHub"))?.to_string();
        tracker.add_count(1);
        tracker.set_finished(ProgressTrackerFinishType::Normal);

        let release_dir = dir.join(format!("SK8V-{tag}"));
        if !release_dir.join("tools").join("prepare.py").is_file() {
            let zip_path = dir.join(&wanted);
            self.download_to(&asset_url, &zip_path, format!("Downloading SK8V {tag}"), modal_action, &[]).await?;
            let tracker = modal_action.push_tracker(Arc::from("Unpacking and checking SK8V"));
            tracker.set_total(1);
            let (zip, into) = (zip_path.clone(), dir.clone());
            tokio::task::spawn_blocking(move || unzip(&zip, &into, None)).await.map_err(|_| err("Unpacking stopped"))??;
            _ = std::fs::remove_file(&zip_path);
            let sums = std::fs::read_to_string(release_dir.join("SHA256SUMS.txt")).unwrap_or_default();
            let sums = parse_sums(&sums);
            if sums.is_empty() {
                return Err(err("The SK8V release has no SHA256SUMS.txt to check it against"));
            }
            let root = release_dir.clone();
            let bad = tokio::task::spawn_blocking(move || {
                sums.into_iter().find(|(hash, path)| sha256_file(&root.join(path)).as_deref() != Some(hash.as_str())).map(|(_, p)| p)
            }).await.map_err(|_| err("Checking stopped"))?;
            if let Some(path) = bad {
                _ = std::fs::remove_dir_all(&release_dir);
                return Err(err(format!("{path} in the SK8V download doesn't match its checksum, so it was removed. Try again.")));
            }
            tracker.add_count(1);
            tracker.set_finished(ProgressTrackerFinishType::Normal);
            // Older releases' folders (and the Python inside them) aren't needed any more.
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("SK8V-") && entry.path() != release_dir && entry.path().is_dir() {
                    _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }

        // ---- 2. Its Python.
        let python_dir = release_dir.join("python");
        if !python_dir.join("python.exe").is_file() {
            let script = std::fs::read_to_string(release_dir.join("tools").join("get-python.ps1")).unwrap_or_default();
            let pins = python_pins(&script);
            if pins.len() < 3 {
                return Err(err("Couldn't read the Python SK8V's setup needs from its get-python.ps1"));
            }
            let stage = release_dir.join("python.download");
            _ = std::fs::remove_dir_all(&stage);
            std::fs::create_dir_all(stage.join("py")).map_err(|e| err(format!("Couldn't create {}: {e}", stage.display())))?;
            for (url, sha) in &pins {
                let name = url.rsplit('/').next().unwrap_or("download").to_string();
                let file = stage.join(&name);
                self.download_to(url, &file, format!("Downloading {name}"), modal_action, &[]).await?;
                if sha256_file(&file).as_deref() != Some(sha.as_str()) {
                    _ = std::fs::remove_dir_all(&stage);
                    return Err(err(format!("{name} doesn't match its published SHA-256, so nothing was installed")));
                }
                // The interpreter goes to the root, the wheels (plain zips) into site-packages.
                let dest = if name.ends_with(".zip") { stage.join("py") } else { stage.join("py").join("Lib").join("site-packages") };
                let (f, d) = (file.clone(), dest.clone());
                tokio::task::spawn_blocking(move || unzip(&f, &d, None)).await.map_err(|_| err("Unpacking stopped"))??;
                _ = std::fs::remove_file(&file);
            }
            // As get-python.ps1: without its ._pth the embeddable Python starts like a normal one.
            for entry in std::fs::read_dir(stage.join("py")).into_iter().flatten().flatten() {
                let n = entry.file_name().to_string_lossy().to_lowercase();
                if n.starts_with("python") && n.ends_with("._pth") {
                    _ = std::fs::remove_file(entry.path());
                }
            }
            std::fs::rename(stage.join("py"), &python_dir).map_err(|e| err(format!("Couldn't set up Python: {e}")))?;
            _ = std::fs::remove_dir_all(&stage);
        }

        // ---- 3. ScriptHookV, from its publisher, when the GTA folder doesn't have it.
        let mut copy_in: Vec<(PathBuf, PathBuf)> = Vec::new();
        let missing: Vec<&str> = ["ScriptHookV.dll", "dinput8.dll"].into_iter().filter(|f| !gta.join(f).is_file()).collect();
        if !missing.is_empty() {
            let lock: serde_json::Value = client.get(format!("https://raw.githubusercontent.com/{REPO}/{tag}/external.lock.json"))
                .send().await.map_err(|_| err("Couldn't reach GitHub"))?
                .json().await.map_err(|_| err("Couldn't read which ScriptHookV SK8V needs"))?;
            let shv = &lock["targets"]["scripthookv"];
            let (Some(url), Some(page)) = (shv["runtime_archive"].as_str(), shv["page"].as_str()) else {
                return Err(err("Couldn't read which ScriptHookV SK8V needs"));
            };
            if !url.starts_with("https://www.dev-c.com/") {
                return Err(err("SK8V points at ScriptHookV somewhere other than dev-c.com, so Flux didn't download it"));
            }
            let shv_dir = dir.join("scripthookv");
            _ = std::fs::remove_dir_all(&shv_dir);
            std::fs::create_dir_all(&shv_dir).map_err(|e| err(format!("Couldn't create {}: {e}", shv_dir.display())))?;
            let zip_path = shv_dir.join("ScriptHookV.zip");
            // dev-c.com only serves its files to visitors coming from its own page.
            self.download_to(url, &zip_path, "Getting ScriptHookV from dev-c.com".into(), modal_action, &[("Referer", page)]).await?;
            let keep = |p: &str| { let l = p.to_lowercase(); l.ends_with("scripthookv.dll") || l.ends_with("dinput8.dll") };
            let (z, d) = (zip_path.clone(), shv_dir.clone());
            tokio::task::spawn_blocking(move || unzip(&z, &d, Some(&keep))).await.map_err(|_| err("Unpacking stopped"))??;
            for name in missing {
                let found = walk_find(&shv_dir, name).ok_or_else(|| err(format!("The ScriptHookV download has no {name}")))?;
                copy_in.push((found, gta.join(name)));
            }
        }

        // ---- 4. SK8V's own setup: prepare the data from both games and install it.
        let log_path = dir.join("setup.log");
        _ = std::fs::remove_file(&log_path);
        let mut script = String::from("@echo off\r\nchcp 65001 >nul\r\n");
        for (from, to) in &copy_in {
            script.push_str(&format!("copy /y {} {} >nul || exit /b 1\r\n", cmd_quote(from), cmd_quote(to)));
        }
        script.push_str(&format!("set \"PYTHONHOME={}\"\r\nset PYTHONPATH=\r\nset PYTHONNOUSERSITE=1\r\nset PYTHONIOENCODING=utf-8\r\n",
            python_dir.display().to_string().replace('%', "%%")));
        script.push_str(&format!("{} -B -u {} --skate {} --gta {} --stance {} --install --clean\r\n",
            cmd_quote(&python_dir.join("python.exe")), cmd_quote(&release_dir.join("tools").join("prepare.py")),
            cmd_quote(&request.skate), cmd_quote(gta), if request.goofy { "Goofy" } else { "Regular" }));
        script.push_str("exit /b %ERRORLEVEL%\r\n");
        let script_path = dir.join("run-setup.cmd");
        std::fs::write(&script_path, script).map_err(|e| err(format!("Couldn't write {}: {e}", script_path.display())))?;
        let args = format!("/d /c \"{} > {} 2>&1\"", cmd_quote(&script_path), cmd_quote(&log_path));
        let elevate = !writable(gta);

        let overall = modal_action.push_tracker(Arc::from(if elevate {
            "Setting up SK8V (Windows asks for permission: the GTA V folder is protected)"
        } else {
            "Setting up SK8V (several minutes; it picks up where it left off if stopped)"
        }));
        overall.set_total(100);
        let process = run_cmd(&args, &release_dir, elevate)?;
        let mut read = 0usize;
        let mut partial = String::new();
        let mut last_line = String::new();
        let mut stage: Option<bridge::modal_action::ProgressTracker> = None;
        loop {
            let exited = process.exit_code();
            if let Ok(bytes) = std::fs::read(&log_path) && bytes.len() > read {
                partial.push_str(&String::from_utf8_lossy(&bytes[read..]));
                read = bytes.len();
                while let Some(end) = partial.find('\n') {
                    let line = partial[..end].trim().to_string();
                    partial.drain(..=end);
                    if let Some(percent) = line.strip_prefix("@progress ") {
                        if let Ok(p) = percent.trim().parse::<usize>() {
                            overall.set_count(p.min(100));
                        }
                    } else if let Some(rest) = line.strip_prefix("== ") {
                        if let Some(old) = stage.take() {
                            old.set_finished(ProgressTrackerFinishType::Normal);
                        }
                        let label = rest.split_once(": ").map(|(_, what)| what).unwrap_or(rest);
                        let t = modal_action.push_tracker(Arc::from(label));
                        t.set_total(1);
                        stage = Some(t);
                    }
                    if !line.is_empty() {
                        last_line = line;
                    }
                }
            }
            if let Some(code) = exited {
                if let Some(old) = stage.take() {
                    old.set_finished(ProgressTrackerFinishType::Normal);
                }
                if code != 0 {
                    let why = last_line.trim_start_matches("setup: ").to_string();
                    return Err(err(if why.is_empty() { format!("SK8V's setup stopped (code {code})") } else { format!("SK8V's setup stopped: {why}") }));
                }
                break;
            }
            if modal_action.has_requested_cancel() {
                process.kill_tree();
                return Err(err("Cancelled. Installing again picks up where it stopped."));
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        overall.set_count(100);
        overall.set_finished(ProgressTrackerFinishType::Normal);

        let record = serde_json::json!({
            "version": tag,
            "gta": gta,
            "skate": request.skate,
            "stance": if request.goofy { "Goofy" } else { "Regular" },
            "installed_at": chrono::Utc::now().to_rfc3339(),
        });
        _ = std::fs::write(dir.join(INSTALLED_FILE), serde_json::to_vec_pretty(&record).unwrap_or_default());
        Ok(tag)
    }
}

fn walk_find(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = walk_find(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)) {
            return Some(path);
        }
    }
    None
}

/// The setup script running in cmd.exe, as the player or (protected GTA folder) as administrator.
enum SetupProcess {
    Child(std::sync::Mutex<std::process::Child>),
    #[cfg(windows)]
    Elevated(isize, u32),
}

impl SetupProcess {
    fn exit_code(&self) -> Option<i32> {
        match self {
            SetupProcess::Child(child) => child.lock().ok()?.try_wait().ok().flatten().map(|s| s.code().unwrap_or(1)),
            #[cfg(windows)]
            SetupProcess::Elevated(handle, _) => unsafe {
                use windows::Win32::{Foundation::{HANDLE, STILL_ACTIVE}, System::Threading::GetExitCodeProcess};
                let mut code = 0u32;
                GetExitCodeProcess(HANDLE(*handle as _), &mut code).ok()?;
                (code != STILL_ACTIVE.0 as u32).then_some(code as i32)
            },
        }
    }

    /// Stops cmd.exe and everything it started (Python and the extractors under it).
    fn kill_tree(&self) {
        let pid = match self {
            SetupProcess::Child(child) => child.lock().map(|c| c.id()).unwrap_or(0),
            #[cfg(windows)]
            SetupProcess::Elevated(_, pid) => *pid,
        };
        if pid != 0 {
            let mut command = std::process::Command::new("taskkill");
            command.args(["/T", "/F", "/PID", &pid.to_string()]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            }
            _ = command.status();
        }
    }
}

#[cfg(windows)]
impl Drop for SetupProcess {
    fn drop(&mut self) {
        if let SetupProcess::Elevated(handle, _) = self {
            unsafe { _ = windows::Win32::Foundation::CloseHandle(windows::Win32::Foundation::HANDLE(*handle as _)); }
        }
    }
}

fn run_cmd(args: &str, dir: &Path, elevate: bool) -> Res<SetupProcess> {
    let cmd = std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
    #[cfg(windows)]
    if elevate {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::UI::{Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW}, WindowsAndMessaging::SW_HIDE};
        let wide = |s: &std::ffi::OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();
        let (file, params, cwd) = (wide(cmd.as_os_str()), wide(args.as_ref()), wide(dir.as_os_str()));
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
            lpVerb: windows::core::w!("runas"),
            lpFile: windows::core::PCWSTR(file.as_ptr()),
            lpParameters: windows::core::PCWSTR(params.as_ptr()),
            lpDirectory: windows::core::PCWSTR(cwd.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut sei) }.map_err(|_| err("Windows didn't give permission, so SK8V wasn't installed"))?;
        if sei.hProcess.is_invalid() {
            return Err(err("Couldn't start SK8V's setup"));
        }
        let pid = unsafe { windows::Win32::System::Threading::GetProcessId(sei.hProcess) };
        return Ok(SetupProcess::Elevated(sei.hProcess.0 as isize, pid));
    }
    let _ = elevate;
    let mut command = std::process::Command::new(cmd);
    command.current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(args);
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = command.spawn().map_err(|e| err(format!("Couldn't start SK8V's setup: {e}")))?;
    Ok(SetupProcess::Child(std::sync::Mutex::new(child)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_python_pins() {
        let script = "$Pins = @(\n  @{ Url = 'https://www.python.org/ftp/python/3.13.15/python-3.13.15-embed-amd64.zip'\n     Sha = 'd1f04d990aee1253d8569e8e5104e30fa9f5fa830899f14843448872d936a2cf' },\n  @{ Url = 'https://evil.example/x.whl'\n     Sha = 'b0544343a702fa80c95ad5d3d608ea3599dd54d4632df855e4c8d24eb6ecfa1c' }\n)";
        let pins = python_pins(script);
        assert_eq!(pins.len(), 1);
        assert!(pins[0].0.ends_with("embed-amd64.zip"));
    }

    #[test]
    fn reads_sums() {
        let sums = parse_sums("ab12  tools/x.py\n0000000000000000000000000000000000000000000000000000000000000000 *tools/prepare.py\n");
        assert_eq!(sums, vec![("0000000000000000000000000000000000000000000000000000000000000000".to_string(), "tools/prepare.py".to_string())]);
    }
}
