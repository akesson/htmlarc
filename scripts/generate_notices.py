"""Generate compact notices for the Python wheel and the two-CLI download.

Source-only Cargo crates do not redistribute dependency implementations.
Keep complete terms and copyrights, sharing identical terms between components.
"""
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from package_notices import ROOT, notice_inputs

UNIX_TARGETS = (
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "aarch64-apple-darwin",
)
PYTHON_TARGETS = (*UNIX_TARGETS, "x86_64-pc-windows-msvc")


def gather(manifest, targets, *, standard_library=False):
    command = ["cargo", "about", "generate", "--locked", "--fail", "--format", "json",
               "--config", str(ROOT / "about.toml"), "--manifest-path", str(manifest)]
    for target in targets:
        command += ["--target", target]
    env = os.environ.copy()
    if standard_library:
        # Metadata only, in a disposable rust-src copy. No unstable application builds.
        env["RUSTC_BOOTSTRAP"] = "1"
        command += ["--features", "backtrace,panic-unwind"]
    return json.loads(subprocess.check_output(command, cwd=ROOT, env=env))


def dependency_notices(data, *, standard_library=False):
    notices, packages = [], {}
    for license in data["licenses"]:
        names = set()
        for user in license["used_by"]:
            package = user["crate"]
            # Our source uses LICENSE; Rust's in-tree code is covered explicitly below.
            if package["source"] is None:
                continue
            key = f"{package['name']} {package['version']}"
            names.add(("Rust standard library: " if standard_library else "") + key)
            packages[key] = package
        if names:
            notices.append((license["id"], license["text"], names))
    for key, package in sorted(packages.items()):
        root = Path(package["manifest_path"]).parent
        # Preserve only notices associated with dependencies in this artifact's graph.
        for path in sorted(root.glob("NOTICE*")):
            if path.is_file():
                notices.append(("Attribution", path.read_text(encoding="utf-8"), {key}))
        if package["name"] == "zstd-sys":
            notices.append(("BSD-3-Clause", (root / "zstd/LICENSE").read_text(), {"Zstandard"}))
        # liblzma is public domain; XZ command-line tools and their GPL/LGPL terms
        # are not part of our binary. The Rust xz2/lzma-sys licenses remain above.
    return notices


def rust_source_notices(sysroot, library):
    docs = sysroot / "share/doc/rust"
    mit = (docs / "licenses/MIT.txt").read_text().replace(
        "Copyright (c) <year> <copyright holders>",
        "Copyright The Rust Project Developers (https://thanks.rust-lang.org)\n"
        "Copyright (c) 2019 The Crossbeam Project Developers",
    )
    result = [("MIT", mit, {"Rust standard library 1.96.0"}),
              ("Unicode-3.0", (docs / "licenses/Unicode-3.0.txt").read_text(),
               {"Rust standard library Unicode data"})]
    for name in ("backtrace", "stdarch", "portable-simd"):
        result.append(("MIT", (library / name / "LICENSE-MIT").read_text(), {f"Rust {name}"}))
    # These in-tree licenses include their own exceptions and attribution; do not
    # replace them with the SPDX fallback text from Cargo's package metadata.
    for name in ("compiler-builtins", "compiler-builtins/libm"):
        result.append(("Rust compiler runtime", (library / name / "LICENSE.txt").read_text(),
                       {f"Rust {name}"}))
    return result


def render(notices, artifact):
    groups = {}
    for license, text, names in notices:
        text = text.strip()
        prefix = ""
        # Identical MIT permission/warranty terms need only appear once, provided
        # every component's original copyright notice stays associated with them.
        if license == "MIT" and "Permission is hereby granted" in text:
            prefix, text = text.split("Permission is hereby granted", 1)
            text = "Permission is hereby granted" + text
        key = (license, " ".join(text.split()))
        group = groups.setdefault(key, {"text": text, "attributions": {}})
        group["attributions"].setdefault(prefix.strip(), set()).update(names)
    lines = [f"Third-party notices — {artifact}",
             "Generated from locked release dependencies and Rust 1.96.0.",
             "Shared license terms below apply to each listed component.",
             "htmlarc's own terms are in LICENSE and COMMERCIAL.md.\n"]
    for (license, _), group in sorted(groups.items()):
        lines += ["=" * 72, license, "=" * 72]
        for prefix, names in sorted(group["attributions"].items()):
            lines.append("Components: " + "; ".join(sorted(names)))
            if prefix:
                lines.append(prefix)
        lines += ["", group["text"], ""]
    return "\n".join(lines)


def main():
    version = subprocess.check_output(["cargo", "about", "--version"], text=True).strip()
    if version != "cargo-about 0.9.2":
        raise SystemExit("Install cargo-about 0.9.2 with --locked --features cli")
    rust = subprocess.check_output(["rustc", "--version"], text=True)
    if not rust.startswith("rustc 1.96.0 "):
        raise SystemExit("Review the standard-library notice selection before changing Rust 1.96.0")
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    source = sysroot / "lib/rustlib/src/rust/library"
    if not source.is_dir() or not (sysroot / "share/doc/rust/licenses/MIT.txt").is_file():
        raise SystemExit("Run: rustup component add rust-src rust-docs")
    with tempfile.TemporaryDirectory(prefix="htmlarc-notices-") as tmp:
        library = Path(tmp) / "library"
        shutil.copytree(source, library)
        # This internal shim inherits Rust's license but omits the Cargo field.
        manifest = library / "windows_link/Cargo.toml"
        manifest.write_text(manifest.read_text().replace(
            "[package]", '[package]\nlicense = "MIT OR Apache-2.0"', 1))
        rust_notices = rust_source_notices(sysroot, library)
        for output, manifests, targets, label in (
            (ROOT / "crates/htmlarc-py/THIRD_PARTY_NOTICES.txt",
             ["crates/htmlarc-py/Cargo.toml"], PYTHON_TARGETS, "Python wheel"),
            (ROOT / "cli/htmlarc-convert/THIRD_PARTY_NOTICES.txt",
             ["cli/htmlarc/Cargo.toml", "cli/htmlarc-convert/Cargo.toml"], UNIX_TARGETS,
             "htmlarc and htmlarc-convert binary bundle"),
        ):
            notices = list(rust_notices)
            notices += dependency_notices(gather(library / "std/Cargo.toml", targets,
                                                 standard_library=True), standard_library=True)
            for manifest in manifests:
                notices += dependency_notices(gather(ROOT / manifest, targets))
            output.write_text(render(notices, label), encoding="utf-8")
    (ROOT / "licenses-inputs.json").write_text(
        json.dumps(notice_inputs(), indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
