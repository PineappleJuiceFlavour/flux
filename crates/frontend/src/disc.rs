//! Reading just enough of game discs and dumps to tell consoles apart and find a game's title:
//! ISO 9660 (PS1/PS2/PSP/PS3, plain `.iso` and raw `.bin`), Xbox XDVDFS, PlayStation `PARAM.SFO`.

use std::{fs::File, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}};

pub fn read_at(file: &mut File, offset: u64, len: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0; len];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

// ------------------------------------------------------------------------------------------ ISO 9660

/// An ISO 9660 image: 2048-byte sectors (`.iso`) or raw 2352-byte CD sectors (`.bin`).
pub struct Iso {
    file: File,
    sector: u64,
    header: u64,
    pub system_id: String,
    pub volume_id: String,
    root: (u32, u32), // (lba, size)
}

const RAW_SYNC: [u8; 12] = [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0];

impl Iso {
    pub fn open(path: &Path) -> Option<Iso> {
        let mut file = File::open(path).ok()?;
        // Try plain 2048-byte sectors, then raw CD sectors (mode 1 data at +16, mode 2 at +24).
        for (sector, header) in [(2048u64, 0u64), (2352, 24), (2352, 16)] {
            if sector == 2352 && read_at(&mut file, 0, 12).as_deref() != Some(&RAW_SYNC[..]) {
                continue;
            }
            let Some(pvd) = read_at(&mut file, 16 * sector + header, 2048) else { continue };
            if pvd[0] == 1 && &pvd[1..6] == b"CD001" {
                let text = |range: std::ops::Range<usize>| String::from_utf8_lossy(&pvd[range]).trim().to_string();
                let root = (le32(&pvd, 156 + 2), le32(&pvd, 156 + 10));
                return Some(Iso { file, sector, header, system_id: text(8..40), volume_id: text(40..72), root });
            }
        }
        None
    }

    fn read_extent(&mut self, lba: u32, size: u32) -> Option<Vec<u8>> {
        let size = size.min(1 << 20) as usize; // directories and SYSTEM.CNF are small
        let mut out = Vec::with_capacity(size);
        let mut lba = lba as u64;
        while out.len() < size {
            let chunk = read_at(&mut self.file, lba * self.sector + self.header, 2048)?;
            out.extend_from_slice(&chunk);
            lba += 1;
        }
        out.truncate(size);
        Some(out)
    }

    /// (name, lba, size, is_dir) of the entries in a directory.
    fn list(&mut self, dir: (u32, u32)) -> Vec<(String, u32, u32, bool)> {
        let Some(data) = self.read_extent(dir.0, dir.1) else { return vec![] };
        let mut entries = vec![];
        let mut at = 0;
        while at < data.len() {
            let len = data[at] as usize;
            if len == 0 {
                // Records don't cross sectors: skip to the next one.
                at = (at / 2048 + 1) * 2048;
                continue;
            }
            if at + 33 > data.len() || at + len > data.len() {
                break;
            }
            let name_len = data[at + 32] as usize;
            let raw = &data[at + 33..(at + 33 + name_len).min(data.len())];
            if raw != [0] && raw != [1] {
                let name = String::from_utf8_lossy(raw).split(';').next().unwrap_or("").to_uppercase();
                entries.push((name, le32(&data, at + 2), le32(&data, at + 10), data[at + 25] & 2 != 0));
            }
            at += len;
        }
        entries
    }

    pub fn root_names(&mut self) -> Vec<String> {
        let root = self.root;
        self.list(root).into_iter().map(|e| e.0).collect()
    }

    /// A file by path from the root, e.g. "SYSTEM.CNF" or "PSP_GAME/PARAM.SFO".
    pub fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
        let mut dir = self.root;
        let parts: Vec<String> = path.split('/').map(|p| p.to_uppercase()).collect();
        for (i, part) in parts.iter().enumerate() {
            let entry = self.list(dir).into_iter().find(|e| &e.0 == part)?;
            if i + 1 == parts.len() {
                return if entry.3 { None } else { self.read_extent(entry.1, entry.2) };
            }
            dir = (entry.1, entry.2);
        }
        None
    }
}

