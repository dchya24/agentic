# Cross-Platform Installer and Public README Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add user-local installers for supported Linux, macOS, and Windows releases, document their safe configuration behavior, and create an English root README that accurately describes Agentic and its current platform support.

**Architecture:** The installers are thin release clients. `scripts/install.sh` handles Linux/macOS using POSIX shell, while `scripts/install.ps1` handles Windows using PowerShell. Each script determines the current OS/architecture, selects the matching GitHub Release asset, downloads the archive and platform checksum manifest, verifies SHA-256 before replacing the binary, installs into a user-local directory, and optionally runs the existing CLI wizard only when explicitly requested. The root README documents the same contract and links to detailed repository docs; it does not duplicate the full roadmap.

**Tech Stack:** POSIX shell (`sh`, `curl`, `tar`, `sha256sum`/`shasum`), PowerShell 5.1+ / PowerShell 7, GitHub Releases, SHA-256 checksum manifests, Markdown.

## Global Constraints

- Linux x86_64, Windows x86_64, macOS x86_64, and macOS aarch64 are supported release targets.
- Linux aarch64 is unsupported; the installer must exit with an actionable message and non-zero status.
- Linux/macOS install default: `${AGENTIC_INSTALL_DIR:-$HOME/.local/bin}`.
- Windows install default: `%LOCALAPPDATA%\\Programs\\agentic\\bin`.
- Installers must not require administrator privileges or modify system-wide `PATH`.
- Installers may update the current user's `PATH` and must explain that a new shell may be required.
- Installers must preserve existing configuration at `~/.config/agentic/config.json` on Linux/macOS and `%USERPROFILE%\\.config\\agentic\\config.json` on Windows.
- The default install must not run the configuration wizard; `--init`/`-Init` is the only opt-in path to `agentic config init --interactive`.
- Existing binaries are replaced only after a successful download and checksum verification.
- Checksum verification is mandatory; no unauthenticated fallback is allowed.
- Version selection supports the latest release by default and an explicit version through `AGENTIC_VERSION` (POSIX) or `-Version` (PowerShell), accepting `vX.Y.Z` and normalizing the release URL consistently.
- The repository's GitHub owner/repository is `dchya24/agentic`.
- No installer test may contact the network or alter the developer's real home directory or PATH.

---

### Task 1: Define Installer Test Fixtures and Shared Contract

**Files:**
- Create: `scripts/tests/fixtures/README.md`
- Create: `scripts/tests/fixtures/release/agentic-linux-x86_64.tar.gz`
- Create: `scripts/tests/fixtures/release/agentic-macos-x86_64.tar.gz`
- Create: `scripts/tests/fixtures/release/agentic-windows-x86_64.zip`
- Create: `scripts/tests/fixtures/release/checksums-linux.txt`
- Create: `scripts/tests/fixtures/release/checksums-macos.txt`
- Create: `scripts/tests/fixtures/release/checksums-windows.txt`
- Create: `scripts/tests/installer_test.sh`

**Interfaces:**
- Produces a local fixture layout that mirrors the GitHub Release asset names and checksum manifests consumed by both installers.
- Provides shell assertions and temporary `HOME`, `AGENTIC_INSTALL_DIR`, and PATH files for deterministic POSIX installer tests.

- [x] **Step 1: Write the failing fixture test**

Create `scripts/tests/installer_test.sh` with tests that invoke the not-yet-created POSIX installer through a local fixture base URL or injectable download command. Cover:

```sh
assert_status 1 "unsupported linux aarch64"
assert_status 0 "linux x86_64 installs verified binary"
assert_status 1 "checksum mismatch does not replace existing binary"
assert_status 0 "explicit version is selected"
assert_status 0 "existing config is preserved"
assert_status 0 "default install does not invoke config init"
```

Use `mktemp -d`, a temporary `HOME`, and fixture files. Never use the real `$HOME` or mutate the caller's shell environment.

