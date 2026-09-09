"""Reject native dependencies that are absent from a stock supported OS."""
import platform
import re
import subprocess
import sys

for binary in sys.argv[1:]:
    if platform.system() == "Darwin":
        output = subprocess.check_output(["otool", "-L", binary], text=True)
        dependencies = [line.strip().split(" (", 1)[0] for line in output.splitlines()[1:]]
        unexpected = [d for d in dependencies
                      if not d.startswith(("/usr/lib/", "/System/Library/"))]
    elif platform.system() == "Linux":
        # Inspect ELF metadata without executing the binary (unlike ldd).
        output = subprocess.check_output(["readelf", "-d", binary], text=True)
        dependencies = re.findall(r"\(NEEDED\).*?\[(.*?)\]", output)
        allowed = {"libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2",
                   "librt.so.1", "libgcc_s.so.1", "ld-linux-x86-64.so.2",
                   "ld-linux-aarch64.so.1"}
        unexpected = [d for d in dependencies if d not in allowed]
    else:
        raise SystemExit("Binary downloads are currently supported on Linux and macOS")
    print(f"{binary}: {', '.join(dependencies)}")
    if unexpected:
        raise SystemExit(f"Unexpected dynamic dependencies: {unexpected}")
