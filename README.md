# Casylum

A calm, fast desktop AI coding agent. Bring your own API keys, open a folder, and ask for anything: Casylum reads your code, plans, makes edits you can review, and runs commands.

![Casylum workspace](docs/screenshots/workspace-dark.png)

## Why it feels fast

- **Tauri 2 + Rust core.** Uses the system webview, so the app is small and light on memory. The agent loop, model streaming, file search, edits and the terminal all run in Rust.
- **Lean UI.** React with CodeMirror 6 (not Monaco) for code and diffs, xterm.js for the terminal, and off-screen chat messages skip rendering.

## Features

- **Bring your own keys:** Anthropic, OpenAI, Google Gemini, OpenRouter, Groq, Abacus.AI (RouteLLM), Mistral, DeepSeek, xAI (Grok), Together AI, Fireworks AI, Cerebras, Perplexity (chat only), Cohere, DeepInfra, Hugging Face, SambaNova, Nebius, Novita, Moonshot (Kimi), Z.ai (GLM), Alibaba Qwen, Ollama and LM Studio (local), and any OpenAI-compatible endpoint. Model lists are fetched live from each provider. Keys live in your OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux) and never reach the UI layer.
- **Agent with real tools:** read, list, search (ripgrep engine), edit and create files, run commands, keep a live plan, load skills.
- **You stay in control:** three permission modes (Ask first, Auto-edit, Full auto), inline approve or deny with a preview of the exact diff or command, a Stop button that leaves the conversation in a clean state, and **Undo this run** to restore every file the agent touched.
- **Calm workspace:** tidy step cards instead of log walls, a side panel for Plan, Changes (diffs), Files and Terminal, a command palette (`Ctrl/Cmd+K`), dark and light themes.
- **MCP servers:** connect Model Context Protocol servers, local (stdio: `npx`, `uvx`, `docker`, ...) or remote (Streamable HTTP, with automatic fallback to legacy SSE). Their tools reach the agent as `mcp__<server>__<tool>` with every provider that supports tool calling. The config is the same `mcpServers` JSON Claude Desktop and Claude Code use, so you can paste yours, or import it in one click from Claude Desktop, Claude Code, Cursor or Windsurf. Manage servers in Settings > MCP servers (status, tools, logs, reconnect). Tools the server marks read-only run without asking; others follow your permission mode unless you set the server to auto-approve.
  - Config file: `mcp.json` in the app's config folder. Extra keys: `"disabled": true`, `"autoApprove": true` (or a list of tool names), `"timeout"` (seconds per call), `"cwd"`. `${VAR}` and `${VAR:-default}` read environment variables.
- **Projects:** group chats under a project with an optional folder (new chats start there) and instructions the agent follows in every chat of the project. Move a chat between projects from the chat header.
- **Skills:** folders with a `SKILL.md` (name + description frontmatter), compatible with Claude Code-style skills. Only descriptions go into the prompt; the agent loads a skill's full instructions when a task needs it. Manage them in Settings > Skills, and install skill packages from a `.zip` or `.skill` file (one skill or many per archive).
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
2. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`, or run the Release workflow from the Actions tab (it tags the commit for you)

The [Release workflow](.github/workflows/release.yml) builds installers for Windows, macOS and Linux, attaches them to a GitHub Release and publishes it. Installed apps then see the update.

In-app updates are signed. The public key is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`); the matching private key and its password are stored as the repo secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (Settings, Secrets and variables, Actions). Keep a private backup of the key: losing it means existing installs can't update. To rotate it, run `pnpm tauri signer generate -w ~/.tauri/casylum.key`, put the new `.pub` contents in `tauri.conf.json` and update both secrets.

Without the secrets, releases still build installers but aren't offered as in-app updates.

## Layout

```
src-tauri/src/
  agent.rs        agent loop, approvals, sessions, undo checkpoints
  providers/      anthropic.rs, openai.rs (every OpenAI-compatible provider), gemini.rs, SSE reader
  tools.rs        read/list/search/write/edit/run/plan/skill tools, sandboxed to the project
  skills.rs       skill discovery, loading, editing and .zip import
  mcp/            MCP client (stdio, Streamable HTTP, SSE), server manager, mcp.json config
  projects.rs     projects: groups of chats with a folder and instructions
  keys.rs         OS keychain storage
  terminal.rs     PTY-backed terminal
  settings.rs     non-secret settings
src/
  components/     Rail, Chat, StepCard, Composer, ContextPanel, Settings, Skills, Mcp, ProjectModal, Palette, Onboarding
  store.ts        app state and agent event handling
  lib/            typed IPC bridge, editor theme, step helpers
```

## License

Apache 2.0
