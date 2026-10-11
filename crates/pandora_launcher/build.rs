fn main() {
    windows();
}

/// The exe's icon and version info (what Explorer's Properties > Details shows, and what antivirus
/// reputation systems read): Flux's own name, publisher and release version.
#[cfg(windows)]
fn windows() {
    println!("cargo:rerun-if-env-changed=FLUX_VERSION");
    let version = std::env::var("FLUX_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut parts = version.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).chain(std::iter::repeat(0));
    let packed = (parts.next().unwrap() << 48) | (parts.next().unwrap() << 32) | (parts.next().unwrap() << 16) | parts.next().unwrap();

    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../package/windows.ico");
    res.set("ProductName", "Flux");
    res.set("FileDescription", "Flux Launcher");
    res.set("CompanyName", "PineappleJuiceFlavour");
    res.set("LegalCopyright", "Copyright (c) 2025 Moulberry, 2026 PineappleJuiceFlavour. MIT licence.");
    res.set("OriginalFilename", "Flux.exe");
    res.set("InternalName", "Flux");
    res.set("Comments", "https://github.com/PineappleJuiceFlavour/flux");
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
    res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
    res.compile().unwrap();
}

#[cfg(not(windows))]
fn windows() {}
