"""Check Rust probe WAVs using colaclanth/sstv, an independent decoder.

Requires packages isolated in target/sstv-reference and sstv_probe built.
Outputs PNGs and numeric comparisons under target/sstv-spike.
"""
import json
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(root / "target/sstv-reference"))
from PIL import Image

out = root / "target/sstv-spike"
out.mkdir(parents=True, exist_ok=True)
results = {}
for mode in ["Robot36", "Martin1", "Scottie1", "Pd120"]:
    wav = out / f"{mode}-rust.wav"
    if not wav.exists():
        subprocess.run([str(root / "target/debug/examples/sstv_probe.exe"),
                        "encode", mode, str(wav)], check=True)
    png = out / f"{mode}-independent.png"
    code = (
        "import os,sys;sys.path.insert(0,sys.argv.pop(1));"
        "os.get_terminal_size=lambda *a:os.terminal_size((80,24));"
        "from sstv.__main__ import main;main()"
    )
    with (out / f"{mode}-independent.log").open("w") as log:
        completed = subprocess.run([sys.executable, "-c", code, str(root / "target/sstv-reference"),
                                    "-d", str(wav), "-o", str(png)], stdout=log, stderr=log)
    if completed.returncode:
        results[mode] = {"error": "reference decoder failed; see independent.log"}
        print(mode, results[mode], flush=True)
        continue
    image = Image.open(png).convert("RGB")
    w, h = image.size
    expected = [(x * 255 // (w - 1), y * 255 // (h - 1), 96)
                for y in range(h) for x in range(w)]
    mae = sum(abs(a - b) for pixel, reference in zip(image.getdata(), expected)
              for a, b in zip(pixel, reference)) / (w * h * 3)
    # Decode the independent encoder through the same decoder as a control.
    control_png = out / f"{mode}-control.png"
    with (out / f"{mode}-control.log").open("w") as log:
        subprocess.run([sys.executable, "-c", code, str(root / "target/sstv-reference"),
                        "-d", str(out / f"{mode}-pysstv.wav"), "-o", str(control_png)],
                       stdout=log, stderr=log, check=True)
    control = Image.open(control_png).convert("RGB")
    assert control.size == image.size
    control_mae = sum(abs(a - b) for pixel, reference in zip(control.getdata(), expected)
                      for a, b in zip(pixel, reference)) / (w * h * 3)
    delta = sum(abs(a - b) for pixel, reference in zip(image.getdata(), control.getdata())
                for a, b in zip(pixel, reference)) / (w * h * 3)
    results[mode] = {"width": w, "height": h, "rgb_mae": mae,
                     "control_mae": control_mae, "rust_vs_control_mae": delta}
    print(mode, results[mode], flush=True)
(out / "encoder-results.json").write_text(json.dumps(results, indent=2) + "\n")
assert all(r.get("rgb_mae", float("inf")) < 12 for r in results.values()), "Absolute encoder quality gate failed; inspect control results"
