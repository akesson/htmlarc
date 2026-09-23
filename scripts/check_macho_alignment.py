"""Reject Mach-O files whose symbol table or string pool is not 8-byte aligned.

macOS 27's dyld refuses to load such files ("mis-aligned LINKEDIT string pool"),
while older macOS loads them fine, so tests on the macOS 15 runners cannot catch
it. rust-objcopy in Rust 1.96 produced this layout when stripping. Accepts
binaries, dylibs and wheels (the .so files inside are checked).
"""
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path


def check(path, label=None):
    label = label or path
    output = subprocess.check_output(["otool", "-l", str(path)], text=True)
    offsets = {}
    for line in output.splitlines():
        fields = line.split()
        if len(fields) == 2 and fields[0] in ("symoff", "stroff"):
            offsets[fields[0]] = int(fields[1])
    if not offsets:
        raise SystemExit(f"{label}: no LC_SYMTAB found")
    print(f"{label}: {offsets}")
    return [f"{label}: {name}={value} is not 8-byte aligned"
            for name, value in offsets.items() if value % 8]


errors = []
with tempfile.TemporaryDirectory() as tmp:
    for arg in sys.argv[1:]:
        if arg.endswith(".whl"):
            with zipfile.ZipFile(arg) as wheel:
                members = [m for m in wheel.namelist() if m.endswith(".so")]
                if not members:
                    raise SystemExit(f"{arg}: no .so files in wheel")
                for member in members:
                    extracted = wheel.extract(member, Path(tmp) / Path(arg).name)
                    errors += check(extracted, f"{arg}!{member}")
        else:
            errors += check(arg)

if errors:
    raise SystemExit("\n".join(errors))
