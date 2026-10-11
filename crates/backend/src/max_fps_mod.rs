//! The Flux companion mod (built from `mod/` in the Flux repo), bundled into the launcher
//! binary and dropped into Max FPS instances for the Minecraft versions it is built for.

/// (Minecraft version, jar file name, jar bytes)
const BUNDLED: &[(&str, &str, &[u8])] = &[
    ("26.3", "flux-0.1.0.jar", include_bytes!("../assets/flux-0.1.0.jar")),
];

pub fn bundled_jar(minecraft_version: &str) -> Option<(&'static str, &'static [u8])> {
    BUNDLED.iter()
        .find(|(version, _, _)| *version == minecraft_version)
        .map(|(_, filename, bytes)| (*filename, *bytes))
}
