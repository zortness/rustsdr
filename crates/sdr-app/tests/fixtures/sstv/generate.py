"""Generate original gradient fixtures with independent PySSTV 0.5.7.

Run from the repository root. Test-only packages live in target/sstv-python;
media goes to target/sstv-spike. No downloaded artwork or radio access.
"""
import hashlib
import json
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(root / "target/sstv-python"))
from PIL import Image
from pysstv.color import Robot36, MartinM1, ScottieS1, PD120

out = root / "target/sstv-spike"
out.mkdir(parents=True, exist_ok=True)
manifest = {}
for name, codec in [("Robot36", Robot36), ("Martin1", MartinM1),
                    ("Scottie1", ScottieS1), ("Pd120", PD120)]:
    w, h = codec.WIDTH, codec.HEIGHT
    im = Image.new("RGB", (w, h))
    im.putdata([(x * 255 // (w - 1), y * 255 // (h - 1), 96)
                for y in range(h) for x in range(w)])
    im.save(out / f"{name}-reference.png")
    path = out / f"{name}-pysstv.wav"
    codec(im, 48000, 16).write_wav(str(path))
    manifest[path.name] = {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                           "width": w, "height": h, "encoder": "PySSTV 0.5.7"}
    print(name, manifest[path.name], flush=True)
(out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
