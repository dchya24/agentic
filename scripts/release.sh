#!/usr/bin/env bash
#
# release.sh — cut a new agentic release.
#
# One-command process for releasing the next version:
#
#   ./scripts/release.sh            # patch bump (0.3.0 → 0.3.1)
#   ./scripts/release.sh minor      # minor bump (0.3.0 → 0.4.0)
#   ./scripts/release.sh major      # major bump (0.3.0 → 1.0.0)
#
# What it does:
#   1. Preflight checks (clean tree, gh CLI, last release reachable)
#   2. Bumps `version` in the Cargo.toml of agentic-cli (always) and
#      core-agentic (only when it has unreleased commits)
#   3. Runs the same checks as CI (fmt, clippy, tests)
#   4. Commits the version bump and creates the annotated tag
#   5. Pushes the branch and tag
#   6. Builds a release binary for the current platform
#   7. Creates the GitHub release with auto-generated notes + assets
#
# Conventions (kept consistent with past releases):
#   - tag:            vX.Y.Z            (annotated)
#   - tag message:    release: vX.Y.Z (agentic-cli X.Y.Z, core-agentic A.B.C)
#   - release title:  vX.Y.Z
#   - assets:         agentic-<os>-<arch> (+ .sha256)
#
# See docs/RELEASING.md for the full process.

set -euo pipefail

# ── Args ──────────────────────────────────────────────────
ASK_CONFIRM=true
BUMP=patch
for arg in "$@"; do
    case "$arg" in
        --yes | -y) ASK_CONFIRM=false ;;
        patch | minor | major) BUMP="$arg" ;;
        *) die "Unknown argument '$arg'. Use: [patch|minor|major] [--yes]" ;;
    esac
done

# ── Config ──────────────────────────────────────────────────
REPO="dchya24/agentic"
CLI_CRATE="agentic-cli"
CORE_CRATE="core-agentic"
DIST_DIR="dist"
RELEASE_BRANCHES=("dev" "main" "master")

# ── Helpers ─────────────────────────────────────────────────
info()  { printf "\033[1;34m▶\033[0m %s\n" "$*"; }
ok()    { printf "\033[1;32m✓\033[0m %s\n" "$*"; }
warn()  { printf "\033[1;33m!\033[0m %s\n" "$*"; }
die()   { printf "\033[1;31m✗\033[0m %s\n" "$*" >&2; exit 1; }

confirm() {
    # confirm "message" — asks yes/no; honors --yes via ASK_CONFIRM=false
    if [ "${ASK_CONFIRM:-true}" = "false" ]; then
        return 0
    fi
    local prompt="$1"
    local answer
    read -r -p "$prompt [y/N] " answer
    case "${answer,,}" in
        y | yes) return 0 ;;
        *) return 1 ;;
    esac
}

read_version() {
    # read_version <crate> → echoes "X.Y.Z"
    local crate="$1"
    awk '
        /^\[package\]/ { in_pkg = 1; next }
        /^\[/ { in_pkg = 0 }
        in_pkg && /^version[[:space:]]*=/ {
            gsub(/^version[[:space:]]*=[[:space:]]*"|".*$/, "")
            print
            exit
        }
    ' "${crate}/Cargo.toml"
}

bump_version() {
    # bump_version X.Y.Z patch|minor|major → echoes new version
    local ver="$1" kind="$2"
    local major minor patch
    major="${ver%%.*}"; rest="${ver#*.}"
    minor="${rest%%.*}"; patch="${rest#*.}"
    case "$kind" in
        major) major=$((major + 1)); minor=0; patch=0 ;;
        minor) minor=$((minor + 1)); patch=0 ;;
        *)     patch=$((patch + 1)) ;;
    esac
    echo "${major}.${minor}.${patch}"
}

