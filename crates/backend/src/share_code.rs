//! Instance share codes: one line of text that rebuilds an instance's mods, resource packs and
//! shaders on a friend's PC. The code only holds the Modrinth and CurseForge release IDs plus the
//! Minecraft version and loader; the friend's Flux downloads every file from Modrinth/CurseForge.
//! Files that are on neither site (and disabled ones) can't be shared and are listed instead.
//!
//! Format: `FLUX1-` + base64url(zlib(payload)), where the payload is
//! `version u8 · name · minecraft version · loader u8 · loader version` (strings are u8 length +
//! UTF-8), then a varint count of Modrinth entries `(folder u8, version id string, file index u8)`
//! and a varint count of CurseForge entries `(folder u8, project varint, file varint)`.

use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
};

use base64::Engine;
use bridge::{
    instance::{InstanceID, ModpackFileSource},
    message::{ExportCurseforgeOptions, ExportModrinthOptions, ExportOptions},
    modal_action::{ModalAction, ProgressTrackerFinishType},
    safe_path::SafePath,
};
use indexmap::IndexMap;
use schema::{
    curseforge::{CurseforgeFingerprintRequest, CurseforgeGetFilesRequest},
    loader::Loader,
    modification::ModrinthModpackFileDownload,
    modrinth::{ModrinthHashes, ModrinthVersionsFromHashesRequest},
    mrpack::ModrinthIndexJson,
};
use sha2::{Digest, Sha512};

use crate::{
    BackendState,
    export::{self, ExportInstanceData},
    metadata::items::{CurseforgeFingerprintMetadataItem, CurseforgeGetFilesMetadataItem, ModrinthVersionMetadataItem, ModrinthVersionsFromHashesMetadataItem},
};

pub const PREFIX: &str = "FLUX1-";
/// File index meaning "the version's primary file" (for modpack entries known only by link).
const ANY_FILE: u8 = 255;
const FORMAT: u8 = 1;
const FOLDERS: [&str; 3] = ["mods", "resourcepacks", "shaderpacks"];

#[derive(Debug, Clone, PartialEq)]
pub struct ShareCode {
    pub name: String,
    pub minecraft_version: String,
    pub loader: Loader,
    pub loader_version: String,
    /// (folder, Modrinth version id, index of the file within that version)
    pub modrinth: Vec<(u8, String, u8)>,
    /// (folder, CurseForge project id, file id)
    pub curseforge: Vec<(u8, u32, u32)>,
}

