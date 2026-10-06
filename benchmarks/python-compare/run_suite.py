"""Run the whole bench.py suite and write one JSON report (environment, inputs, runs).

  python run_suite.py OUT.json [--repeats 3] [--corpora wikt,cc]

Every phase is a fresh process (`bench.py <phase> <corpus>`), rebuilt archive per
repeat, warm phases first and the HTMLARC_BENCH_COLD=1 phases last. Uses the same
HTMLARC_BENCH_DATA as bench.py; the Python running this script runs the phases, so
its environment is the one recorded. OUT.json is rewritten after every phase, so
an interrupted run keeps what it measured. A failed phase is recorded as a row with
an "error" and the suite goes on (a failed build skips the rest of that corpus's
repeat, which would otherwise query a stale archive); the exit status is 1 if any
phase failed.
"""

import argparse
import hashlib
import importlib.metadata
import json
import os
import platform
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
DIR = Path(os.environ.get("HTMLARC_BENCH_DATA", HERE / "data"))

WARM = [
    "oneshot_bs4", "oneshot_lxml", "oneshot_htmlarc",
    "build_htmlarc",
    "requery_htmlarc", "requery_htmlarc_scan", "requery_htmlarc_arrow",
    "requery_lxml_par",
    "requery_lxml_count", "requery_lxml_count_par",
    "requery_htmlarc_count", "requery_htmlarc_scan_count",
    "hot_bs4", "hot_lxml",
    "newq_lxml", "newq_lxml_par", "newq_htmlarc",
]
WARM_CC = ["pipeline_read", "pipeline_lxml", "pipeline_lxml_par", "pipeline_bs4"]
COLD = [
    "requery_htmlarc", "requery_htmlarc_scan", "requery_htmlarc_arrow",
    "requery_htmlarc_count", "requery_htmlarc_scan_count", "newq_htmlarc",
]
COLD_CC = ["pipeline_read", "pipeline_lxml", "pipeline_lxml_par"]
PACKAGES = ["htmlarc", "lxml", "cssselect", "beautifulsoup4", "soupsieve",
            "pyarrow", "warcio", "psutil"]


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def cpu_name():
    if sys.platform == "darwin":
        return subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"],
                              capture_output=True, text=True).stdout.strip()
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor()


def environment():
    import htmlarc

    cpu = cpu_name()
    ext = next(Path(htmlarc.__file__).parent.glob("htmlarc*.so"))
    return {
        "date": time.strftime("%Y-%m-%d"),
        "python": sys.version,
        "platform": platform.platform(),
        "cpu": cpu,
        "logical_cpus": os.cpu_count(),
        "packages": {p: importlib.metadata.version(p) for p in PACKAGES},
        "extension_sha256": sha256(ext),
    }


def inputs(corpora):
    import pickle

    out = {}
    for c in corpora:
        with open(DIR / f"{c}.pkl", "rb") as f:
            docs = pickle.load(f)
        out[c] = {"pickle_sha256": sha256(DIR / f"{c}.pkl"), "documents": len(docs),
                  "html_utf8_bytes": sum(len(h.encode()) for _, h in docs)}
    if "cc" in corpora:
        warc = DIR / "cc.warc.gz"  # extract.py's prefix of cc_000.warc.gz
        out["cc_warc"] = {"file": warc.name, "bytes": warc.stat().st_size,
                          "sha256": sha256(warc)}
    return out


def archive(corpus, repeat):
    path = DIR / f"{corpus}.htmlarc"
    with open(path, "rb") as f:
        header = f.read(9)  # 8-byte magic, then the format version byte
    return {"corpus": corpus, "repeat": repeat, "format": header[8],
            "sha256": sha256(path)}


def run(phase, corpus, repeat, cold):
    """Run one phase; returns its row, which has an "error" if the phase failed."""
    env = dict(os.environ, HTMLARC_BENCH_COLD="1" if cold else "0")
    load = os.getloadavg()[0]
    proc = subprocess.run([sys.executable, str(HERE / "bench.py"), phase, corpus],
                          env=env, capture_output=True, text=True)
    try:
        if proc.returncode != 0:
            raise ValueError(f"exit status {proc.returncode}")
        row = json.loads(proc.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError) as e:
        row = {"phase": phase, "corpus": corpus,
               "error": f"{e}\n{proc.stderr[-4000:]}"}
        print(f"{phase} {corpus} failed: {row['error']}", file=sys.stderr, flush=True)
    row.update(repeat=repeat, cache="cold" if cold else "warm", load1_before=round(load, 2))
    print(json.dumps(row), flush=True)
    return row


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--corpora", default="wikt,cc")
    args = ap.parse_args()
    corpora = args.corpora.split(",")

    out = Path(args.out)
    report = {**environment(), "inputs": inputs(corpora), "archives": [], "runs": []}

    def save():
        tmp = out.with_name(out.name + ".tmp")
        tmp.write_text(json.dumps(report, indent=2) + "\n")
        os.replace(tmp, out)

    for repeat in range(1, args.repeats + 1):
        for corpus in corpora:
            cc = corpus == "cc"
            phases = ([(p, False) for p in WARM + (WARM_CC if cc else [])]
                      + [(p, True) for p in COLD + (COLD_CC if cc else [])])
            for phase, cold in phases:
                row = run(phase, corpus, repeat, cold)
                report["runs"].append(row)
                if phase == "build_htmlarc":
                    if "error" in row:
                        save()
                        break
                    report["archives"].append(archive(corpus, repeat))
                save()
    if any("error" in r for r in report["runs"]):
        sys.exit(1)


if __name__ == "__main__":
    main()
