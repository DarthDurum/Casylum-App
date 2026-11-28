# Casylum design

Casylum's interface is a work tool. People spend long stretches in it reading what the agent did and deciding what to allow, so it favours calm and clear over expressive. The brand shows in a few small details, not in decoration.

## Layout

Three flat columns, separated by 1px lines, with no floating cards:

- **Sidebar** (260px, a shade darker than the chat): wordmark, folder switcher, New chat, Commands, Projects (each folds open to its chats), the other chats, and Skills, MCP, Settings and theme at the bottom.
- **Chat**: a 52px header (project / chat title, token usage, panel toggle) above one centred column, at most 768px wide.
  - Your messages sit right-aligned in a tinted bubble. The agent's replies are plain text with no bubble.
  - Tool steps are a flat list of rows: icon, verb, target, then status. When a step expands, its output hangs under a hairline, like a transcript.
  - While a run is going, the last line says "Working for 12s", or "Waiting for your approval".
  - The composer is a rounded box: permission mode on the left, model and send on the right.
- **Side panel** (resizable, 440px by default): Plan, Changes, Files and Terminal tabs.

When the chat column is narrower than 600px, token usage hides and the composer controls wrap.

## Colour

Soft graphite neutrals with a faint violet cast and one periwinkle accent. Tokens are defined on `[data-theme]` in `src/styles.css`, and every component reads them.

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--canvas` | `#15161a` | `#eff0f3` | Sidebar, app background |
| `--bg` | `#1a1b20` | `#fafafb` | Chat and side panel |
| `--surface` | `#202128` | `#ffffff` | Composer, menus, modals |
| `--text` | `#e4e5ea` | `#1e2027` | Body text |
| `--muted` | `#858894` | `#666977` | Secondary labels (4.5:1 or better on `--bg` and `--canvas`) |
| `--accent` | `#a3a8f0` | `#5a60c8` | Primary actions, focus, the current item |
| `--accent-2` | `#8fd3dc` | `#2a8f9c` | Only in the logo ring and the working sweep |

Status colours keep fixed meanings: `--mint` for done and added lines, `--rose` for errors and removed lines, `--amber` for caution (Auto-edit mode, modified files). Use the accent for state: what is selected, current or waiting on you. Never use it as decoration.

## Type

- **Instrument Sans** (variable, weights 400 to 700, widths 75% to 100%) for all UI text. Chat text is 15px, chrome is 13 to 14px. The wordmark and headings use a condensed width (80 to 90%) to give them a voice without needing a second family.
- **Geist Mono** for code, paths, diffs, key caps and the terminal.
- Section labels are in sentence case. Don't use tracked all-caps labels.

## Motion

- One signature moment: while the agent works, a band of light travels the composer's edge (`--sweep`).
- Other motion only answers what you do: menus rise, chevrons rotate, rows highlight on hover.
- `prefers-reduced-motion` turns the sweep into a static accent border and cuts other animation short.

## Floor

- Every interactive element shows a visible focus ring (`:focus-visible`, the accent at 70%).
- Text contrast is AA in both themes.
- Status never relies on colour alone: each step also shows a check, spinner, cross or shield icon.
