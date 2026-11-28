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

/** The part of a model id people type: "cognitivecomputations/dolphin:free" -> "dolphin". */
const bareId = (id: string) => (id.split("/").pop() ?? id).split(":")[0].toLowerCase();

/**
 * Work out which provider and model the user means when they type into the
 * model search and press Enter. The model must be sent to the provider that
 * lists it: sending an OpenRouter model id to Gemini fails with a Gemini error.
 *
 * Order: exact id, then the id without its vendor prefix or ":tag", then the
 * first search result. Only when no list has a match is the text used as a
 * raw model id, on OpenRouter if it looks like "vendor/model", otherwise on
 * the current provider.
 */
export function resolveTypedModel(
  query: string,
  lists: { provider: string; models: ModelInfo[] }[],
  current: string,
): { provider: string; model: string } {
  const raw = query.trim();
  const q = raw.toLowerCase();
  // The current provider wins ties.
  const ordered = [...lists].sort((a, b) => Number(b.provider === current) - Number(a.provider === current));
  const find = (test: (m: ModelInfo) => boolean) => {
    for (const l of ordered) {
      const m = l.models.find(test);
      if (m) return { provider: l.provider, model: m.id };
    }
    return null;
  };
  const bare = bareId(raw);
  return (
    find((m) => m.id.toLowerCase() === q) ??
    find((m) => bareId(m.id) === bare) ??
    find((m) => m.id.toLowerCase().includes(q) || modelLabel(m.id, [m]).toLowerCase().includes(q)) ?? {
      provider: raw.includes("/") && lists.some((l) => l.provider === "openrouter") ? "openrouter" : current,
      model: raw,
    }
  );
}