- [x] **Step 2: Run the test to verify it fails**

Run:

```bash
bash scripts/tests/installer_test.sh
```

Expected: FAIL because `scripts/install.sh` and its testable download/fixture hooks do not yet exist.

- [x] **Step 3: Define deterministic fixture generation**

Add a fixture setup section that creates tiny executable marker binaries and archives them with the exact release names expected by the installer. Generate checksum files using the host's available `sha256sum` or `shasum -a 256`. Keep fixture generation inside the test script or a clearly documented helper so binary fixtures are not hand-maintained.

- [x] **Step 4: Run fixture tests after setup**

Run:

```bash
bash scripts/tests/installer_test.sh
```

Expected: still FAIL at installer behavior assertions, while fixture creation and checksum generation succeed.

---

### Task 2: Implement POSIX Installer for Linux and macOS

**Files:**
- Create: `scripts/install.sh`
- Modify: `scripts/tests/installer_test.sh`

**Interfaces:**
- Command: `sh scripts/install.sh [--init]`.
- Environment: `AGENTIC_VERSION`, `AGENTIC_INSTALL_DIR`, `AGENTIC_RELEASE_BASE_URL`, and optional `AGENTIC_DOWNLOAD_CMD` test hook.
- Produces an executable `agentic` at the selected user-local install directory and an optional user PATH update.

- [x] **Step 1: Implement platform and architecture detection**

Use `uname -s` and `uname -m`:

```sh
case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *) fail "Unsupported operating system" ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch=x86_64 ;;
  arm64|aarch64)
    [ "$os" = macos ] && arch=aarch64 || fail "Linux aarch64 is not supported yet" ;;
  *) fail "Unsupported architecture" ;;
esac
```

Map the selected target to the existing release archive names:

```text
linux/x86_64   -> agentic-linux-x86_64.tar.gz
macos/x86_64   -> agentic-macos-x86_64.tar.gz
macos/aarch64  -> agentic-macos-aarch64.tar.gz
```

- [x] **Step 2: Implement release/version resolution**

Normalize `AGENTIC_VERSION` by adding `v` when the user provides `0.3.2`, validate `^v[0-9]+\.[0-9]+\.[0-9]+$`, and default to the GitHub `latest/download` endpoint. For explicit versions use `/releases/download/<tag>/`. Keep the repository constants near the top of the script.

- [x] **Step 3: Implement secure download and checksum verification**

Download the archive and platform checksum manifest into a temporary directory with `curl --fail --location --silent --show-error`. Select exactly the checksum line for the selected archive, calculate the local SHA-256, and compare it byte-for-byte. Abort before touching the destination if the checksum is missing or mismatched.

Support both `sha256sum` and `shasum -a 256`, and fail clearly if neither is installed. Use `umask 077` for temporary files.

- [x] **Step 4: Implement atomic user-local installation**

Create the destination directory, extract into a temporary staging directory, verify that the extracted `agentic` file exists, mark it executable, then move it into the destination. Preserve configuration and unrelated files. A failed extraction or verification must leave the existing installed binary untouched.

- [x] **Step 5: Implement PATH guidance and explicit initialization**

If the destination is not already present in `PATH`, print shell-specific export instructions. Do not edit shell startup files automatically in the first implementation. After installation, when `--init` is present, execute the newly installed binary using `agentic config init --interactive`; otherwise print the command as the next step. Do not run the wizard by default.

- [x] **Step 6: Run POSIX tests to verify green behavior**

Run:

```bash
bash scripts/tests/installer_test.sh
```

Expected: all fixture-based tests pass, including unsupported Linux aarch64, checksum failure, config preservation, version selection, and opt-in initialization.

- [x] **Step 7: Run shell lint when available**

Run:

```bash
if command -v shellcheck >/dev/null 2>&1; then shellcheck scripts/install.sh scripts/tests/installer_test.sh; fi
```

