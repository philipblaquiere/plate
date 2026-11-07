// Fuzzy matching for quick open and the project switcher. Kept free of the DOM so tests can use it.

export interface Match {
  score: number;
  /** Which characters of the text matched, for highlighting. */
  positions: number[];
}

const WORD_START = /[\s/._-]/;

/**
 * Do the query's characters appear in the text, in order? Better matches score higher: a run of
 * characters, the start of a word, and the file's name rather than its folders. Spaces in the
 * query are ignored, so "hotel wing" finds `hotel/wing.ncl`.
 */
export function fuzzyMatch(query: string, text: string): Match | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return { score: 0, positions: [] };
  const t = text.toLowerCase();

  // Scan from the right, so a match lands in the file name when it can.
  const positions: number[] = [];
  let at = t.length - 1;
  for (let i = q.length - 1; i >= 0; i--) {
    at = t.lastIndexOf(q[i], at);
    if (at < 0) return null;
    positions.unshift(at);
    at--;
  }
  // Pull each character as far right as the next one allows, then tighten into runs from the left.
  for (let i = 0; i < positions.length - 1; i++) {
    const next = t.indexOf(q[i], positions[i] + 1);
    if (next >= 0 && next < positions[i + 1] && next === positions[i + 1] - 1) positions[i] = next;
  }

  const nameStart = text.lastIndexOf("/") + 1;
  let score = 0;
  positions.forEach((p, i) => {
    score += 1;
    if (i > 0 && positions[i - 1] === p - 1) score += 9;
    if (p === 0 || WORD_START.test(text[p - 1])) score += 8;
    if (p >= nameStart) score += 3;
    if (text[p] === query.replace(/\s+/g, "")[i]) score += 0.5;
  });
  if (t.startsWith(q)) score += 12;
  if (t.slice(nameStart).startsWith(q)) score += 12;
  // Between equals, the shorter text is the closer match.
  score -= text.length / 200;
  return { score, positions };
}

/** Items that match, best first; ties keep their order. An empty query keeps them all, in order. */
export function fuzzyFilter<T>(
  query: string,
  items: readonly T[],
  text: (item: T) => string,
): { item: T; match: Match }[] {
  const out: { item: T; match: Match; index: number }[] = [];
  items.forEach((item, index) => {
    const match = fuzzyMatch(query, text(item));
    if (match) out.push({ item, match, index });
  });
  if (query.trim()) out.sort((a, b) => b.match.score - a.match.score || a.index - b.index);
  return out;
}
