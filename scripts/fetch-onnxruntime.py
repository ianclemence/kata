"""Put the ONNX Runtime shared library for this machine into a folder.

Kata loads ONNX Runtime at run time and looks next to its own executable, so a
release ships the library beside `kata`. The library comes from the official
`onnxruntime` wheel on PyPI for this platform, with its SHA-256 checked against
what PyPI publishes, then renamed to the plain name Kata looks for.

usage: python scripts/fetch-onnxruntime.py <dest-dir> [--version 1.32.0]
"""
import argparse
import hashlib
import json
import platform
import shutil
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path


def wheel_matches(filename: str) -> bool:
    """Is this the cp312 wheel for the machine we are on?"""
    system, machine = platform.system(), platform.machine().lower()
    arm = machine in ("arm64", "aarch64")
    if "cp312" not in filename:
        return False
    if system == "Windows":
        return filename.endswith("win_arm64.whl" if arm else "win_amd64.whl")
    if system == "Darwin":
        if not arm:
            sys.exit("Intel macOS has no current onnxruntime wheel; build ONNX Runtime yourself.")
        return "macosx" in filename and filename.endswith("arm64.whl")
    return "manylinux" in filename and filename.endswith("aarch64.whl" if arm else "x86_64.whl")


def target_name() -> str:
    return {"Windows": "onnxruntime.dll", "Darwin": "libonnxruntime.dylib"}.get(platform.system(), "libonnxruntime.so")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("dest")
    parser.add_argument("--version", default="1.32.0")
    args = parser.parse_args()

    meta = json.load(urllib.request.urlopen(f"https://pypi.org/pypi/onnxruntime/{args.version}/json", timeout=60))
    candidates = [f for f in meta["urls"] if f["packagetype"] == "bdist_wheel" and wheel_matches(f["filename"])]
    if not candidates:
        sys.exit(f"no onnxruntime {args.version} wheel for {platform.system()} {platform.machine()}")
    wheel = candidates[0]
    print(f"onnxruntime {args.version}: {wheel['filename']}")

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / wheel["filename"]
        with urllib.request.urlopen(wheel["url"], timeout=300) as response, open(path, "wb") as out:
            shutil.copyfileobj(response, out)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != wheel["digests"]["sha256"]:
            sys.exit(f"checksum mismatch for {wheel['filename']}: {digest}")

        dest = Path(args.dest)
        dest.mkdir(parents=True, exist_ok=True)
        wanted = target_name()
        found = False
        with zipfile.ZipFile(path) as z:
            for name in z.namelist():
                base = name.rsplit("/", 1)[-1]
                if not name.startswith("onnxruntime/capi/"):
                    continue
                is_main = (
                    base == "onnxruntime.dll"
                    or (base.startswith("libonnxruntime.so") and "providers" not in base)
                    or (base.startswith("libonnxruntime.") and base.endswith(".dylib") and "providers" not in base)
                )
                if is_main:
                    (dest / wanted).write_bytes(z.read(name))
                    found = True
                    print(f"  {name} -> {dest / wanted}")
                elif base == "onnxruntime_providers_shared.dll":
                    (dest / base).write_bytes(z.read(name))
                    print(f"  {name} -> {dest / base}")
        if not found:
            sys.exit("the wheel has no ONNX Runtime library in onnxruntime/capi/")
        if platform.system() != "Windows":
            (dest / wanted).chmod(0o755)


if __name__ == "__main__":
    main()
