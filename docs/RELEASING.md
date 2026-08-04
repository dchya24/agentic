# Releasing agentic

This document describes the release process for the agentic workspace
(`core-agentic` + `agentic-cli`). Releases are published as **annotated git
tags** plus a **GitHub Release** with platform binaries that the
[`agentic update`](../agentic-cli/src/update.rs) self-update mechanism
downloads.

The whole flow is automated by [`scripts/release.sh`](../scripts/release.sh).

## Versioning

- The project follows [Semantic Versioning](https://semver.org).
- The **tag** is the authority: `v<cli-version>` (e.g. `v0.3.0`).
- `agentic-cli` is bumped on every release.
- `core-agentic` is bumped in lockstep **only when it has unreleased
  commits** since the last tag; otherwise its version stays put.

| Bump | When |
|------|------|
| `patch` | Bug fixes, small behavior changes (default) |
| `minor` | New features, backward-compatible additions |
| `major` | Breaking changes (public API, config schema, behavior) |

## Prerequisites

- Clean working tree (commit or stash pending work first).
- [`gh` CLI](https://cli.github.com) installed and authenticated
  (`gh auth login`).
- Git remote `origin` pointing at `github.com:dchya24/agentic`.
- Rust toolchain with `rustfmt` and `clippy` components.

## The process

### 1. Prepare the branch

Releases are cut from `dev` (or `main`/`master`). Finish and merge all work
for the release first, then:

```bash
git checkout dev
git pull
./scripts/release.sh [patch | minor | major]
```

The script runs in six stages and pauses for confirmation at each
destructive step. Pass `--yes` to skip the prompts.

### 2. What the script does

1. **Preflight** — verifies the repo, warns about dirty tree / off-branch,
   checks `gh`, finds the previous release tag.
2. **Version bump** — computes the next version, bumps `version` in
   `agentic-cli/Cargo.toml` (and `core-agentic/Cargo.toml` when it changed),
   updates `Cargo.lock`.
3. **Quality gates** — runs the same checks as CI:
   `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace`.
4. **Commit + tag** — commits as
   `release: vX.Y.Z (agentic-cli X.Y.Z, core-agentic A.B.C)` and creates the
   annotated tag with the same message.
5. **Push** — pushes the branch and tag to `origin`.
6. **Build + publish** — builds a release binary for the current platform,
   writes `dist/agentic-<os>-<arch>` (+ `.sha256`), and creates the GitHub
   release with auto-generated notes (grouped conventional commits).

### 3. After the release

```bash
gh release view vX.Y.Z            # verify notes + assets
git fetch --tags                  # make sure local tags are current
```

Optionally install the new build locally:

```bash
cargo install --path agentic-cli   # or use the downloaded asset
```

## Asset naming

Assets must match what the updater expects:

```
agentic-<os>-<arch>          e.g. agentic-linux-x86_64
                                 agentic-macos-aarch64
                                 agentic-windows-x86_64
```

(`os` = `linux` / `macos` / `windows`, `arch` = `x86_64` / `aarch64`.)
Each asset has a sibling `.sha256` file. The OS qualifier is required —
without it an x86_64 macOS client would match the Linux x86_64 asset and
download the wrong binary.

## Manual release (fallback)

If the script cannot be used, the steps are:

```bash
# 1. bump versions in agentic-cli/Cargo.toml (and core-agentic if changed)
# 2. checks
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --all-targets
# 3. commit + tag
git add agentic-cli/Cargo.toml core-agentic/Cargo.toml Cargo.lock
git commit -m "release: vX.Y.Z (agentic-cli X.Y.Z, core-agentic A.B.C)"
git tag -a vX.Y.Z -m "release: vX.Y.Z (agentic-cli X.Y.Z, core-agentic A.B.C)"
git push origin dev vX.Y.Z
# 4. build + publish
cargo build --release -p agentic-cli
cp target/release/agentic dist/agentic-<os>-<arch>
gh release create vX.Y.Z dist/agentic-<os>-<arch> dist/agentic-<os>-<arch>.sha256 --title vX.Y.Z --notes "..."
```

## Troubleshooting

- **`gh` not authenticated** → `gh auth login`
- **Script aborts on dirty tree** → commit or stash; the release must not
  include unreleased working-tree changes.
- **Clippy/fmt/test fail** → fix before re-running; the script never ships a
  failing tree.
- **Tag already exists** → you cannot overwrite a pushed tag; bump again
  (`patch` → `minor`) or delete the remote tag deliberately.
- **Release created but asset missing** → `gh release upload vX.Y.Z <asset>`
