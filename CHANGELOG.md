# Changelog

## 0.2.0

Decodes legacy-charset pages at ingest instead of garbling them, follows the specs
for `[class…]`, attribute-value case, `:root` and doctype siblings, adds `*`, and
speeds up parsing and Python sweeps. Archive format is unchanged (v12). See
[release notes](docs/releases/v0.2.0.md).

## 0.1.1

Adds a Python wheel for Windows ARM64. See [release notes](docs/releases/v0.1.1.md).

## 0.1.0 (initial release)

See [release notes](docs/releases/v0.1.0.md) for features, installation, platform
coverage, archive compatibility, and limitations.

Publishing preparation fixes interrupted append recovery, bounds append reads to
footer metadata, updates ZIM dependencies, statically links xz in converter
binaries, and includes third-party license notices in distributions.
