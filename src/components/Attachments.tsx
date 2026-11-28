import { FileText, X } from "lucide-react";
import { api, type Attachment } from "../lib/api";

const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
const MAX_IMAGE = 5 * 1024 * 1024;

export const imageSrc = (a: Extract<Attachment, { type: "image" }>) => `data:${a.media_type};base64,${a.data}`;

/** Read files from disk (picked or dropped). Files that can't be attached are reported, not thrown. */
export async function readPaths(paths: string[], onError: (msg: string) => void): Promise<Attachment[]> {
  const out: Attachment[] = [];
  for (const p of paths) {
    try {
      out.push(await api.readAttachment(p));
    } catch (e) {
      onError(String(e));
    }
  }
  return out;
}

/** Images pasted from the clipboard. */
export async function readPastedImages(files: File[], onError: (msg: string) => void): Promise<Attachment[]> {
  const out: Attachment[] = [];
  for (const f of files) {
    if (!IMAGE_TYPES.includes(f.type)) continue;
    if (f.size > MAX_IMAGE) {
      onError(`${f.name || "That image"} is over 5 MB`);
      continue;
    }
    const url = await new Promise<string>((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => resolve(String(r.result));
      r.onerror = () => reject(r.error);
      r.readAsDataURL(f);
    });
    const ext = f.type.split("/")[1] === "jpeg" ? "jpg" : f.type.split("/")[1];
    out.push({ type: "image", name: f.name && f.name !== "image.png" ? f.name : `pasted-image.${ext}`, media_type: f.type, data: url.split(",")[1] ?? "" });
  }
  return out;
}

export function AttachmentChip({ a, onRemove }: { a: Attachment; onRemove?: () => void }) {
  return (
    <div className="att" title={a.name}>
      {a.type === "image" ? (
        <img className="att-thumb" src={imageSrc(a)} alt="" />
      ) : (
        <span className="att-icon">
          <FileText size={14} />
        </span>
      )}
      <span className="att-name">{a.name}</span>
      {onRemove && (
        <button className="att-x" onClick={onRemove} title="Remove" aria-label={`Remove ${a.name}`}>
          <X size={11} />
        </button>
      )}
    </div>
  );
}

export function AttachmentRow({ items, onRemove }: { items: Attachment[]; onRemove?: (i: number) => void }) {
  if (!items.length) return null;
  return (
    <div className="att-row">
      {items.map((a, i) => (
        <AttachmentChip key={i} a={a} onRemove={onRemove ? () => onRemove(i) : undefined} />
      ))}
    </div>
  );
}

/** How attachments look inside a sent message: images as previews, files as chips. */
export function SentAttachments({ items }: { items: Attachment[] }) {
  if (!items.length) return null;
  const images = items.filter((a) => a.type === "image");
  const files = items.filter((a) => a.type === "file");
  return (
    <>
      {images.length > 0 && (
        <div className="sent-images">
          {images.map((a, i) => (
            <img key={i} className="sent-image" src={imageSrc(a)} alt={a.name} title={a.name} />
          ))}
        </div>
      )}
      {files.length > 0 && <AttachmentRow items={files} />}
    </>
  );
}
