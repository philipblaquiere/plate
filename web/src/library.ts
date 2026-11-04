// A library file (a service, a set of assemblies) opened on its own: nothing to draw, so the
// sheet shows what it is instead. Kept free of the DOM so tests can use it: it returns HTML.

type Value = Record<string, unknown>;

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);

/** A length as written: `"115 mm"`, or a plain number (in the plate's units). */
function len(v: unknown): string {
  return typeof v === "number" ? String(v) : typeof v === "string" ? v : "";
}

/** Millimetres, for lengths written in mm (how build-ups are written); else null. */
function mm(v: unknown): number | null {
  const m = typeof v === "string" ? v.match(/^\s*(\d+(?:\.\d+)?)\s*mm\s*$/) : null;
  return m ? Number(m[1]) : null;
}

const list = (v: unknown): string[] => (Array.isArray(v) ? v.map(String) : v == null ? [] : [String(v)]);
const roles = (v: unknown) =>
  list(v)
    .map((r) => `'${r}`)
    .join(", ");
const sel = (v: unknown) => list(v).join(", ");

export interface CardOptions {
  /** Plates that import the file. */
  usedBy: string[];
  /** A symbol's thumbnail, as SVG markup. */
  thumbnail: (symbol: string) => string | null;
}

/** A service file that is only numbers (a code's): merged into a service, not used on its own. */
export function onlyParams(value: Value): boolean {
  return Object.keys(value).length > 0 && Object.keys(value).every((k) => k === "params" || k === "label");
}

export function libraryCard(
  kind: "service" | "assemblies",
  name: string,
  value: Value,
  opts: CardOptions,
): string {
  const numbers = kind === "service" && onlyParams(value);
  const body = kind === "assemblies" ? assemblies(value) : service(value, opts);
  const title = kind === "assemblies" ? `${esc(name)}: assemblies` : esc(String(value.label ?? name));
  const what =
    kind === "assemblies"
      ? "Wall types, for a plate's <code>assemblies</code>. Each wall is built as the thinnest one that meets what its roles and rules ask."
      : numbers
        ? 'Numbers for a service written in terms of them. Merge it into one: <code>(import "…") &amp; (import "this file")</code>.'
        : "A service, for a plate's <code>services</code>. It places its parts on the solved plan and connects them; its claims shape the plan.";
  const used = opts.usedBy.length
    ? `<p class="lib-used">Used by ${opts.usedBy.map((p) => `<code>${esc(p)}</code>`).join(", ")}</p>`
    : `<p class="lib-used">Not used by any plate yet.</p>`;
  return `<div class="lib"><h2 class="lib-title">${title}</h2><p class="lib-what">${what}</p>${body}${used}</div>`;
}

function assemblies(value: Value): string {
  const rows = Object.entries(value).map(([key, raw]) => {
    const a = (raw ?? {}) as Value;
    const layers = Array.isArray(a.layers) ? (a.layers as Value[]) : [];
    const sum = layers.reduce<number | null>(
      (t, l) => (t === null || mm(l.thickness) === null ? null : t + mm(l.thickness)!),
      0,
    );
    const thickness =
      a.thickness !== undefined ? len(a.thickness) : sum !== null && layers.length ? `${sum} mm` : "";
    const cavity =
      a.cavity !== undefined
        ? len(a.cavity)
        : (() => {
            const c = layers.filter((l) => l.cavity);
            const t = c.reduce<number | null>(
              (s, l) => (s === null || mm(l.thickness) === null ? null : s + mm(l.thickness)!),
              0,
            );
            return c.length && t !== null ? `${t} mm` : "";
          })();
    const thick = mm(thickness);
    const swatch = `<span class="lib-swatch lib-swatch-${esc(String(a.pattern ?? "solid"))}" style="height:${thick ? Math.max(4, Math.min(22, thick / 12)) : 8}px"></span>`;
    const facts: string[] = [];
    if (cavity) facts.push(`cavity ${esc(cavity)}`);
    if (a.fire !== undefined) facts.push(`${esc(String(a.fire))} h fire`);
    if (a.acoustic !== undefined) facts.push(`STC ${esc(String(a.acoustic))}`);
    if (a.bearing) facts.push("bearing");
    if (a.line === "centre") facts.push("centred on the boundary");
    const on = a.on !== undefined ? `on ${esc(roles(a.on))}` : "only where a rule asks";
    const buildup = layers.length
      ? `<div class="lib-buildup">${layers
          .map(
            (l) =>
              `${esc(len(l.thickness))} ${esc(String(l.material ?? ""))}${l.cavity ? " <i>cavity</i>" : ""}`,
          )
          .join(" · ")}</div>`
      : "";
    const label = a.label ? `<span class="lib-label">${esc(String(a.label))}</span>` : "";
    return (
      `<li class="lib-item">${swatch}<div><div class="lib-name"><code>${esc(key)}</code>${label}` +
      `<span class="lib-size">${esc(thickness)}</span></div>` +
      `<div class="lib-facts">${[on, ...facts].join(" · ")}</div>${buildup}</div></li>`
    );
  });
  return `<ul class="lib-list">${rows.join("")}</ul>`;
}

