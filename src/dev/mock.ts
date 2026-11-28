// Browser preview: fakes the Rust core so the UI can be developed and
// screenshotted with `pnpm dev` in a normal browser. Never used in the app.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { Message, Settings } from "../lib/api";

const before = `export function total(items: Item[]) {
  let sum = 0;
  for (const i of items) sum += i.price;
  return sum;
}
`;
const after = `export function total(items: Item[], taxRate = 0) {
  const subtotal = items.reduce((sum, i) => sum + i.price * i.qty, 0);
  return Math.round(subtotal * (1 + taxRate) * 100) / 100;
}
`;

let messages: Message[] = [
  {
    role: "user",
    content: [
      { type: "text", text: "The cart total ignores quantity and tax. Fix it and add a test." },
      { type: "file", name: "bug-report.txt", text: "Cart shows $10 for 3 items at $10 each." },
    ],
  },
  {
    role: "assistant",
    content: [
      { type: "thinking", thinking: "Find where totals are computed, check the tests, then patch.", signature: "x" },
      { type: "text", text: "I'll find where the total is computed and check the existing tests first." },
      { type: "tool_use", id: "t0", name: "update_plan", input: { items: [
        { text: "Find the cart total logic", description: "Search for where the cart total is computed and read the existing tests.", status: "done" },
        { text: "Fix quantity and tax handling", description: "Multiply each line by its quantity, apply an optional tax rate and round to cents.", status: "done" },
        { text: "Add a regression test", description: "Cover multiple quantities and a non-zero tax rate in src/cart/total.test.ts.", status: "in_progress" },
        { text: "Run the test suite", description: "Run vitest on src/cart and fix anything that fails.", status: "pending" },
      ] } },
      { type: "tool_use", id: "t1", name: "search", input: { pattern: "function total", glob: "**/*.ts" } },
      { type: "tool_use", id: "t2", name: "read_file", input: { path: "src/cart/total.ts" } },
      { type: "tool_use", id: "t3", name: "edit_file", input: { path: "src/cart/total.ts", old_string: before, new_string: after } },
    ],
  },
  {
    role: "user",
    content: [
      { type: "tool_result", tool_use_id: "t0", content: "Plan updated.", is_error: false },
      { type: "tool_result", tool_use_id: "t1", content: "1 matches in 1 files\nsrc/cart/total.ts:1: export function total(items: Item[]) {", is_error: false },
      { type: "tool_result", tool_use_id: "t2", content: before, is_error: false },
      { type: "tool_result", tool_use_id: "t3", content: "Edited src/cart/total.ts (1 replacement).", is_error: false, meta: { kind: "diff", path: "src/cart/total.ts", before, after, created: false, added: 4, removed: 5 } },
    ],
  },
  {
    role: "assistant",
    content: [
      { type: "text", text: "The total now multiplies by **quantity** and applies an optional `taxRate`, rounded to cents. Next I'll add a test and run the suite." },
      { type: "tool_use", id: "t4", name: "run_command", input: { command: "pnpm vitest run src/cart" } },
    ],
  },
];

let settings: Settings = {
  provider: "anthropic",
  model: "claude-opus-5-5",
  permission_mode: "ask",
  theme: (new URLSearchParams(location.search).get("theme") as any) ?? "dark",
  ollama_base_url: "http://localhost:11434/v1",
  custom_base_url: "",
  recent_projects: ["/Users/you/code/shopfront"],
  disabled_skills: [],
  auto_update: true,
};

export function installMock() {
  localStorage.setItem("onboarded", new URLSearchParams(location.search).has("onboard") ? "0" : "1");
  mockWindows("main");
  mockIPC(
    (cmd, args: any) => {
      switch (cmd) {
        case "get_settings":
          return settings;
        case "save_settings":
          settings = args.settings;
          return;
        case "list_providers":
          return [
            { id: "anthropic", name: "Anthropic", needs_key: true, models: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"], key_url: "", has_key: !new URLSearchParams(location.search).has("onboard"), key_hint: "sk-…9f2c" },
            { id: "openai", name: "OpenAI", needs_key: true, models: ["gpt-5", "gpt-5-mini"], key_url: "", has_key: false, key_hint: null },
            { id: "gemini", name: "Google Gemini", needs_key: true, models: ["gemini-2.5-pro"], key_url: "", has_key: false, key_hint: null },
            { id: "openrouter", name: "OpenRouter", needs_key: true, models: [], key_url: "", has_key: false, key_hint: null },
            { id: "ollama", name: "Ollama (local)", needs_key: false, models: ["qwen3-coder"], key_url: "", has_key: false, key_hint: null },
          ];
        case "list_models": {
          const lists: Record<string, Array<{ id: string; name?: string }>> = {
            anthropic: [
              { id: "claude-opus-5-5", name: "Claude Opus 5.5" },
              { id: "claude-sonnet-5-5", name: "Claude Sonnet 5.5" },
              { id: "claude-haiku-5-5", name: "Claude Haiku 5.5" },
            ],
            gemini: [{ id: "gemini-4-argon", name: "Gemini 4 Argon" }, { id: "gemini-2.5-pro", name: "Gemini 2.5 Pro" }],
          };
          return { provider: args.provider, models: lists[args.provider] ?? [], source: "live", fetched_at: Date.now(), error: null };
        }
        case "set_messages":
          messages = args.messages;
          return;
        case "read_attachment":
          return { type: "file", name: String(args.path).split("/").pop(), text: "" };
        case "agent_running":
          return [];
        case "list_sessions":
          return [
            { id: "s1", project: settings.recent_projects[0], title: "Fix cart total with quantity and tax", updated_at: Date.now() - 60_000, model: "claude-opus-5-5" },
            { id: "s2", project: settings.recent_projects[0], title: "Explain the checkout flow", updated_at: Date.now() - 3_600_000 * 5, model: "claude-sonnet-5-5" },
            { id: "s3", project: settings.recent_projects[0], title: "Add dark mode to settings page", updated_at: Date.now() - 86_400_000 * 2, model: "gpt-5" },
          ];
        case "load_session":
          return { id: args.id, project: settings.recent_projects[0], title: "", created_at: 0, updated_at: 0, provider: "anthropic", model: "claude-opus-5-5", messages: args.id === "s1" ? messages : [], runs: [], usage: { input_tokens: 48210, output_tokens: 3120 } };
        case "skills_list":
          return [
            { name: "release-notes", description: "Write release notes from merged changes", scope: "project", dir: "/x/.casylum/skills/release-notes", enabled: true },
            { name: "react-review", description: "Review React components for accessibility and performance", scope: "global", dir: "/home/.casylum/skills/react-review", enabled: true },
          ];
        case "skills_paths":
          return { global: "~/.casylum/skills", project: "shopfront/.casylum/skills" };
        case "skill_read":
          return "---\nname: release-notes\ndescription: Write release notes from merged changes\n---\n\n# Release notes\n\n1. Read the git log since the last tag.\n2. Group changes by area.\n";
        case "fs_list":
          return [
            { name: "src", path: "src", is_dir: true },
            { name: "package.json", path: "package.json", is_dir: false },
            { name: "README.md", path: "README.md", is_dir: false },
          ];
        case "plugin:event|listen":
          return 1;
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
