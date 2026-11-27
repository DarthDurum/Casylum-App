// Model names for people: "gemini-4-argon" -> "Gemini 4 Argon",
// "claude-opus-5-5" -> "Claude Opus 5.5", "gpt-5-mini" -> "GPT-5 Mini".
// Providers that return a display name (Anthropic, Gemini, OpenRouter) use
// theirs; everything else is prettified from the id.

export interface ModelInfo {
  id: string;
  name?: string;
}

export interface ModelList {
  provider: string;
  models: ModelInfo[];
  source: "live" | "cache" | "fallback";
  fetched_at: number | null;
  error: string | null;
}

const WORDS: Record<string, string> = {
  gpt: "GPT",
  oss: "OSS",
  ai: "AI",
  api: "API",
  llm: "LLM",
  vl: "VL",
  moe: "MoE",
  it: "IT",
  hd: "HD",
  r1: "R1",
  v3: "V3",
  chatgpt: "ChatGPT",
  deepseek: "DeepSeek",
  openai: "OpenAI",
  xai: "xAI",
  phi: "Phi",
  exp: "Experimental",
};

const isNum = (t: string) => /^\d{1,2}$/.test(t);

function word(t: string): string {
  const lower = t.toLowerCase();
  if (WORDS[lower]) return WORDS[lower];
  // OpenAI's reasoning models keep their lowercase names: o3, o4.
  if (/^o\d+$/.test(lower)) return lower;
  // Sizes and versions: 70b, 8x7b, 4o, 2.5
  if (/^\d/.test(t)) return lower;
  return t.charAt(0).toUpperCase() + t.slice(1);
}

export function prettifyModelId(id: string): string {
  if (!id) return id;
  // "anthropic/claude-sonnet-5-5" -> "claude-sonnet-5-5"; "qwen3-coder:30b" keeps the tag.
  let base = id.split("/").pop() ?? id;
  let tag = "";
  const colon = base.indexOf(":");
  if (colon >= 0) {
    tag = base.slice(colon + 1);
    base = base.slice(0, colon);
  }
  const raw = base.split(/[-_\s]+/).filter(Boolean);
  // Drop date stamps like 20250514 or 2024-08-06, and "latest".
  const tokens: string[] = [];
  for (let i = 0; i < raw.length; i++) {
    const t = raw[i];
    if (t.toLowerCase() === "latest") continue;
    if (/^\d{8}$/.test(t)) continue;
    if (/^\d{4}$/.test(t) && isNum(raw[i + 1] ?? "") && isNum(raw[i + 2] ?? "") && i > 0) {
      i += 2;
      continue;
    }
    tokens.push(t);
  }
  // Join version parts: ["opus", "5", "5"] -> ["opus", "5.5"].
  const merged: string[] = [];
  for (const t of tokens) {
    const prev = merged[merged.length - 1];
    if (prev && isNum(t) && !t.startsWith("0") && !prev.startsWith("0") && /^\d{1,2}(\.\d+)*$/.test(prev)) merged[merged.length - 1] = `${prev}.${t}`;
    else merged.push(t);
  }
  const words = merged.map(word);
  // "GPT 5" reads as "GPT-5" the way OpenAI writes it.
  let out = words.join(" ").replace(/\bGPT (\d[\w.]*|OSS)\b/g, "GPT-$1");
  if (tag && tag.toLowerCase() !== "latest") out += ` (${tag})`;
  return out;
}

export function modelLabel(id: string, models?: ModelInfo[]): string {
  const m = models?.find((x) => x.id === id);
  return m?.name || prettifyModelId(id);
}
