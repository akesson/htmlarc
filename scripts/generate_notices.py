"""Regenerate release notices from the locked dependency graph and pinned Rust toolchain.
"""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
about_version = subprocess.check_output(["cargo", "about", "--version"], text=True).strip()
if about_version != "cargo-about 0.9.2":
    raise SystemExit("Install cargo-about 0.9.2 with --locked --features cli")
subprocess.run(["cargo", "about", "generate", "--locked", "--workspace", "--fail",
                "scripts/licenses.hbs", "-o", "THIRD_PARTY_NOTICES.md"], cwd=ROOT, check=True)
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT
))
sections = ["Native licenses and supplementary upstream notices\n"
            "=================================================\n\n"
            "This supplements THIRD_PARTY_NOTICES.md. Individual distributions use\n"
            "a subset. Only liblzma from XZ Utils is linked, not its command-line tools.\n"]
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["source"] is None:
        continue
    root = Path(package["manifest_path"]).parent
    paths = sorted(p for p in root.iterdir()
                   if p.is_file() and p.name.upper().startswith("NOTICE"))
    extras = {
        "lzma-sys": ["xz-5.2/COPYING", "xz-5.2/AUTHORS"],
        "zstd-sys": ["zstd/LICENSE"],
    }.get(package["name"], [])
    paths += [root / name for name in extras]
    for path in paths:
        sections.append(f"\n{package['name']} {package['version']} — {path.relative_to(root)}\n"
                        + "-" * 72 + "\n" + path.read_text(encoding="utf-8"))
(ROOT / "THIRD_PARTY_NATIVE.txt").write_text("\n".join(sections), encoding="utf-8")

# Rust links its standard library into the native distributions too.
sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
standard_library = sysroot / "share/doc/rust/COPYRIGHT-library.html"
if not standard_library.is_file():
    raise SystemExit("Install rust-docs for the pinned toolchain: rustup component add rust-docs")
(ROOT / "THIRD_PARTY_RUST.html").write_bytes(standard_library.read_bytes())

# Snapshot the inputs only after the companion license generator has succeeded.
from package_notices import notice_inputs
(ROOT / "licenses-inputs.json").write_text(
    json.dumps(notice_inputs(), indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