/** A placement in words. */
export function placementText(p: Value, part: string): string {
  const where = (v: unknown) => (v === undefined ? "" : ` of ${esc(sel(v))}`);
  switch (p.place) {
    case "every": {
      const skip = p.min_run !== undefined ? `, walls under ${esc(len(p.min_run))} skipped` : "";
      return `${part} every ${esc(len(p.spacing))} along the walls${where(p.spaces)}${skip}`;
    }
    case "beside": {
      const side = p.side ?? (p.opening === "window" ? "under" : "latch");
      const by = side === "under" ? `under each ${p.opening}` : `beside each ${p.opening} (${side} side)`;
      return `${part} ${esc(String(by))}${where(p.spaces)}`;
    }
    case "over": {
      const grid =
        p.per !== undefined
          ? `, one per ${esc(String(p.per))} of floor`
          : p.spacing !== undefined
            ? `, ${esc(len(p.spacing))} apart`
            : "";
      return `${part} in the middle${where(p.spaces)}${grid}`;
    }
    case "on":
      return `${part} at the back of each ${esc(sel(p.fixtures))}`;
    case "one": {
      const bits: string[] = [];
      if (p.near !== undefined) bits.push(`near ${esc(roles(p.near).replace(/'#/g, "#"))}`);
      if (p.apart !== undefined) bits.push(`away from ${esc(roles(p.apart).replace(/'#/g, "#"))}`);
      const wall = p.wall as Value | undefined;
      if (wall?.cavity !== undefined) bits.push(`on a wall with a ${esc(len(wall.cavity))} cavity`);
      return `${part}: one in each ${esc(sel(p.spaces))}${bits.length ? `, ${bits.join(", ")}` : ""}`;
    }
    default:
      return part;
  }
}

function service(value: Value, opts: CardOptions): string {
  const parts = (value.parts ?? {}) as Record<string, Value>;
  const label = (k: string) => esc(String(parts[k]?.label ?? k.replace(/_/g, " ")));
  const out: string[] = [];
  if (value.scope !== undefined)
    out.push(`<p class="lib-facts">One for each ${esc(sel(value.scope))}: its own parts and networks.</p>`);
  const partRows = Object.entries(parts).map(([k, p]) => {
    const thumb = p.symbol ? opts.thumbnail(String(p.symbol)) : null;
    const mount = String(p.mount ?? "wall");
    const at =
      mount === "wall"
        ? `on a wall, ${esc(len(p.above ?? "300 mm"))} up`
        : mount === "cavity"
          ? "in a wall"
          : `on the ${mount}`;
    return `<li class="lib-item">${thumb ? `<span class="lib-thumb">${thumb}</span>` : '<span class="lib-thumb"></span>'}<div><div class="lib-name"><code>${esc(k)}</code><span class="lib-label">${label(k)}</span></div><div class="lib-facts">${at}</div></div></li>`;
  });
  if (partRows.length) out.push(`<h3>Parts</h3><ul class="lib-list">${partRows.join("")}</ul>`);
  const place = Array.isArray(value.place) ? (value.place as Value[]) : [];
  if (place.length)
    out.push(
      `<h3>Where they go</h3><ul class="lib-plain">${place.map((p) => `<li>${placementText(p, label(String(p.part)))}</li>`).join("")}</ul>`,
    );
  const claims = (value.claims ?? {}) as Value;
  const claimRules = Array.isArray(claims.rules) ? claims.rules.length : 0;
  const inside = Object.entries((claims.inside ?? {}) as Record<string, Value>);
  if (claimRules || inside.length) {
    const bits: string[] = [];
    if (claimRules) bits.push(`${claimRules} rule${claimRules > 1 ? "s" : ""} in every plan it covers`);
    for (const [k, frag] of inside) {
      const what: string[] = [];
      const spaces = Object.keys((frag.spaces ?? {}) as Value);
      if (spaces.length) what.push(spaces.map((s) => `<code>${esc(s)}</code>`).join(", "));
      const rules = Array.isArray(frag.rules) ? frag.rules.length : 0;
      if (rules) what.push(`${rules} rule${rules > 1 ? "s" : ""}`);
      bits.push(`inside each ${esc(k)}: ${what.join(" and ") || "needs"}`);
    }
    out.push(
      `<h3>What it claims</h3><p class="lib-facts">The solver makes room for: ${bits.join("; ")}.</p>`,
    );
  }
  const connect = Array.isArray(value.connect) ? (value.connect as Value[]) : [];
  if (connect.length) {
    const rows = connect.map((n) => {
      const via = (n.via ?? {}) as Value;
      const bits = [`from ${esc(sel(n.from))} to ${esc(sel(n.to))}`];
      if (n.max !== undefined) bits.push(`at most ${esc(String(n.max))} a group`);
      if (via.cavity !== undefined) bits.push(`in walls with a ${esc(len(via.cavity))} cavity`);
      if (via.ceilings !== undefined) bits.push(`across the ceilings of ${esc(sel(via.ceilings))}`);
      if (n.limit !== undefined) bits.push(`no run over ${esc(len(n.limit))}`);
      return `<li><b>${esc(String(n.label ?? "Network"))}</b>: ${bits.join(", ")}${n.draw === "routed" ? ", routed" : ""}</li>`;
    });
    out.push(`<h3>Networks</h3><ul class="lib-plain">${rows.join("")}</ul>`);
  }
  if (value.params && typeof value.params === "object") {
    const rows = Object.entries(value.params as Value).map(
      ([k, v]) => `<code>${esc(k)}</code> ${esc(len(v) || JSON.stringify(v))}`,
    );
    out.push(
      `<h3>${onlyParams(value) ? "Numbers" : "Written in terms of"}</h3><p class="lib-facts">${rows.join(" · ")}</p>`,
    );
  }
  return out.join("");
}
