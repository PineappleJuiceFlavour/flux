"""Build a Flux release: the share folder, the signed update and its manifest.

    uv run --with cryptography python release.py           build the version in flux-release.json
    uv run --with cryptography python release.py 1.0.1     set the version first, then build

What it does:
1. Makes Flux's update-signing key the first time (kept in %USERPROFILE%\\.flux-signing; back it up,
   never share it: without it you can't ship updates to people who already have Flux).
2. Builds Flux from the launcher source with this release's version, update channel and Microsoft
   app baked in.
3. Puts the ready-to-share copy in ..\\Share (and ..\\Flux-<version>.zip).
4. Signs the exe and writes releases\\v<version>\\ with the two files to upload to GitHub:
   Flux.exe and update_manifest_windows.json.
"""

import base64
import hashlib
import json
import os
import secrets
import shutil
import subprocess
import sys
import time
import zipfile
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

HERE = Path(__file__).resolve().parent
CONFIG = HERE / "flux-release.json"
SHARE = HERE.parent / "Share"


def load_config():
    config = json.loads(CONFIG.read_text(encoding="utf-8"))
    config["signing_key"] = os.path.expandvars(config["signing_key"])
    return config


# --- minisign-compatible signing (what Flux's updater checks with minisign_verify) ---

def load_or_create_key(path: Path):
    if path.exists():
        data = json.loads(path.read_text(encoding="utf-8"))
        key = Ed25519PrivateKey.from_private_bytes(bytes.fromhex(data["seed"]))
        return key, bytes.fromhex(data["key_id"])
    path.parent.mkdir(parents=True, exist_ok=True)
    key = Ed25519PrivateKey.generate()
    key_id = secrets.token_bytes(8)
    seed = key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption())
    path.write_text(json.dumps({"seed": seed.hex(), "key_id": key_id.hex(),
                                "note": "Flux update-signing key. Back this up. Never share it."}, indent=2), encoding="utf-8")
    print(f"Made a new update-signing key: {path}")
    print("  BACK IT UP (e.g. a USB stick or password manager). Without it you can't ship updates.")
    return key, key_id


def public_key_text(key, key_id: bytes) -> str:
    pk = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    b64 = base64.b64encode(b"Ed" + key_id + pk).decode()
    return f"untrusted comment: minisign public key {key_id[::-1].hex().upper()}\n{b64}\n"


def sign(key, key_id: bytes, data: bytes, file_name: str, version: str) -> str:
    """A minisign signature file (pre-hashed BLAKE2b-512 variant). The version goes in the signed
    ("trusted") comment, so an old signed build can't be offered as a newer one."""
    digest = hashlib.blake2b(data, digest_size=64).digest()
    signature = key.sign(digest)
    trusted = f"timestamp:{int(time.time())}\tfile:{file_name}\tversion:{version}\thashed"
    global_signature = key.sign(signature + trusted.encode())
    return (
        "untrusted comment: signature from Flux update key\n"
        + base64.b64encode(b"ED" + key_id + signature).decode() + "\n"
        + f"trusted comment: {trusted}\n"
        + base64.b64encode(global_signature).decode() + "\n"
    )