// ----------------------------------------------------------------------------------------- PARAM.SFO

/// A string value from a PlayStation PARAM.SFO (e.g. "TITLE", "TITLE_ID").
pub fn sfo_value(sfo: &[u8], key: &str) -> Option<String> {
    if sfo.len() < 20 || &sfo[0..4] != b"\0PSF" {
        return None;
    }
    let keys = le32(sfo, 8) as usize;
    let data = le32(sfo, 12) as usize;
    let count = le32(sfo, 16) as usize;
    for i in 0..count {
        let entry = 20 + i * 16;
        if entry + 16 > sfo.len() {
            break;
        }
        let key_at = keys + le16(sfo, entry) as usize;
        let key_end = sfo[key_at..].iter().position(|b| *b == 0).map(|p| key_at + p)?;
        if &sfo[key_at..key_end] != key.as_bytes() {
            continue;
        }
        let len = le32(sfo, entry + 4) as usize;
        let at = data + le32(sfo, entry + 12) as usize;
        let value = sfo.get(at..at + len)?;
        let value = String::from_utf8_lossy(value).trim_end_matches('\0').trim().replace('\n', " ");
        return (!value.is_empty()).then_some(value);
    }
    None
}

pub fn sfo_title(path: &Path) -> Option<String> {
    sfo_value(&std::fs::read(path).ok()?, "TITLE")
}

// ------------------------------------------------------------------------------------------- XDVDFS

const XDVDFS_MAGIC: &[u8] = b"MICROSOFT*XBOX*MEDIA";
/// Game partition offsets: raw XDVDFS (original Xbox .xiso / redumps of the data partition), XGD1-3.
const XBOX_PARTITIONS: &[u64] = &[0, 0x1830_0000, 0x0FD9_0000, 0x0208_0000];

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum XboxDisc {
    Original,
    X360,
}

/// Is this an Xbox disc image, and for which Xbox? Decided by the boot file in the root directory.
pub fn xbox_disc(path: &Path) -> Option<XboxDisc> {
    let mut file = File::open(path).ok()?;
    for base in XBOX_PARTITIONS {
        let Some(volume) = read_at(&mut file, base + 0x10000, 0x20) else { continue };
        if &volume[0..20] != XDVDFS_MAGIC {
            continue;
        }
        let (root_sector, root_size) = (le32(&volume, 20) as u64, le32(&volume, 24).min(1 << 20) as usize);
        let root = read_at(&mut file, base + root_sector * 2048, root_size).unwrap_or_default();
        let lower = String::from_utf8_lossy(&root).to_lowercase();
        if lower.contains("default.xbe") && !lower.contains("default.xex") {
            return Some(XboxDisc::Original);
        }
        return Some(XboxDisc::X360);
    }
    None
}

/// An Xbox / Xbox 360 XDVDFS disc image, opened at its game partition.
pub struct Xdvdfs {
    file: File,
    base: u64,
    pub root: (u32, u32), // (sector, size)
}

#[derive(Debug, Clone)]
pub struct XEntry {
    pub name: String,
    pub sector: u32,
    pub size: u32,
    pub dir: bool,
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

impl Xdvdfs {
    pub fn open(path: &Path) -> Option<Xdvdfs> {
        let mut file = File::open(path).ok()?;
        for &base in XBOX_PARTITIONS {
            let Some(volume) = read_at(&mut file, base + 0x10000, 0x20) else { continue };
            if &volume[0..20] == XDVDFS_MAGIC {
                let root = (le32(&volume, 20), le32(&volume, 24));
                return Some(Xdvdfs { file, base, root });
            }
        }
        None
    }

    fn read(&mut self, sector: u32, len: usize) -> Option<Vec<u8>> {
        read_at(&mut self.file, self.base + sector as u64 * 2048, len)
    }

