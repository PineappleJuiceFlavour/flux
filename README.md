# Flux

A free launcher for **Minecraft: Java Edition**, with a game library built in.

Flux is based on [Pandora Launcher](https://github.com/Moulberry/PandoraLauncher) by Moulberry (MIT licence) and keeps all of its features.

## Minecraft
- Sign in with your Microsoft account (the official Microsoft / Xbox / Minecraft sign-in; Flux never sees your password)
- Instances for any Minecraft version, with Fabric, Forge, NeoForge and Quilt
- Install mods, resource packs, shaders and modpacks from Modrinth and CurseForge
- **Max FPS** setup: a ready-made instance with performance mods (Sodium, Lithium, FerriteCore and more)
- Skins and capes, with a preview of any player's skin before you apply it
- Syncing, quick play, and importing from other launchers

## Games
- Add your own games (Windows games and your own console game backups); Flux finds each game's art
- Built-in support for emulators (Xenia, RPCS3, PCSX2, DuckStation and more), with per-game settings, patches and mods
- Use a PS5 / PS4 controller as an Xbox controller

Flux does not include any games, BIOS or firmware.

## Download
Get the latest version from [Releases](../../releases/latest). Run `Flux.exe`: nothing to install.
Windows may say "Windows protected your PC": click **More info** then **Run anyway**.

Flux updates itself: it checks for new versions when it starts and asks before updating.

## Privacy
Flux has no servers of its own, no accounts of its own, no tracking and no analytics.

**What Flux keeps on your PC**
- Sign-ins (Microsoft/Minecraft, chat) and your SteamGridDB key: in Windows Credential Manager, under `FluxLauncher_*` names.
- Settings, your game list, art, mods and save backups: in `%LOCALAPPDATA%\FluxLauncher`.

**Who Flux talks to, and when**
- **Every start:**
  - GitHub, to check for a Flux update (only the address and Flux's version are sent);
  - Mojang, for the list of Minecraft versions;
  - for games you've added that don't have details yet: Steam, libretro's thumbnail archive, Wikipedia and Wikidata. These services see which game titles you look up.
- **When you do something:**
  - signing in: Microsoft, Xbox Live and Mojang;
  - Minecraft, Java and mod downloads: Mojang, Modrinth, CurseForge and the Fabric/Forge/NeoForge sites;
  - emulators and patches: the emulators' GitHub releases, rpcs3.net and Xenia's patch repo;
  - game art from SteamGridDB, only with your own key;
  - uploading a log: mclo.gs.
- **Chat (only if you connect it):**
  - Flux chat runs on Matrix, matrix.org by default. Messages sent from Flux are not end-to-end encrypted.
  - PlayStation chat uses the private API of Sony's PlayStation App. Flux warns you before you connect it.
  - Xbox chat uses Xbox Live with your Microsoft sign-in.
  - A connected network reconnects when Flux starts and refreshes your friends every 90 seconds.
- **Privacy mode** (Settings > General > Privacy) hides names, emails, messages and your Windows user name on screen, e.g. for streaming. It doesn't change what Flux sends.

## Checking a download
Releases are built by GitHub Actions from this repo, and each one has signed build provenance. With the [GitHub CLI](https://cli.github.com/):

```
gh attestation verify Flux.exe -R PineappleJuiceFlavour/flux
```

The release notes also list each `Flux.exe`'s SHA-256. Flux's updater checks every update's checksum and signature (including its version) before installing it.

## Licence
MIT. See [LICENSE](LICENSE). Not affiliated with Mojang or Microsoft.
