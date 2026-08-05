# Agentic Public README and Installation Design

**Date:** 2026-08-05

## Goal

Create an English-language root `README.md` that introduces Agentic as a product, documents supported installation and configuration behavior, and presents a concise public limitations/roadmap list without duplicating the detailed internal roadmap.

## Scope

The README will cover:

- Product positioning and the two-crate workspace (`core-agentic` and `agentic-cli`).
- Major implemented capabilities: agent loop, tools, memory, safety, multi-provider support, MCP, skills, planner mode, interactive REPL, and full-screen TUI.
- Supported prebuilt release targets: Linux x86_64, Windows x86_64, macOS x86_64, and macOS aarch64.
- Download-first installation guidance for all supported platforms.
- One-liner installation as an optional convenience method.
- SHA-256 checksum verification guidance.
- User-local installation without administrator privileges.
- Explicit configuration initialization via `agentic config init --interactive`; normal installation and upgrades must not create, overwrite, or delete configuration.
- Platform-specific configuration paths.
- Quick-start commands and links to detailed documentation.
- Public known limitations and TODOs, including Linux aarch64 prebuilt support being unavailable.

The README will not contain the complete task tracker or duplicate detailed architecture and release procedures.

## Installation and Configuration Contract

Installation scripts will install release binaries into user-local directories:

- Linux/macOS: `${AGENTIC_INSTALL_DIR:-$HOME/.local/bin}`.
- Windows: `%LOCALAPPDATA%\\Programs\\agentic\\bin`.

The scripts may update the current user's `PATH`, but must not require administrator privileges or modify system-wide configuration. The installer must preserve existing binaries/configuration unless the user explicitly requests an installation update.

Configuration remains separate from binary installation:

- Linux/macOS: `~/.config/agentic/config.json`.
- Windows: `%USERPROFILE%\\.config\\agentic\\config.json`.

The default install path does not run the configuration wizard. An explicit `--init` option for the shell installer or `-Init` for PowerShell may invoke `agentic config init --interactive` after a successful install. Existing configuration remains protected by the CLI's confirmation prompt.

## Supported Platforms

The initial installer/release matrix supports:

| OS | Architecture | Status |
|---|---|---|
| Linux | x86_64 | Supported |
| Linux | aarch64 | Not supported; installer exits with an actionable message |
| Windows | x86_64 | Supported |
| macOS | x86_64 | Supported |
| macOS | aarch64 | Supported |

The README must not imply Linux aarch64 support until a corresponding release artifact exists.

## Documentation Links

The README will link to the existing detailed documents:

- `agentic-cli/README.md` for CLI command details.
- `docs/CONFIGURATION.md` for configuration schema and options.
- `docs/ROADMAP.md` for the full implementation roadmap.
- `docs/RELEASING.md` for maintainer release procedures.
- `AGENT_ARCHITECTURE.md` for the architecture overview.
- `CONTRIBUTING.md` and `SECURITY.md`.

## Out of Scope

- Implementing installer scripts in this README-only design step.
- Adding package-manager formulas or manifests.
- Claiming code signing, Linux aarch64 artifacts, or shell completion packages before they exist.
- Changing the existing configuration schema.

## Acceptance Criteria

- A new reader can understand what Agentic does without opening another file.
- A supported-platform user can find the installation path and next configuration command.
- The README clearly states that installation does not alter existing configuration.
- Linux aarch64 is explicitly listed as unsupported.
- All documented commands and links correspond to repository behavior or are clearly marked as planned.
- The detailed roadmap remains the source of truth for implementation status.