    /// The entries of a directory table (a binary tree of records).
    pub fn list(&mut self, dir: (u32, u32)) -> Vec<XEntry> {
        if dir.1 == 0 {
            return vec![];
        }
        let Some(data) = self.read(dir.0, dir.1.min(16 << 20) as usize) else { return vec![] };
        let mut out = Vec::new();
        let mut stack = vec![0usize];
        let mut seen = std::collections::HashSet::new();
        while let Some(at) = stack.pop() {
            if !seen.insert(at) || at + 14 > data.len() || data[at..at + 4] == [0xFF; 4] {
                continue;
            }
            let (left, right) = (le16(&data, at) as usize, le16(&data, at + 2) as usize);
            let name_len = data[at + 13] as usize;
            let Some(name) = data.get(at + 14..at + 14 + name_len) else { continue };
            out.push(XEntry {
                name: String::from_utf8_lossy(name).to_string(),
                sector: le32(&data, at + 4),
                size: le32(&data, at + 8),
                dir: data[at + 12] & 0x10 != 0,
            });
            for child in [left, right] {
                if child != 0 && child != 0xFFFF {
                    stack.push(child * 4);
                }
            }
        }
        out
    }

    /// A file by path from the root ("default.xex", "media/x.big"), case-insensitive.
    pub fn find(&mut self, path: &str) -> Option<XEntry> {
        let mut dir = self.root;
        let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            let entry = self.list(dir).into_iter().find(|e| e.name.eq_ignore_ascii_case(part))?;
            if i + 1 == parts.len() {
                return Some(entry);
            }
            dir = (entry.sector, entry.size);
        }
        None
    }

    pub fn read_head(&mut self, entry: &XEntry, max: usize) -> Option<Vec<u8>> {
        self.read(entry.sector, (entry.size as usize).min(max))
    }

    /// Every file on the disc: (path with '/', entry).
    pub fn files(&mut self) -> Vec<(String, XEntry)> {
        let mut out = Vec::new();
        let mut dirs = vec![(String::new(), self.root)];
        while let Some((prefix, dir)) = dirs.pop() {
            for entry in self.list(dir) {
                let path = if prefix.is_empty() { entry.name.clone() } else { format!("{prefix}/{}", entry.name) };
                if entry.dir {
                    dirs.push((path, (entry.sector, entry.size)));
                } else {
                    out.push((path, entry));
                }
            }
        }
        out
    }

