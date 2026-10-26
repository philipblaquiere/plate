// Files from the user's computer, outside the workspace: picking a .ncl to add to the workspace,
// and downloading exports.

export interface PickedFile {
  name: string;
  text: string;
}

/** Ask for a .ncl file on this computer. `null` when the user cancels. */
export async function pickFile(): Promise<PickedFile | null> {
  const file = await new Promise<File | null>((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".ncl,text/plain";
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
  return file ? { name: file.name, text: await file.text() } : null;
}

export function download(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** The single-file draft of earlier versions, if one was left behind. Read once, then dropped. */
export function takeLegacyDraft(): PickedFile | null {
  try {
    const raw = localStorage.getItem("plate:draft");
    localStorage.removeItem("plate:draft");
    const draft = raw ? (JSON.parse(raw) as PickedFile) : null;
    return draft?.text?.trim() ? draft : null;
  } catch {
    return null;
  }
}