impl Default for ShareCode {
    fn default() -> Self {
        Self {
            name: String::new(),
            minecraft_version: String::new(),
            loader: Loader::Vanilla,
            loader_version: String::new(),
            modrinth: Vec::new(),
            curseforge: Vec::new(),
        }
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    // Cut at a character boundary so the length fits in one byte.
    let mut end = s.len().min(255);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    out.push(end as u8);
    out.extend_from_slice(&s.as_bytes()[..end]);
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let (&b, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(b)
    }

    fn str(&mut self) -> Option<String> {
        let len = self.u8()? as usize;
        if self.0.len() < len {
            return None;
        }
        let (s, rest) = self.0.split_at(len);
        self.0 = rest;
        String::from_utf8(s.to_vec()).ok()
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
}

fn loader_byte(loader: Loader) -> u8 {
    match loader {
        Loader::Vanilla => 0,
        Loader::Fabric => 1,
        Loader::Forge => 2,
        Loader::NeoForge => 3,
    }
}

pub fn encode(code: &ShareCode) -> String {
    let mut raw = vec![FORMAT];
    put_str(&mut raw, &code.name);
    put_str(&mut raw, &code.minecraft_version);
    raw.push(loader_byte(code.loader));
    put_str(&mut raw, &code.loader_version);
    put_varint(&mut raw, code.modrinth.len() as u64);
    for (folder, version, file) in &code.modrinth {
        raw.push(*folder);
        put_str(&mut raw, version);
        raw.push(*file);
    }
    put_varint(&mut raw, code.curseforge.len() as u64);
    for (folder, project, file) in &code.curseforge {
        raw.push(*folder);
        put_varint(&mut raw, *project as u64);
        put_varint(&mut raw, *file as u64);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    _ = z.write_all(&raw);
    let packed = z.finish().unwrap_or(raw);
    format!("{PREFIX}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(packed))
}

pub fn decode(text: &str) -> Result<ShareCode, &'static str> {
    // Chat apps sometimes wrap or space long codes: ignore all whitespace.
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let body = text.strip_prefix(PREFIX).ok_or("That isn't a Flux share code (they start with FLUX1-)")?;
    let packed = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(body.trim_end_matches('='))
        .map_err(|_| "The code is damaged: check it was copied completely")?;
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&packed[..]).take(4 << 20).read_to_end(&mut raw)
        .map_err(|_| "The code is damaged: check it was copied completely")?;
    let bad = "The code is damaged: check it was copied completely";
    let mut r = Reader(&raw);
    if r.u8().ok_or(bad)? != FORMAT {
        return Err("This code was made by a newer Flux: update Flux to import it");
    }
    let mut code = ShareCode {
        name: r.str().ok_or(bad)?,
        minecraft_version: r.str().ok_or(bad)?,
        ..Default::default()
    };
    code.loader = match r.u8().ok_or(bad)? {
        1 => Loader::Fabric,
        2 => Loader::Forge,
        3 => Loader::NeoForge,
        _ => Loader::Vanilla,
    };
    code.loader_version = r.str().ok_or(bad)?;
    for _ in 0..r.varint().ok_or(bad)?.min(10_000) {
        code.modrinth.push((r.u8().ok_or(bad)?, r.str().ok_or(bad)?, r.u8().ok_or(bad)?));
    }
    for _ in 0..r.varint().ok_or(bad)?.min(10_000) {
        let folder = r.u8().ok_or(bad)?;
        let project = r.varint().ok_or(bad)? as u32;
        let file = r.varint().ok_or(bad)? as u32;
        code.curseforge.push((folder, project, file));
    }
    if code.minecraft_version.is_empty() {
        return Err(bad);
    }
    Ok(code)
}

/// What a finished share code covers.
pub struct Shared {
    pub code: String,
    pub included: usize,
    /// File names that couldn't go in the code (not on Modrinth or CurseForge).
    pub skipped: Vec<String>,
}

/// `https://cdn.modrinth.com/data/<project>/versions/<version>/<file>` -> the version id.
fn modrinth_version_from_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://cdn.modrinth.com/data/")?;
    let mut parts = rest.split('/');
    let (_project, keyword, version) = (parts.next()?, parts.next()?, parts.next()?);
    (keyword == "versions" && !version.is_empty()).then(|| version.to_string())
}

fn folder_of(rel: &SafePath) -> Option<u8> {
    if export::is_mod_file(rel) {
        Some(0)
    } else if export::is_resourcepack_file(rel) {
        Some(1)
    } else if export::is_shaderpack_file(rel) {
        Some(2)
    } else {
        None
    }
}

pub async fn create(backend: &Arc<BackendState>, id: InstanceID, modal_action: &ModalAction) -> Result<Shared, String> {
    let (instance, name) = {
        let mut instance_state = backend.instance_state.write();
        let instance = instance_state.instances.get_mut(id).ok_or("Unknown instance")?;
        (ExportInstanceData {
            root_path: Arc::clone(&instance.root_path),
            dot_minecraft_path: Arc::clone(&instance.dot_minecraft_path),
            configuration: instance.configuration.get().clone(),
            sync_targets: backend.config.lock().get().sync_targets.clone(),
        }, instance.name.to_string())
    };

    let options = ExportOptions {
        include_saves: false,
        include_mods: true,
        include_resourcepacks: true,
        include_shaders: true,
        include_configs: false,
        include_screenshots: false,
        include_backups: false,
        include_logs: false,
        include_cache: false,
        include_synced: true,
        modrinth: ExportModrinthOptions { name: "".into(), version: "".into(), summary: None },
        curseforge: ExportCurseforgeOptions { name: "".into(), version: "".into(), author: None, recommended_ram: None },
    };
    let tracker = modal_action.push_tracker("Finding your mods".into());
    let files = export::collect_files(&instance.dot_minecraft_path, &instance.dot_minecraft_path, &options,
        &instance.sync_targets, &backend.directories.synced_dir, modal_action)
        .map_err(|_| "Couldn't read the instance's files".to_string())?;

    let mut code = ShareCode {
        name: name.clone(),
        minecraft_version: instance.configuration.minecraft_version.to_string(),
        loader: instance.configuration.loader,
        loader_version: instance.determine_loader_version(backend).await.map(|v| v.to_string()).unwrap_or_default(),
        ..Default::default()
    };

    // Files to look up by hash: (folder, file name, sha512, CurseForge fingerprint).
    let mut hashed: Vec<(u8, String, String, u32)> = Vec::new();
    // Modpack files from other sites, known only by sha1: (folder, file name, sha1).
    let mut by_sha1: Vec<(u8, String, String)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    let enabled: Vec<_> = files.iter().filter(|f| f.enabled).collect();
    tracker.set_total(enabled.len());
    for file in enabled {
        if modal_action.has_requested_cancel() {
            return Err("Cancelled".into());
        }
        tracker.add_count(1);
        let file_name = file.rel.file_name().unwrap_or_default().to_string();
        if let Some(folder) = folder_of(&file.rel) {
            let data = std::fs::read(&file.abs).map_err(|e| e.to_string())?;
            hashed.push((folder, file_name, hex::encode(Sha512::digest(&data)), export::murmur2_32(&data)));
            continue;
        }
        // A modpack installed in the instance (imported packs live here as one file): share what's in it.
        if !file.rel.starts_with("mods") {
            continue;
        }
        let summary = backend.mod_metadata_manager.get_path(&file.abs);
        let Some(pack_files) = summary.extra.modpack_files() else { continue };
        for pack_file in pack_files.iter().filter(|f| !f.default_disabled) {
            let Some(path) = pack_file.path() else { continue };
            let Some(folder) = folder_of(&path) else { continue };
            let file_name = path.file_name().unwrap_or_default().to_string();
            match &pack_file.source {
                ModpackFileSource::DownloadUrl { url, .. } => match modrinth_version_from_url(url) {
                    Some(version) => code.modrinth.push((folder, version, ANY_FILE)),
                    None => by_sha1.push((folder, file_name, hex::encode(pack_file.hash))),
                },
                ModpackFileSource::DownloadCurseforge { file_id } => code.curseforge.push((folder, 0, *file_id)),
                ModpackFileSource::Builtin { bytes } => {
                    hashed.push((folder, file_name, hex::encode(Sha512::digest(bytes)), export::murmur2_32(bytes)));
                },
            }
        }
    }
    tracker.set_finished(ProgressTrackerFinishType::Normal);

    let lookup = modal_action.push_tracker("Looking them up on Modrinth and CurseForge".into());
    if !by_sha1.is_empty() {
        let request = ModrinthVersionsFromHashesRequest {
            hashes: by_sha1.iter().map(|h| Arc::<str>::from(h.2.as_str())).collect::<Vec<_>>().into(),
            algorithm: "sha1".into(),
        };
        let found = backend.meta.fetch(ModrinthVersionsFromHashesMetadataItem(&request)).await.ok();
        for (folder, file_name, sha1) in by_sha1 {
            let hit = found.as_ref().and_then(|f| f.0.get(sha1.as_str())).and_then(|v| v.as_ref()).and_then(|version| {
                let index = version.files.iter().position(|f| f.hashes.sha1.as_ref() == sha1.as_str())?;
                Some((version.id.to_string(), index.min(254) as u8))
            });
            match hit {
                Some((version, index)) => code.modrinth.push((folder, version, index)),
                None => skipped.push(file_name),
            }
        }
    }

    // Modrinth first: the version that has this exact file.
    let lookup = modal_action.push_tracker("Looking them up on Modrinth and CurseForge".into());
    let mut left = Vec::new();
    if !hashed.is_empty() {
        let request = ModrinthVersionsFromHashesRequest {
            hashes: hashed.iter().map(|h| Arc::<str>::from(h.2.as_str())).collect::<Vec<_>>().into(),
            algorithm: "sha512".into(),
        };
        let found = backend.meta.fetch(ModrinthVersionsFromHashesMetadataItem(&request)).await
            .map_err(|e| format!("Couldn't reach Modrinth: {e}"))?;
        for (folder, file_name, sha512, murmur) in hashed {
            let hit = found.0.get(sha512.as_str()).and_then(|v| v.as_ref()).and_then(|version| {
                let index = version.files.iter().position(|f| f.hashes.sha512.as_deref() == Some(sha512.as_str()))?;
                Some((version.id.to_string(), index.min(255) as u8))
            });
            match hit {
                Some((version, index)) => code.modrinth.push((folder, version, index)),
                None => left.push((folder, file_name, murmur)),
            }
        }
    }

    // Then CurseForge, by its file fingerprint.
    if !left.is_empty() {
        let request = CurseforgeFingerprintRequest { fingerprints: left.iter().map(|l| l.2).collect() };
        if let Ok(found) = backend.meta.fetch(CurseforgeFingerprintMetadataItem(&request)).await {
            let by_print: HashMap<u32, (u32, u32)> = found.data.exact_matches.iter()
                .map(|m| (m.file.file_fingerprint, (m.file.mod_id, m.file.id))).collect();
            left.retain(|(folder, _, murmur)| match by_print.get(murmur) {
                Some(&(project, file)) => {
                    code.curseforge.push((*folder, project, file));
                    false
                },
                None => true,
            });
        }
    }
    lookup.set_finished(ProgressTrackerFinishType::Normal);

    skipped.extend(left.into_iter().map(|(_, name, _)| name));
    Ok(Shared { included: code.modrinth.len() + code.curseforge.len(), code: encode(&code), skipped })
}

/// Turn a code into a temporary .mrpack (downloads only, no overrides) for Flux's modpack import.
/// Returns its path, and the files that couldn't be fetched.
pub async fn to_mrpack(backend: &Arc<BackendState>, text: &str, modal_action: &ModalAction) -> Result<(PathBuf, Vec<String>), String> {
    let code = decode(text).map_err(str::to_string)?;
    let tracker = modal_action.push_tracker(format!("Looking up {} files", code.modrinth.len() + code.curseforge.len()).into());
    tracker.set_total(code.modrinth.len() + code.curseforge.len());
    let mut files = Vec::new();
    let mut missing = Vec::new();

    let lookups = code.modrinth.iter().map(|(folder, version, index)| {
        let backend = Arc::clone(backend);
        let version: Arc<str> = version.as_str().into();
        async move { (*folder, *index, backend.meta.fetch(ModrinthVersionMetadataItem(version.clone())).await, version) }
    });
    for (folder, index, result, version) in futures::future::join_all(lookups).await {
        tracker.add_count(1);
        let file = result.ok().and_then(|v| {
            v.files.get(index as usize).or_else(|| v.files.iter().find(|f| f.primary)).or_else(|| v.files.first()).cloned()
        });
        match file {
            Some(f) => files.push(ModrinthModpackFileDownload {
                path: format!("{}/{}", FOLDERS[(folder as usize).min(2)], f.filename).into(),
                hashes: f.hashes.clone(),
                env: None,
                downloads: Arc::from([f.url.clone()]),
                file_size: f.size,
            }),
            None => missing.push(format!("Modrinth version {version}")),
        }
    }

    if !code.curseforge.is_empty() {
        let request = CurseforgeGetFilesRequest { file_ids: code.curseforge.iter().map(|c| c.2).collect() };
        let found = backend.meta.fetch(CurseforgeGetFilesMetadataItem(&request)).await.ok();
        for (folder, _project, file_id) in &code.curseforge {
            tracker.add_count(1);
            let file = found.as_ref().and_then(|r| r.data.iter().find(|f| f.id == *file_id));
            let sha1 = file.and_then(|f| f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone()));
            match (file, sha1) {
                // Some authors turn off downloads from other apps: those can't be fetched for you.
                (Some(f), Some(sha1)) if f.download_url.is_some() => files.push(ModrinthModpackFileDownload {
                    path: format!("{}/{}", FOLDERS[(*folder as usize).min(2)], f.file_name).into(),
                    hashes: ModrinthHashes { sha1, sha512: None },
                    env: None,
                    downloads: Arc::from([f.download_url.clone().unwrap()]),
                    file_size: f.file_length as usize,
                }),
                (Some(f), _) => missing.push(f.file_name.to_string()),
                (None, _) => missing.push(format!("CurseForge file {file_id}")),
            }
        }
    }
    tracker.set_finished(ProgressTrackerFinishType::Normal);