set_version() {
    # set_version <crate> <old> <new> — replaces the [package] version line
    local crate="$1" old="$2" new="$3"
    local file="${crate}/Cargo.toml"
    local count
    count="$(grep -c "^version[[:space:]]*=[[:space:]]*\"${old}\"" "$file" || true)"
    [ "$count" -eq 1 ] || die "Expected exactly one 'version = \"${old}\"' line in ${file}, found ${count}"
    sed -i "s/^version[[:space:]]*=[[:space:]]*\"${old}\"/version = \"${new}\"/" "$file"
}

last_tag() {
    # last_tag → echoes the most recent release tag, or empty string
    git tag --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n 1 || true
}

changelog_since() {
    # changelog_since <tag> — grouped, conventional-commit release notes
    local tag="$1"
    local commits
    commits="$(git log "${tag}..HEAD" --format='%s' 2>/dev/null | grep -E '^(feat|fix|refactor|perf|docs|chore|style|test|build|ci|revert|breaking)(\(.*\))?[!: ]' || true)"
    [ -n "$commits" ] || { echo "(no conventional commits since ${tag})"; return; }
    {
        echo "### Features"
        echo "$commits" | grep '^feat' | sed 's/^feat\(([^)]*)\)\?[!]*: */  - /' || true
        echo
        echo "### Bug Fixes"
        echo "$commits" | grep '^fix' | sed 's/^fix\(([^)]*)\)\?[!]*: */  - /' || true
        echo
        echo "### Refactors & Performance"
        echo "$commits" | grep -E '^(refactor|perf)' | sed -E 's/^(refactor|perf)(\(([^)]*)\))?[!]*: */  - /' || true
        echo
        echo "### Other"
        echo "$commits" | grep -vE '^(feat|fix|refactor|perf)' | sed -E 's/^(docs|chore|style|test|build|ci|revert|breaking)(\(([^)]*)\))?[!]*: */  - /' || true
    } | grep -vE '^[[:space:]]*$' || true
}

# ── 1. Preflight ────────────────────────────────────────────
cd "$(dirname "$0")/.."
ROOT="$(pwd)"

info "Preflight checks…"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "Not inside a git repository"

BRANCH="$(git branch --show-current)"
case " ${RELEASE_BRANCHES[*]} " in
    *" ${BRANCH} "*) ;;
    *) warn "On branch '${BRANCH}' (expected one of: ${RELEASE_BRANCHES[*]}). Release tag will be cut here regardless." ;;
esac

if [ -n "$(git status --porcelain)" ]; then
    warn "Working tree has uncommitted changes:"
    git status --porcelain | sed 's/^/    /'
    confirm "Continue with uncommitted changes? (they will NOT be included)" \
        || die "Aborting. Commit or stash your changes first."
fi

command -v gh >/dev/null 2>&1 || die "gh CLI is required (https://cli.github.com)"
gh auth status >/dev/null 2>&1 || die "gh is not authenticated. Run: gh auth login"

PREV_TAG="$(last_tag || true)"
if [ -z "$PREV_TAG" ]; then
    warn "No previous release tag found — treating this as the first release."
else
    ok "Previous release: ${PREV_TAG}"
fi

# ── 2. Version bump ─────────────────────────────────────────
case "$BUMP" in
    patch | minor | major) ;;
    *) die "Unknown bump level '${BUMP}'. Use: patch | minor | major" ;;
esac

CLI_OLD="$(read_version "$CLI_CRATE")"
CORE_OLD="$(read_version "$CORE_CRATE")"
[ -n "$CLI_OLD" ] && [ -n "$CORE_OLD" ] || die "Could not read current versions"

CLI_NEW="$(bump_version "$CLI_OLD" "$BUMP")"

# Bump core-agentic only when it has unreleased commits. Keeps the two
# crates in lockstep when the core changed, and avoids churn otherwise.
CORE_NEW="$CORE_OLD"
if [ -n "$PREV_TAG" ] && [ -n "$(git log "${PREV_TAG}..HEAD" -- "${CORE_CRATE}/" 2>/dev/null)" ]; then
    CORE_NEW="$(bump_version "$CORE_OLD" "$BUMP")"
    info "core-agentic ${CORE_OLD} → ${CORE_NEW} (has unreleased commits)"
