// A chat is a list of turns: something the user sent, then every message
// (replies, tool calls, tool results) until the next thing the user sent.
// Deleting, retrying and editing all work on whole turns.
import { isAttachment, type Attachment, type Message } from "./api";

/** A message the user wrote (text or attachments), as opposed to tool results. */
export const isPrompt = (m: Message) => m.role === "user" && m.content.some((b) => b.type === "text" || isAttachment(b));

export interface Turn {
  /** Index of the turn's first message (the prompt, when `prompt` is true). */
  start: number;
  /** One past the turn's last message. */
  end: number;
  prompt: boolean;
}

export function turns(messages: Message[]): Turn[] {
  const out: Turn[] = [];
  messages.forEach((m, i) => {
    const p = isPrompt(m);
    if (p || out.length === 0) out.push({ start: i, end: i + 1, prompt: p });
    else out[out.length - 1].end = i + 1;
  });
  return out;
}

export const hasReply = (t: Turn) => t.end > t.start + 1;

/** Older chats can carry tool results in the same message as a prompt. Those
 *  must stay, or the tool call before them is left without an answer. */
function keepResults(m: Message | undefined): Message[] {
  const results = m?.content.filter((b) => b.type === "tool_result") ?? [];
  return results.length ? [{ role: "user", content: results }] : [];
}

/** Everything before the turn (for retry and edit, which resend its prompt). */
export const before = (messages: Message[], t: Turn): Message[] => [...messages.slice(0, t.start), ...keepResults(messages[t.start])];

export const withoutTurn = (messages: Message[], t: Turn): Message[] => [...before(messages, t), ...messages.slice(t.end)];

export const withoutReply = (messages: Message[], t: Turn): Message[] => [...messages.slice(0, t.start + 1), ...messages.slice(t.end)];

export function promptParts(m: Message): { text: string; attachments: Attachment[] } {
  const text = m.content
    .map((b) => (b.type === "text" ? b.text : null))
    .filter((t): t is string => t !== null)
    .join("\n\n");
  return { text, attachments: m.content.filter(isAttachment) };
}

/** The reply's prose, for copying. */
export function replyText(messages: Message[], t: Turn): string {
  return messages
    .slice(t.start + 1, t.end)
    .filter((m) => m.role === "assistant")
    .flatMap((m) => m.content.map((b) => (b.type === "text" ? b.text.trim() : "")))
    .filter(Boolean)
    .join("\n\n");
}
