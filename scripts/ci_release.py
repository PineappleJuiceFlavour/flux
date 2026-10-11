"""Release build on GitHub Actions (see .github/workflows/release.yml).

Builds Flux.exe for the tag in FLUX_TAG (v1.2.3 -> version 1.2.3) from this checkout, signs the update with
the FLUX_UPDATE_KEY secret (the JSON from %USERPROFILE%\\.flux-signing\\flux-update-key.json) and
writes dist\\Flux.exe and dist\\update_manifest_windows.json, ready to attach to the release.
"""

import base64
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release import public_key_text, sign  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent


def main():
    version = os.environ["FLUX_TAG"].lstrip("v")
    repo = os.environ["GITHUB_REPOSITORY"]
    key_json = os.environ.get("FLUX_UPDATE_KEY", "").strip()
    if not key_json:
        sys.exit("FLUX_UPDATE_KEY secret is missing: add it in the repo's Settings > Secrets and variables > Actions")
    key_data = json.loads(key_json)
    key = Ed25519PrivateKey.from_private_bytes(bytes.fromhex(key_data["seed"]))
    key_id = bytes.fromhex(key_data["key_id"])
    pubkey_text = public_key_text(key, key_id)

    cargo_home = os.environ.get("CARGO_HOME") or str(Path.home() / ".cargo")
    env = dict(os.environ)
    env.update({
        "FLUX_VERSION": version,
        "FLUX_UPDATE_URL": f"https://github.com/{repo}/releases/latest/download",
        "FLUX_UPDATE_PUBKEY": base64.b64encode(pubkey_text.encode()).decode(),
        "CARGO_PROFILE_RELEASE_DEBUG": "false",
        "CARGO_PROFILE_RELEASE_STRIP": "symbols",
        # No build machine paths in the exe: the checkout becomes "flux", Cargo's folder "cargo".
        "RUSTFLAGS": f"--remap-path-prefix={cargo_home}=cargo --remap-path-prefix={ROOT}=flux",
    })
    # Flux's own Microsoft app, once Mojang has approved it (repo variable FLUX_MSA_CLIENT_ID).
    for optional in ("FLUX_MSA_CLIENT_ID", "FLUX_CURSEFORGE_KEY"):
        if not env.get(optional):
            env.pop(optional, None)

    print(f"Building Flux {version}...", flush=True)
    subprocess.run(["cargo", "build", "--release", "--frozen", "-p", "pandora_launcher"], cwd=ROOT, env=env, check=True)
    data = (ROOT / "target" / "release" / "Flux.exe").read_bytes()

    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    (dist / "Flux.exe").write_bytes(data)
    manifest = {
        "version": version,
        "downloads": {"x86_64": {"executable": {
            "download": f"https://github.com/{repo}/releases/download/v{version}/Flux.exe",
            "size": len(data),
            "sha1": hashlib.sha1(data).hexdigest(),
            "sig": base64.b64encode(sign(key, key_id, data, "Flux.exe", version).encode()).decode(),
        }}},
    }
    (dist / "update_manifest_windows.json").write_text(json.dumps(manifest, indent=4) + "\n", encoding="utf-8")
    sha256 = hashlib.sha256(data).hexdigest()
    (dist / "SHA256.txt").write_text(f"{sha256}  Flux.exe\n", encoding="utf-8")
    print(f"Flux {version}: SHA-256 {sha256}")


if __name__ == "__main__":
    main()
