/**
 * What can be done to an edge between two spaces, as rule text: make it open, cut an opening or a
 * door in it, or build a wall on it. Pure, so it's tested without the editor.
 *
 * An edge's `rule` is the rule that decided it, if one did. A rule that names exactly these two
 * spaces can be changed or removed. One that covers more (a list, a tag) is left alone: changing
 * it would change other edges too.
 */
import type { EdgeInfo } from "./types";

export type Treatment = "open" | "opening" | "door" | "wall";

export const TREATMENTS: { treatment: Treatment; label: string; note: string }[] = [
  { treatment: "open", label: "Make open", note: "No wall: plate.open" },
  { treatment: "opening", label: "Add an opening", note: "A gap with no door: plate.opening" },
  { treatment: "door", label: "Add a door", note: "plate.door" },
  { treatment: "wall", label: "Build a wall", note: "plate.wall" },
];

/** The rule text that gives an edge between `a` and `b` this treatment. */
export function ruleText(treatment: Treatment, a: string, b: string): string {
  const pair = `${JSON.stringify(a)} ${JSON.stringify(b)}`;
  switch (treatment) {
    case "open":
      return `plate.open ${pair}`;
    case "opening":
      return `plate.opening ${pair}`;
    case "door":
      return `plate.door ${pair}`;
    case "wall":
      return `plate.wall ${pair}`;
  }
}

/** A rule as evaluated: its kind and selectors. */
export interface RuleValue {
  rule?: string;
  a?: unknown;
  b?: unknown;
}

/** Does this rule name exactly these two spaces, and nothing else? */
export function namesOnly(rule: RuleValue, names: string[]): boolean {
  const one = (v: unknown): string | null =>
    typeof v === "string" ? v : Array.isArray(v) && v.length === 1 && typeof v[0] === "string" ? v[0] : null;
  const [a, b] = [one(rule.a), one(rule.b)];
  if (a === null || b === null) return false;
  return (a === names[0] && b === names[1]) || (a === names[1] && b === names[0]);
}

/** One menu entry for an edge: what it does, as text to add or a rule to change. */
export type EdgeAction =
  | { label: string; note: string; add: string[] }
  | { label: string; note: string; replace: string; text: string }
  | { label: string; note: string; remove: string };

/**
 * The menu for an edge. `rule` is the rule that decided it, as evaluated (null if none or if it
 * can't be read). Treatments it already has aren't offered.
 */
export function edgeActions(edge: EdgeInfo, rule: RuleValue | null): EdgeAction[] {
  const [a, b] = edge.names;
  const others = TREATMENTS.filter((t) => t.treatment !== edge.treatment);
  if (edge.rule && rule && namesOnly(rule, edge.names)) {
    const pointer = edge.rule;
    // Replacing a rule with a door where these two get no wall by default would leave the door
    // in no wall: offered only where they're walled.
    const ok = others.filter((t) => t.treatment !== "door" || edge.default === "wall");
    return [
      ...ok.map((t) => ({
        label: t.label,
        note: `Changes the rule: ${t.note.replace(/^.*: /, "")}`,
        replace: pointer,
        text: ruleText(t.treatment, a, b),
      })),
      { label: "Remove the rule", note: "Back to what these two spaces get by default", remove: pointer },
    ];
  }
  if (edge.rule) return [];
  return others.map((t) => {
    // A door needs a wall: where these two get none by default, build one for it.
    const wall = t.treatment === "door" && edge.default === "open";
    return {
      label: t.label,
      note: wall ? "plate.wall and plate.door" : t.note,
      add: wall ? [ruleText("wall", a, b), ruleText("door", a, b)] : [ruleText(t.treatment, a, b)],
    };
  });
}