Expected: no ShellCheck errors when the tool is installed.

---

### Task 3: Implement Windows PowerShell Installer

**Files:**
- Create: `scripts/install.ps1`
- Create: `scripts/tests/install.ps1.tests.md`
- Modify: `scripts/tests/installer_test.sh`

**Interfaces:**
- Command: `.\\scripts\\install.ps1 [-Init] [-Version <tag>] [-InstallDir <path>]`.
- Environment: `AGENTIC_VERSION`, `AGENTIC_INSTALL_DIR`, `AGENTIC_RELEASE_BASE_URL` for automation and fixture testing.
- Produces `%LOCALAPPDATA%\\Programs\\agentic\\bin\\agentic.exe` by default and updates the current user's persistent PATH only when needed.

- [x] **Step 1: Specify PowerShell test cases**

Document and structure tests for:

```text
x64 Windows installs the .zip after checksum verification
checksum mismatch preserves the existing agentic.exe
ARM64 and non-x64 Windows fail with an actionable message
existing config remains untouched
default mode does not invoke config init
-Init invokes config init after installation
```

Tests must use a temporary `InstallDir`, temporary release fixture URL, and a stub executable for the `-Init` path. They must not change the real user's registry PATH.

- [x] **Step 2: Implement platform validation and parameters**

Use `[System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture` and accept only `X64`. Define parameters `[switch]$Init`, `[string]$Version`, and `[string]$InstallDir`. Reject non-Windows execution and unsupported architectures with a non-zero exit.

- [x] **Step 3: Implement release download and checksum verification**

Select `agentic-windows-x86_64.zip` and `checksums-windows.txt`. Download with `Invoke-WebRequest`, parse the exact checksum entry, compute the archive hash with `Get-FileHash -Algorithm SHA256`, and compare case-insensitively. Do not overwrite an existing executable before all checks pass.

- [x] **Step 4: Implement safe extraction and replacement**

Extract to a temporary staging directory using `Expand-Archive`, verify `agentic-windows-x86_64.exe` exists, then move it atomically to `agentic.exe` in the destination. Create the destination without elevation. Preserve `%USERPROFILE%\\.config\\agentic\\config.json`.

- [x] **Step 5: Implement user PATH update and `-Init`**

Read the current user's persistent PATH with `[Environment]::GetEnvironmentVariable('Path', 'User')`. Add the install directory only if missing, write back only to the User scope, and print that new PowerShell sessions are required. With `-Init`, invoke the installed executable with `config init --interactive`; without it, print that command as the next step.

- [x] **Step 6: Validate PowerShell syntax and behavior where available**

Run on a Windows or PowerShell-capable environment:

```powershell
pwsh -NoProfile -Command "& { [void][System.Management.Automation.Language.Parser]::ParseFile('scripts/install.ps1',[ref]$null,[ref]$null) }"
```

Then run the documented fixture tests using temporary paths. On environments without PowerShell, record that syntax/runtime verification requires Windows or `pwsh` and still run the POSIX test suite.

---

### Task 4: Create the English Root README

**Files:**
- Create: `README.md`
- Modify: `agentic-cli/README.md`
- Modify: `docs/RELEASING.md`
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Public documentation must match the installer commands, release asset names, config paths, and supported platform matrix implemented in Tasks 1–3.
- Release notes must link users to the installer scripts and checksum assets without claiming unsupported targets.

- [x] **Step 1: Write README acceptance checks**

Before authoring prose, define grep-based checks in the documentation review command:

```bash
rg -n "Linux ARM64|aarch64|checksums|config init --interactive|does not.*config|AGENTIC_INSTALL_DIR|Windows|macOS" README.md
```

The checks must confirm the README states the supported targets, Linux aarch64 limitation, checksum verification, user-local paths, explicit initialization, and config preservation.

- [x] **Step 2: Write product overview and feature summary**

