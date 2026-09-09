# Releasing htmlarc

`.github/workflows/release.yml` builds and tests release artifacts on a manual run.
Pushing a matching `v<version>` tag additionally publishes to PyPI and crates.io,
then creates a **draft** GitHub release with CLI downloads and SHA-256 checksums.
Publishing that draft is a separate editorial step; the packages are already public.

## Distribution targets

| Artifact | Distribution | Platforms |
| --- | --- | --- |
| Python `htmlarc` | PyPI wheels + source distribution | Linux glibc x86-64/ARM64, macOS Intel/Apple Silicon, Windows x86-64 |
| `htmlarc-dom`, `htmlarc-archive`, `htmlarc` | crates.io | Source builds; CLI requires Unix |
| `htmlarc`, `htmlarc-convert` binaries | GitHub release | Linux x86-64/ARM64, macOS Intel/Apple Silicon |

Python wheels use CPython's stable ABI (3.10+); each wheel is tested on 3.10 and
3.14. Free-threaded Python, PyPy, musl Linux and Windows ARM64 are not part of
this initial wheel matrix. The sdist is installed and tested independently, so
it must include the Rust dependencies needed to build outside the checkout.

On Windows, recovering an abandoned append requires releasing all readers first:
Archive, Document, and Element handles retain its memory mapping. Windows rejects
recovery's truncation while any mapping remains open, even in another process.
Normal appends do not shrink the file and can keep readers open.

Linux wheels target manylinux2014 (glibc 2.17+). CLI binaries are native builds:
the x86-64 Linux build uses Ubuntu 22.04 and ARM64 uses Ubuntu 24.04. They do not
promise the wheels' glibc compatibility. Windows CLI builds are excluded because
`htmlarc` uses termion.

`htmlarc-convert` deliberately has `publish = false`: its patched `zim` reader
is a Git dependency, which crates.io cannot distribute. It remains available
as a release binary or via:

```sh
cargo install --git https://github.com/akesson/htmlarc --tag v0.1.0 --locked htmlarc-convert
```

To publish the converter on crates.io later, first publish the patched ZIM
reader under an available package name and change the dependency to that
registry release. Merely adding a version to the existing Git dependency would
select the unpatched upstream crate for registry consumers.

## One-time account setup

1. Confirm ownership/availability of `htmlarc` on PyPI and all three crate names
   on crates.io. Repository configuration does not reserve registry names.
2. Add a PyPI trusted publisher (a pending publisher if this is the first release):
   owner `akesson`, repository `htmlarc`, workflow `release.yml`, environment `pypi`.
   See [PyPI trusted publishing](https://docs.pypi.org/trusted-publishers/).
3. Create the GitHub environments `pypi` and `crates-io`. Add a crates.io API
   token with publishing access as the `crates-io` environment secret
   `CARGO_REGISTRY_TOKEN`. Use environment protection rules if review is wanted.
4. Verify Actions can create GitHub releases with the job's `contents: write`.

## Rehearse and release

The source packages contain local copies of `LICENSE`, `NOTICE`, and
`COMMERCIAL.md`; Python wheels include them under `.dist-info/licenses/`.
After editing a root notice, run `python3 scripts/package_notices.py --sync`
and commit the copies too. CI checks that they match.

1. Update `[workspace.package].version` and the two internal dependency versions
   in `Cargo.toml` together; update the lockfile. Python inherits this version.
2. Commit the release changes. Run **Release → Run workflow** on that commit's
   branch. Manual runs never publish, including when a tag is selected.
3. Require every job to pass. Download and try the artifacts. This is the first
   real verification of platforms unavailable on the developer's machine.
4. Push `v<version>` pointing at the verified commit. The workflow checks the tag,
   formatting, clippy, Rust tests, and a multi-package Cargo publishing dry run.
   All wheel/source/binary builds must pass before either registry upload begins.
5. Check both registries, install the published packages into fresh environments,
   then edit and publish the draft GitHub release and announce.

After the first release, the public installation commands are:

```sh
pip install htmlarc
cargo install htmlarc --locked
```

Local packaging checks (add `--allow-dirty` only while reviewing uncommitted changes):

```sh
cargo publish --dry-run --locked -p htmlarc-dom -p htmlarc-archive -p htmlarc
uvx maturin==1.15.0 sdist -m crates/htmlarc-py/Cargo.toml -o target/release-dist
uvx maturin==1.15.0 build --release --locked -m crates/htmlarc-py/Cargo.toml -o target/release-dist
```

Cargo packages the three crates together, verifies them using its temporary local
registry, and publishes in dependency order. This also works before the internal
crates exist on crates.io.

## Interrupted publishing

The two registries are independent and uploads cannot be rolled back as one
transaction. Inspect what exists before retrying. If a crate upload succeeded
but a later one failed, publish only the missing crates at the same version from
the tagged commit. Likewise, upload only missing Python artifacts after checking
that existing files belong to this release. Do not move an already published tag
or rebuild different contents under a published version. Use a new patch version
for code changes. If only GitHub draft creation failed, rerun that job.

## Notices, documentation, and release notes

Generate notices once per dependency or toolchain change:

```sh
cargo install cargo-about --version 0.9.2 --locked --features cli
rustup component add rust-src rust-docs
python3 scripts/generate_notices.py
python3 scripts/package_notices.py --sync
```

There are two generated files:

- `crates/htmlarc-py/THIRD_PARTY_NOTICES.txt` ships in wheels and the Python sdist
  (so a wheel built from the sdist retains the notices).
- `cli/htmlarc-convert/THIRD_PARTY_NOTICES.txt` ships with the two-CLI binary download.

Source-only Cargo packages retain LICENSE, NOTICE and COMMERCIAL.md, including
fixture attribution. They do not redistribute dependency implementations, so
no compiled-dependency notice copies are added to them.

The generator selects each artifact's default-feature dependency graph and
supported targets, excludes development/build-only dependencies, and retains
procedural macros that may contribute generated code. It includes applicable
upstream NOTICE files and the bundled zstd license. liblzma is public domain;
the unrelated XZ command-line utilities' GPL/LGPL terms are omitted.

Rust runtime dependencies are resolved from a disposable copy of the pinned
`rust-src`, with backtrace/panic-unwind enabled. `RUSTC_BOOTSTRAP=1` is used only
for Cargo metadata in that copy, never to build htmlarc. Rust's in-tree copyright
notices and compiler-runtime terms are retained explicitly. Toolchain upgrades
require reviewing this selection. Unsupported targets and Rust's test/build
dependencies are excluded instead of copying its entire HTML notice report.

Identical terms share a section while preserving the associated component names
and copyright notices. htmlarc's AGPL text is not repeated in these files.
Review generated changes before committing. CI rejects stale dependency inputs
and stale copies of the project notices.

Create `docs/releases/v<version>.md` and update CHANGELOG.md before rehearsing.
The release workflow requires that notes file and uses it for the GitHub draft.
`cargo audit --deny warnings` gates CI and release checks; do not ignore a new
advisory merely to get a release through. Rustdoc warnings also fail CI.
Unix artifacts are checked for unexpected dynamic dependencies before upload;
for example, a converter linked to `/opt/homebrew/opt/xz/...` fails the check.

Archive format policy and migration limitations are in the root README. Keep
`crates/htmlarc-archive/tests/data/v12.bin` frozen as a reader compatibility
fixture. Performance claims for the initial release must use the v12 write-up,
not the historical v11 benchmark tables.
