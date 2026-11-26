#!/usr/bin/env python3
"""Generate public source + pinned-hash manifest. Does not upload anything."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
source = root / "extension" / "animeav1.json"
data = json.loads(source.read_text())
assert data["id"] == "animeav1"
assert data["version"] and isinstance(data["version_code"], int)
content = (json.dumps(data, ensure_ascii=False, indent=2) + "\n").encode()
(root / "animeav1_extension.json").write_bytes(content)
manifest = {
    "id": data["id"], "name": "AnimeAV1", "version": data["version"],
    "version_code": data["version_code"],
    "extension_url": "https://raw.githubusercontent.com/XKillBot/AniLinux/main/animeav1_extension.json",
    "sha256": hashlib.sha256(content).hexdigest(),
}
(root / "extension_manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
print("Generados animeav1_extension.json y extension_manifest.json. Revisa y publica ambos juntos.")