Describe Agentic as a Rust workspace with `core-agentic` and `agentic-cli`. Summarize implemented capabilities without copying the full internal roadmap: agent loop, tools, memory, safety controls, providers, MCP, skills, planner mode, REPL, and TUI.

- [x] **Step 3: Write installation documentation**

Document download-first as the primary flow, including platform asset selection, checksum verification, user-local install behavior, and the optional one-liners:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/dchya24/agentic/dev/scripts/install.sh -o install.sh
sh install.sh
```

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/dchya24/agentic/dev/scripts/install.ps1 -OutFile install.ps1
.\\install.ps1
```

Use the release/tag URL or installer constants selected in Task 2/3, and mark branch-based script URLs as examples only if the repository's release process requires a tagged script URL.

- [x] **Step 4: Write configuration and quick-start sections**

Document both platform config paths, the fact that install/upgrade does not create, overwrite, or delete config, and the explicit command:

```text
agentic config init --interactive
```

Include API key environment-variable examples and basic `agentic run`, `agentic interactive`, and `agentic tui` commands.

- [x] **Step 5: Write support matrix, limitations, and links**

List Linux x86_64, Windows x86_64, macOS x86_64, and macOS aarch64 as supported. List Linux aarch64 as unsupported. Keep TODOs concise and link to `docs/ROADMAP.md` for the authoritative detailed list. Link architecture, configuration, release, contributing, security, and license documentation.

- [x] **Step 6: Align package-specific and release docs**

Update `agentic-cli/README.md` so its installation section points to the root installer documentation and no longer presents `cargo install` as the only user installation path. Update `docs/RELEASING.md` with installer asset expectations and local verification guidance. Update release notes in `.github/workflows/release.yml` to mention the script-based installation path and the explicit config initialization behavior.

- [x] **Step 7: Validate documentation links and claims**

Run:

```bash
rg -n "docs/|agentic-cli/README|CONTRIBUTING|SECURITY|LICENSE" README.md
rg -n "aarch64|x86_64|checksums|config init|install\.sh|install\.ps1" README.md agentic-cli/README.md docs/RELEASING.md .github/workflows/release.yml
```

Review every referenced path and command against the repository and installer implementation.

---

### Task 5: End-to-End Verification and Review

**Files:**
- Modify: `scripts/tests/installer_test.sh` if test fixes are required.
- Modify: `README.md` or documentation files only when verification finds a stale claim.

**Interfaces:**
- Confirms the installer scripts, docs, and existing Rust workspace are consistent.

- [x] **Step 1: Run the POSIX installer fixture suite**

```bash
bash scripts/tests/installer_test.sh
```

Expected: all tests pass without network access or writes outside temporary directories.

- [x] **Step 2: Run shell formatting/lint checks**

```bash
if command -v shellcheck >/dev/null 2>&1; then shellcheck scripts/install.sh scripts/tests/installer_test.sh; fi
```

Expected: no reported shell errors when ShellCheck is available.

- [x] **Step 3: Run repository formatting and tests**

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
```

Expected: both commands exit successfully.

- [x] **Step 4: Check repository diff and accidental files**

```bash
git diff --check
git status --short
git diff --stat
```

Confirm that only installer scripts, installer tests/fixtures, README/documentation, and intentionally updated workflow files are present.

- [x] **Step 5: Perform final support-matrix review**

Verify each matrix entry against `.github/workflows/release.yml`, confirm Linux aarch64 fails before download, confirm checksum manifests contain the archive names used by scripts, and confirm no default path invokes `config init`.

- [x] **Step 6: Commit the implementation**

```bash
git add README.md agentic-cli/README.md docs/RELEASING.md scripts/install.sh scripts/install.ps1 scripts/tests .github/workflows/release.yml
git commit -m "feat: add cross-platform installers and product README"
```

Do not include generated binaries, real user configuration, logs, or unrelated worktree changes.
