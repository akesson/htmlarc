"""Run the whole bench.py suite and write one JSON report (environment, inputs, runs).

  python run_suite.py OUT.json [--repeats 3] [--corpora wikt,cc]

Every phase is a fresh process (`bench.py <phase> <corpus>`), rebuilt archive per
repeat, warm phases first and the HTMLARC_BENCH_COLD=1 phases last. Uses the same
HTMLARC_BENCH_DATA / HTMLARC_CORPUS as bench.py; the Python running this script
runs the phases, so its environment is the one recorded.
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
CORPUS = Path(os.environ.get("HTMLARC_CORPUS", HERE.parent.parent / "corpus"))

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


def environment():
    import htmlarc

    cpu = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"],
                         capture_output=True, text=True).stdout.strip()
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
        out["cc_warc"] = {"file": "cc_000.warc.gz",
                          "sha256": sha256(CORPUS / "cc_000.warc.gz")}
    return out


def run(phase, corpus, repeat, cold):
    env = dict(os.environ, HTMLARC_BENCH_COLD="1" if cold else "0")
    load = os.getloadavg()[0]
    proc = subprocess.run([sys.executable, str(HERE / "bench.py"), phase, corpus],
                          env=env, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.exit(f"{phase} {corpus} failed:\n{proc.stderr}")
    row = json.loads(proc.stdout.strip().splitlines()[-1])
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

    report = {**environment(), "inputs": inputs(corpora), "runs": []}
    for repeat in range(1, args.repeats + 1):
        for corpus in corpora:
            cc = corpus == "cc"
            for phase in WARM + (WARM_CC if cc else []):
                report["runs"].append(run(phase, corpus, repeat, cold=False))
            for phase in COLD + (COLD_CC if cc else []):
                report["runs"].append(run(phase, corpus, repeat, cold=True))
    report["archive_sha256"] = {c: sha256(DIR / f"{c}.htmlarc") for c in corpora}
    # Header: 8-byte magic, then the format version byte.
    report["archive_format"] = (DIR / f"{corpora[0]}.htmlarc").read_bytes()[8]
    Path(args.out).write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
