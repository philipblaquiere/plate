/**
 * A small Markdown renderer for the reference: headings, paragraphs, lists, tables, fenced code,
 * `code`, **bold**, *emphasis* and links. Everything is escaped; links to other topics
 * (`guide:typicals`, `def:Space`, `helper:door`) become `data-ref` links the panel follows.
 */

export interface MarkdownOptions {
  /** Topic an inline `code` span refers to (`plate.door` → `helper:door`), if any. */
  codeRef?(code: string): string | undefined;
}

const escape = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

const TOPIC = /^(guide|def|helper):[\w#-]+$/;

export function inline(text: string, options: MarkdownOptions = {}): string {
  const out: string[] = [];
  // Code spans first, so nothing inside them is formatted.
  const parts = text.split(/(`[^`]+`)/g);
  for (const part of parts) {
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      const code = part.slice(1, -1);
      const ref = options.codeRef?.(code);
      out.push(
        ref
          ? `<a href="#" class="ref-link" data-ref="${escape(ref)}"><code>${escape(code)}</code></a>`
          : `<code>${escape(code)}</code>`,
      );
      continue;
    }
    let html = escape(part);
    html = html.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label: string, href: string) => {
      const target = href.replace(/^#/, "").replace(/&amp;/g, "&");
      if (TOPIC.test(target)) return `<a href="#" class="ref-link" data-ref="${target}">${label}</a>`;
      if (/^https?:\/\//.test(target))
        return `<a href="${href}" target="_blank" rel="noopener noreferrer">${label}</a>`;
      return label;
    });
    html = html.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
    html = html.replace(/(^|[^\w*])\*([^*\s][^*]*)\*(?!\w)/g, "$1<em>$2</em>");
    out.push(html);
  }
  return out.join("");
}

export function renderMarkdown(md: string, options: MarkdownOptions = {}): string {
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let i = 0;
  const para: string[] = [];
  const flush = () => {
    if (para.length) out.push(`<p>${inline(para.join(" "), options)}</p>`);
    para.length = 0;
  };
  while (i < lines.length) {
    const line = lines[i];
    const fence = /^```\s*(\w*)\s*$/.exec(line);
    if (fence) {
      flush();
      const code: string[] = [];
      i++;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) code.push(lines[i++]);
      i++;
      out.push(
        `<pre class="ref-code" data-lang="${escape(fence[1] || "text")}"><code>${escape(code.join("\n"))}</code></pre>`,
      );
      continue;
    }
    const heading = /^(#{1,4})\s+(.+)$/.exec(line);
    if (heading) {
      flush();
      // Pages have their own title: headings inside start at h3.
      const level = Math.min(6, heading[1].length + 2);
      out.push(`<h${level}>${inline(heading[2], options)}</h${level}>`);
      i++;
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      flush();
      const ordered = /^\s*\d+\./.test(line);
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) {
        let item = lines[i].replace(/^\s*([-*]|\d+\.)\s+/, "");
        i++;
        // Continuation lines, indented.
        while (i < lines.length && /^\s{2,}\S/.test(lines[i]) && !/^\s*([-*]|\d+\.)\s+/.test(lines[i]))
          item += ` ${lines[i++].trim()}`;
        items.push(`<li>${inline(item, options)}</li>`);
      }
      out.push(ordered ? `<ol>${items.join("")}</ol>` : `<ul>${items.join("")}</ul>`);
      continue;
    }
    if (/^\s*\|.*\|\s*$/.test(line) && /^\s*\|[\s:|-]+\|\s*$/.test(lines[i + 1] ?? "")) {
      flush();
      const cells = (l: string) =>
        l
          .trim()
          .replace(/^\||\|$/g, "")
          .split("|")
          .map((c) => inline(c.trim(), options));
      const head = cells(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) rows.push(cells(lines[i++]));
      out.push(
        `<table><thead><tr>${head.map((h) => `<th>${h}</th>`).join("")}</tr></thead><tbody>${rows
          .map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join("")}</tr>`)
          .join("")}</tbody></table>`,
      );
      continue;
    }
    if (!line.trim()) {
      flush();
      i++;
      continue;
    }
    para.push(line.trim());
    i++;
  }
  flush();
  return out.join("\n");
}