    /// Copy every file to `dest`, reporting (bytes done, bytes total). Empty folders are skipped.
    pub fn extract(&mut self, dest: &Path, progress: &mut dyn FnMut(u64, u64) -> bool) -> Result<(), String> {
        let files = self.files();
        let total: u64 = files.iter().map(|(_, e)| e.size as u64).sum();
        let mut done = 0u64;
        for (path, entry) in files {
            let out = dest.join(path.replace('/', "\\"));
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
            }
            let mut writer = std::io::BufWriter::new(File::create(&out).map_err(|e| format!("Couldn't create {}: {e}", out.display()))?);
            self.file.seek(SeekFrom::Start(self.base + entry.sector as u64 * 2048)).map_err(|e| e.to_string())?;
            let mut left = entry.size as u64;
            let mut buf = vec![0u8; 4 << 20];
            while left > 0 {
                let n = left.min(buf.len() as u64) as usize;
                self.file.read_exact(&mut buf[..n]).map_err(|e| format!("Couldn't read the disc image: {e}"))?;
                std::io::Write::write_all(&mut writer, &buf[..n]).map_err(|e| format!("Couldn't write {}: {e}", out.display()))?;
                left -= n as u64;
                done += n as u64;
                if !progress(done, total) {
                    return Err("Stopped".into());
                }
            }
            std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// The title ID in an Xbox 360 executable's header (XEX2 execution info), e.g. 0x454108E6.
pub fn xex_title_id(head: &[u8]) -> Option<u32> {
    if head.get(0..4)? != b"XEX2" {
        return None;
    }
    let count = be32(head, 0x14).min(64) as usize;
    for i in 0..count {
        let at = 0x18 + i * 8;
        if at + 8 > head.len() {
            break;
        }
        if be32(head, at) == 0x0004_0006 {
            let info = be32(head, at + 4) as usize;
            return (info + 16 <= head.len()).then(|| be32(head, info + 12));
        }
    }
    None
}

/// An Xbox 360 game's title ID as 8 hex digits, from a disc image, a game folder or its default.xex.
pub fn xbox360_title_id(path: &Path) -> Option<String> {
    let file_head = |xex: &Path| -> Option<Vec<u8>> {
        let mut f = File::open(xex).ok()?;
        let len = f.metadata().ok()?.len().min(0x10000) as usize;
        read_at(&mut f, 0, len)
    };
    let head = if path.is_dir() {
        file_head(&child(path, "default.xex")?)?
    } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("xex")) {
        file_head(path)?
    } else {
        let mut disc = Xdvdfs::open(path)?;
        let xex = disc.find("default.xex")?;
        disc.read_head(&xex, 0x10000)?
    };
    xex_title_id(&head).map(|id| format!("{id:08X}"))
}

// ------------------------------------------------------------------------------------------ helpers

pub fn starts_with(path: &Path, magic: &[u8]) -> bool {
    File::open(path).ok().and_then(|mut f| read_at(&mut f, 0, magic.len())).as_deref() == Some(magic)
}

pub fn has_at(path: &Path, offset: u64, magic: &[u8]) -> bool {
    File::open(path).ok().and_then(|mut f| read_at(&mut f, offset, magic.len())).as_deref() == Some(magic)
}

/// The first data track a .cue sheet points at.
pub fn cue_bin(cue: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(cue).ok()?;
    let line = text.lines().find(|l| l.trim_start().to_uppercase().starts_with("FILE"))?;
    let name = line.split('"').nth(1)?;
    Some(cue.parent()?.join(name))
}

/// A child of `dir` matched case-insensitively.
pub fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path())
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sfo_title() {
        // Minimal SFO: one key "TITLE" -> "Hello".
        let mut sfo = Vec::new();
        sfo.extend_from_slice(b"\0PSF");
        sfo.extend_from_slice(&0x0101u32.to_le_bytes());
        let key_table = 20 + 16;
        let data_table = key_table + 8;
        sfo.extend_from_slice(&(key_table as u32).to_le_bytes());
        sfo.extend_from_slice(&(data_table as u32).to_le_bytes());
        sfo.extend_from_slice(&1u32.to_le_bytes());
        sfo.extend_from_slice(&0u16.to_le_bytes()); // key offset
        sfo.extend_from_slice(&0x0204u16.to_le_bytes()); // utf8
        sfo.extend_from_slice(&6u32.to_le_bytes()); // len
        sfo.extend_from_slice(&8u32.to_le_bytes()); // max
        sfo.extend_from_slice(&0u32.to_le_bytes()); // data offset
        sfo.extend_from_slice(b"TITLE\0\0\0");
        sfo.extend_from_slice(b"Hello\0\0\0");
        assert_eq!(sfo_value(&sfo, "TITLE").as_deref(), Some("Hello"));
        assert_eq!(sfo_value(&sfo, "NOPE"), None);
    }

    #[test]
    fn xex_header_title_id() {
        let mut head = vec![0u8; 0x200];
        head[0..4].copy_from_slice(b"XEX2");
        head[0x14..0x18].copy_from_slice(&1u32.to_be_bytes());
        head[0x18..0x1C].copy_from_slice(&0x0004_0006u32.to_be_bytes());
        head[0x1C..0x20].copy_from_slice(&0x100u32.to_be_bytes());
        head[0x10C..0x110].copy_from_slice(&0x454108E6u32.to_be_bytes());
        assert_eq!(xex_title_id(&head), Some(0x454108E6));
    }

    /// FLUX_XDVDFS_TEST=<xbox 360 iso>: title ID and file list of a real disc (read-only).
    #[test]
    fn real_xbox_disc() {
        let Ok(path) = std::env::var("FLUX_XDVDFS_TEST") else { return };
        println!("title id: {:?}", xbox360_title_id(Path::new(&path)));
        let mut disc = Xdvdfs::open(Path::new(&path)).unwrap();
        let files = disc.files();
        let total: u64 = files.iter().map(|(_, e)| e.size as u64).sum();
        println!("{} files, {:.2} GB", files.len(), total as f64 / 1e9);
        for (p, e) in files.iter().take(12) {
            println!("  {p} {}", e.size);
        }
    }
}
