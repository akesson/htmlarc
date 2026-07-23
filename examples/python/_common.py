"""Shared helpers for the runnable recipes: data cache, polite downloads, decoding."""

from pathlib import Path

import requests

DATA = Path(__file__).resolve().parent / "data"

USER_AGENT = "htmlarc-recipes/0.1 (+https://github.com/akesson/htmlarc)"


def get(url: str, tries: int = 3, **kw) -> requests.Response:
    """GET with a proper User-Agent and a small retry loop (CC endpoints 503 under load)."""
    import time

    kw.setdefault("timeout", 60)
    headers = {"User-Agent": USER_AGENT, **kw.pop("headers", {})}
    for attempt in range(tries):
        r = requests.get(url, headers=headers, **kw)
        if r.status_code in (503, 429) and attempt < tries - 1:
            time.sleep(2 ** (attempt + 1))
            continue
        r.raise_for_status()
        return r
    raise AssertionError("unreachable")


def decode_html(body: bytes, content_type: str | None) -> str:
    """Decode fetched HTML bytes to the str htmlarc parses.

    Honors an explicit charset in the Content-Type header, falls back to UTF-8
    with replacement — the same policy the htmlarc benchmarks use. For corpora
    with messier encodings, swap the fallback for charset-normalizer.
    """
    if content_type and "charset=" in content_type:
        cs = content_type.split("charset=")[-1].split(";")[0].strip().strip("\"'")
        try:
            return body.decode(cs, errors="replace")
        except LookupError:
            pass
    return body.decode("utf-8", errors="replace")