    let mut dependencies: IndexMap<Arc<str>, Arc<str>> = IndexMap::new();
    dependencies.insert("minecraft".into(), code.minecraft_version.as_str().into());
    let loader_key = match code.loader {
        Loader::Fabric => Some("fabric-loader"),
        Loader::Forge => Some("forge"),
        Loader::NeoForge => Some("neoforge"),
        Loader::Vanilla => None,
    };
    if let Some(key) = loader_key {
        let version = if code.loader_version.is_empty() { "*" } else { code.loader_version.as_str() };
        dependencies.insert(key.into(), version.into());
    }
    let name = if code.name.trim().is_empty() { "Shared instance".to_string() } else { code.name.clone() };
    let index = ModrinthIndexJson {
        format_version: 1,
        game: "minecraft".into(),
        version_id: "flux-share".into(),
        name: name.into(),
        summary: Some("Shared with a Flux code".into()),
        files: files.into(),
        dependencies,
        authors: None,
        author: None,
    };
    let json = serde_json::to_vec(&index).map_err(|e| e.to_string())?;

    std::fs::create_dir_all(&backend.directories.temp_dir).map_err(|e| e.to_string())?;
    let path = backend.directories.temp_dir.join(format!("share-{:016x}.mrpack", rand::random::<u64>()));
    let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("modrinth.index.json", zip::write::SimpleFileOptions::default()).map_err(|e| e.to_string())?;
    zip.write_all(&json).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok((path, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let code = ShareCode {
            name: "Skycraft ✨".into(),
            minecraft_version: "1.21.1".into(),
            loader: Loader::Fabric,
            loader_version: "0.16.9".into(),
            modrinth: vec![(0, "AANobbMI".into(), 0), (1, "kHZ4yzpB".into(), 1)],
            curseforge: vec![(0, 238222, 5101366)],
        };
        let text = encode(&code);
        assert!(text.starts_with(PREFIX));
        assert_eq!(decode(&text).unwrap(), code);
        // Spaces and line breaks from chat apps don't matter.
        let spaced: String = text.chars().enumerate().flat_map(|(i, c)| if i % 20 == 19 { vec![c, '\n'] } else { vec![c] }).collect();
        assert_eq!(decode(&format!("  {spaced} ")).unwrap(), code);
        assert!(decode("hello").is_err());
        assert!(decode(&format!("{PREFIX}AAAA")).is_err());
    }

    #[test]
    fn stays_short() {
        let code = ShareCode {
            name: "Big pack".into(),
            minecraft_version: "1.20.1".into(),
            loader: Loader::Forge,
            loader_version: "47.3.0".into(),
            modrinth: (0..100).map(|i| (0, format!("Ab{i:06}"), 0)).collect(),
            curseforge: vec![],
        };
        assert!(encode(&code).len() < 1400, "{}", encode(&code).len());
    }
}