def main():
    config = load_config()
    if len(sys.argv) > 1:
        config_raw = json.loads(CONFIG.read_text(encoding="utf-8"))
        config_raw["version"] = sys.argv[1].lstrip("v")
        CONFIG.write_text(json.dumps(config_raw, indent=2) + "\n", encoding="utf-8")
        config = load_config()

    version = config["version"]
    repo = config.get("github_repo", "").strip().strip("/")
    client_id = config.get("microsoft_client_id", "").strip()
    if client_id and not config.get("mojang_approved", False):
        # Minecraft refuses sign-ins from an app Mojang hasn't approved yet: keep the working one.
        print(f"NOTE: Flux's Microsoft app ({client_id}) is waiting for Mojang's approval, so this build")
        print("      still signs in through Pandora Launcher's app. When Mojang says yes, set")
        print('      "mojang_approved": true in flux-release.json and build again.')
        client_id = ""
    source = Path(config["launcher_source"])
    key, key_id = load_or_create_key(Path(config["signing_key"]))
    pubkey_text = public_key_text(key, key_id)

    env = dict(os.environ)
    env["FLUX_VERSION"] = version
    env["CARGO_TARGET_DIR"] = str(source / "target-dist")  # separate from day-to-day builds
    # Release exes carry no debug info or symbol paths (smaller, and cleaner for antivirus scans).
    env["CARGO_PROFILE_RELEASE_DEBUG"] = "false"
    env["CARGO_PROFILE_RELEASE_STRIP"] = "symbols"
    # Don't bake this PC's folders (and user name) into the exe's error messages.
    home = os.path.expanduser("~")
    # (Tried -C link-arg=/RELEASE for a PE checksum in 1.0.4: Trapmine's ML score got worse, so it's off.)
    # rustc uses the last matching rule, so the project folder (more specific) goes last.
    env["RUSTFLAGS"] = f"--remap-path-prefix={home}=~ --remap-path-prefix={source}=flux"
    if repo:
        env["FLUX_UPDATE_URL"] = f"https://github.com/{repo}/releases/latest/download"
        env["FLUX_UPDATE_PUBKEY"] = base64.b64encode(pubkey_text.encode()).decode()
    else:
        for k in ("FLUX_UPDATE_URL", "FLUX_UPDATE_PUBKEY"):
            env.pop(k, None)
        print("NOTE: github_repo is empty in flux-release.json, so this build can't auto-update.")
        print("      Fill it in before giving Flux to people, or their copies will never update.")
    if client_id:
        env["FLUX_MSA_CLIENT_ID"] = client_id
    else:
        env.pop("FLUX_MSA_CLIENT_ID", None)
        if not config.get("microsoft_client_id"):
            print("NOTE: microsoft_client_id is empty, so sign-in still uses Pandora Launcher's Microsoft app.")

    print(f"Building Flux {version} (this takes a few minutes)...")
    subprocess.run(["cargo", "build", "--release", "-p", "pandora_launcher"], cwd=source, env=env, check=True)
    exe = source / "target-dist" / "release" / "Flux.exe"
    data = exe.read_bytes()

    # The share folder: what you give people.
    SHARE.mkdir(parents=True, exist_ok=True)
    shutil.copy2(exe, SHARE / "Flux.exe")
    shutil.copy2(HERE / "share-files" / "README.txt", SHARE / "README.txt")
    shutil.copy2(source / "LICENSE", SHARE / "LICENSE.txt")
    zip_path = HERE.parent / f"Flux-{version}.zip"
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
        for f in SHARE.iterdir():
            z.write(f, f"Flux/{f.name}")

    # The update: signed exe + manifest, uploaded to the GitHub release v<version>.
    out = HERE / "releases" / f"v{version}"
    out.mkdir(parents=True, exist_ok=True)
    shutil.copy2(exe, out / "Flux.exe")
    signature = sign(key, key_id, data, "Flux.exe", version)
    download = f"https://github.com/{repo or 'YOUR-NAME/flux'}/releases/download/v{version}/Flux.exe"
    manifest = {
        "version": version,
        "downloads": {"x86_64": {"executable": {
            "download": download,
            "size": len(data),
            "sha1": hashlib.sha1(data).hexdigest(),
            "sig": base64.b64encode(signature.encode()).decode(),
        }}},
    }
    (out / "update_manifest_windows.json").write_text(json.dumps(manifest, indent=4) + "\n", encoding="utf-8")
    (HERE / "update-public-key.txt").write_text(pubkey_text, encoding="utf-8")

    print()
    print(f"Done: Flux {version}")
    print(f"  Share with people:   {SHARE}  (or {zip_path.name})")
    print(f"  Upload to GitHub:    {out}\\Flux.exe and update_manifest_windows.json")
    if repo:
        print(f"                       as a release tagged v{version} on https://github.com/{repo}/releases/new")


if __name__ == "__main__":
    main()