fi

TAG="v${CLI_NEW}"
if git rev-parse "$TAG" >/dev/null 2>&1; then
    die "Tag ${TAG} already exists."
fi

info "Releasing ${TAG}"
info "  ${CLI_CRATE}:  ${CLI_OLD} → ${CLI_NEW}"
info "  ${CORE_CRATE}: ${CORE_OLD} → ${CORE_NEW}"

confirm "Bump versions, commit and tag ${TAG}?" || die "Aborted."

set_version "$CLI_CRATE" "$CLI_OLD" "$CLI_NEW"
if [ "$CORE_NEW" != "$CORE_OLD" ]; then
    set_version "$CORE_CRATE" "$CORE_OLD" "$CORE_NEW"
fi
ok "Versions bumped in Cargo.toml"

# ── 3. Quality gates (mirrors CI) ───────────────────────────
info "Running quality gates…"
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
ok "fmt, clippy, tests passed"

# ── 4. Commit + tag ─────────────────────────────────────────
git add "${CLI_CRATE}/Cargo.toml" "${CORE_CRATE}/Cargo.toml" "Cargo.lock"
git commit -m "release: ${TAG} (${CLI_CRATE} ${CLI_NEW}, ${CORE_CRATE} ${CORE_NEW})"
ok "Committed version bump"

git tag -a "$TAG" -m "release: ${TAG} (${CLI_CRATE} ${CLI_NEW}, ${CORE_CRATE} ${CORE_NEW})"
ok "Created annotated tag ${TAG}"

confirm "Push ${BRANCH} and ${TAG} to origin?" || die "Aborted. Tag created locally: git push origin ${TAG}"
git push origin "$BRANCH"
git push origin "$TAG"
ok "Pushed ${BRANCH} and ${TAG}"

# ── 5. Build release binary for the current platform ────────
info "Building release binary…"
cargo build --release -p agentic-cli
mkdir -p "$DIST_DIR"

OS_NAME="$(uname -s | tr '[:upper:]' '[:lower:]')"
[ "$OS_NAME" = "darwin" ] && OS_NAME="macos"
ARCH_NAME="$(uname -m)"
ASSET="${DIST_DIR}/agentic-${OS_NAME}-${ARCH_NAME}"
cp target/release/agentic "$ASSET"
chmod +x "$ASSET"
(cd "$DIST_DIR" && sha256sum "agentic-${OS_NAME}-${ARCH_NAME}" > "agentic-${OS_NAME}-${ARCH_NAME}.sha256")
ok "Built ${ASSET}"

# ── 6. GitHub release ───────────────────────────────────────
NOTES_FILE="$(mktemp)"
{
    echo "## ${TAG}"
    echo
    echo "### What's changed since ${PREV_TAG:-the beginning}"
    echo
    changelog_since "${PREV_TAG:-HEAD}"
    echo
    echo "### Install"
    echo
    echo '```bash'
    echo "chmod +x agentic-${OS_NAME}-${ARCH_NAME}"
    echo "./agentic-${OS_NAME}-${ARCH_NAME}"
    echo '```'
    echo
    echo "Or update an existing install with: \`agentic update\`"
} > "$NOTES_FILE"

confirm "Create GitHub release ${TAG} with assets and publish?" || {
    warn "Skipped GitHub release. Publish manually:"
    warn "  gh release create ${TAG} ${ASSET} ${ASSET}.sha256 --title ${TAG} --notes-file ${NOTES_FILE}"
    rm -f "$NOTES_FILE"
    exit 0
}

gh release create "$TAG" \
    "${ASSET}" "${ASSET}.sha256" \
    --repo "$REPO" \
    --title "$TAG" \
    --notes-file "$NOTES_FILE"
rm -f "$NOTES_FILE"
ok "Published ${TAG} → https://github.com/${REPO}/releases/tag/${TAG}"

info "Release complete. Verify: gh release view ${TAG} --repo ${REPO}"
