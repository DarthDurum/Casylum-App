# Casylum

A calm, fast desktop AI coding agent. Bring your own API keys, open a folder, and ask for anything: Casylum reads your code, plans, makes edits you can review, and runs commands.

![Casylum workspace](docs/screenshots/workspace-dark.png)

## Why it feels fast

- **Tauri 2 + Rust core.** Uses the system webview, so the app is small and light on memory. The agent loop, model streaming, file search, edits and the terminal all run in Rust.
- **Lean UI.** React with CodeMirror 6 (not Monaco) for code and diffs, xterm.js for the terminal, and off-screen chat messages skip rendering.

## Features

- **Bring your own keys:** Anthropic, OpenAI, Google Gemini, OpenRouter, Ollama (local) and any OpenAI-compatible endpoint. Keys live in your OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux) and never reach the UI layer.
- **Agent with real tools:** read, list, search (ripgrep engine), edit and create files, run commands, keep a live plan, load skills.
- **You stay in control:** three permission modes (Ask first, Auto-edit, Full auto), inline approve or deny with a preview of the exact diff or command, a Stop button that leaves the conversation in a clean state, and **Undo this run** to restore every file the agent touched.
- **Calm workspace:** tidy step cards instead of log walls, a side panel for Plan, Changes (diffs), Files and Terminal, a command palette (`Ctrl/Cmd+K`), dark and light themes.
- **Skills:** folders with a `SKILL.md` (name + description frontmatter), compatible with Claude Code-style skills. Only descriptions go into the prompt; the agent loads a skill's full instructions when a task needs it. Manage them in the Skills panel.
  - Global: `~/.casylum/skills/<name>/SKILL.md`
  - Per project: `<project>/.casylum/skills/<name>/SKILL.md`
- **Project instructions:** a `CASYLUM.md`, `AGENTS.md` or `CLAUDE.md` at the project root is added to the agent's instructions.
- **Chats persist** per project in the app's config folder.

## Install

Download the installer for your system from the [latest release](https://github.com/DarthDurum/Casylum-App/releases/latest): `.exe` or `.msi` for Windows, `.dmg` for macOS (Apple Silicon `aarch64` or Intel `x64`), `.AppImage`, `.deb` or `.rpm` for Linux.

Or let a one-line installer fetch the latest release from GitHub:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/DarthDurum/Casylum-App/main/scripts/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/DarthDurum/Casylum-App/main/scripts/install.ps1 | iex
```

Casylum checks GitHub for new versions when it starts and offers to update itself (also under Settings, General, Check for updates). macOS builds aren't notarized yet: if macOS says the app can't be opened, right-click it and choose Open once (the install script does this for you).

## Develop

Prerequisites: Node 20+, pnpm, Rust (stable), and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```bash
pnpm install
pnpm tauri dev        # run the desktop app
pnpm tauri build      # make an installer for your OS
```

UI-only work in a normal browser: `pnpm dev` and open http://localhost:1420. Outside Tauri the app uses a mock core (`src/dev/mock.ts`) with a sample conversation. Add `?theme=light` or `?onboard=1` to the URL to preview those states.

Tests: `cargo test --manifest-path src-tauri/Cargo.toml` covers path sandboxing, edits and undo, commands, skill parsing, and streaming for the Anthropic and OpenAI-compatible adapters.

## Release

1. Set the version everywhere and commit: `pnpm version:set 0.2.0`
2. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`

The [Release workflow](.github/workflows/release.yml) builds installers for Windows, macOS and Linux, attaches them to a GitHub Release and publishes it. Installed apps then see the update.

In-app updates are signed, so set this up once before the first release:

1. Make a signing key on your computer: `pnpm tauri signer generate -w ~/.tauri/casylum.key` (pick a password). Keep the key file private and backed up; losing it means existing installs can't update.
2. In the repo on GitHub, open Settings, Secrets and variables, Actions, and add:
   - Secret `TAURI_SIGNING_PRIVATE_KEY`: the contents of `~/.tauri/casylum.key`
   - Secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password
   - Variable (Variables tab) `TAURI_UPDATER_PUBKEY`: the contents of `~/.tauri/casylum.key.pub`

Without these, releases still build installers but aren't offered as in-app updates.

## Layout

```
src-tauri/src/
  agent.rs        agent loop, approvals, sessions, undo checkpoints
  providers/      anthropic.rs, openai.rs (also OpenRouter/Ollama/custom), gemini.rs, SSE reader
  tools.rs        read/list/search/write/edit/run/plan/skill tools, sandboxed to the project
  skills.rs       skill discovery, loading and editing
  keys.rs         OS keychain storage
  terminal.rs     PTY-backed terminal
  settings.rs     non-secret settings
src/
  components/     Rail, Chat, StepCard, Composer, ContextPanel, Settings, Skills, Palette, Onboarding
  store.ts        app state and agent event handling
  lib/            typed IPC bridge, editor theme, step helpers
```

## License

Apache 2.0
