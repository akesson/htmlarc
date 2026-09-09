"""Check package-local notices against the root copies; --sync refreshes them.

Keep real files in each package so Cargo and Python source builds are complete
on Windows as well as Unix, without requiring symlink support or build hooks.
"""

import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACKAGES = ("crates/htmlarc-dom", "crates/htmlarc-archive", "crates/htmlarc-py", "cli/htmlarc")
NOTICES = ("LICENSE", "NOTICE", "COMMERCIAL.md")


def notice_inputs():
    paths = [ROOT / "Cargo.lock", ROOT / "Cargo.toml", ROOT / "about.toml",
             ROOT / "rust-toolchain.toml",
             ROOT / "scripts/generate_notices.py"]
    paths += sorted(ROOT.glob("crates/*/Cargo.toml")) + sorted(ROOT.glob("cli/*/Cargo.toml"))
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in paths}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sync", action="store_true")
    args = parser.parse_args()
    recorded = json.loads((ROOT / "licenses-inputs.json").read_text(encoding="utf-8"))
    if recorded != notice_inputs():
        parser.exit(1, "License inputs changed; regenerate notices as documented in docs/releasing.md\n")
    generated = (ROOT / "cli/htmlarc-convert/THIRD_PARTY_NOTICES.txt",
                 ROOT / "crates/htmlarc-py/THIRD_PARTY_NOTICES.txt")
    stale = [str(p.relative_to(ROOT)) for p in generated if not p.is_file()]

    for package in PACKAGES:
        for name in NOTICES:
            source = (ROOT / name).read_bytes()
            target = ROOT / package / name
            if args.sync:
                target.write_bytes(source)
            elif not target.is_file() or target.read_bytes() != source:
                stale.append(str(target.relative_to(ROOT)))
    if stale:
        parser.exit(1, "Stale package notices: " + ", ".join(stale)
                    + "\nRun python3 scripts/package_notices.py --sync\n")
    print("Package notices match the root files.")


if __name__ == "__main__":
    main()
