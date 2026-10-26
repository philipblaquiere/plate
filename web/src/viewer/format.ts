import type { System, Units } from "../types";

/**
 * How lengths are written on screen. Geometry arrives in the layout's `units`; the
 * `system` decides the presentation:
 *
 * - metric: decimals in the file's metric unit (m, cm or mm; metres if the file uses feet)
 * - imperial: feet and inches with fractions, `98'-6 1/2"`; areas in ft²
 */

const MM: Record<Units, number> = { mm: 1, cm: 10, m: 1000, in: 25.4, ft: 304.8 };
const DECIMALS = { mm: 0, cm: 1, m: 2 } as const;
type MetricUnit = keyof typeof DECIMALS;

/** Imperial grid and scale-bar steps, in inches. */
const IMPERIAL_STEPS = [
  1 / 16,
  1 / 8,
  1 / 4,
  1 / 2,
  1,
  3,
  6,
  12,
  24,
  60,
  120,
  240,
  600,
  1200,
  2400,
  6000,
  12000,
  24000,
  60000,
];

export class Lengths {
  constructor(
    readonly system: System,
    readonly units: Units,
  ) {}

  static for(layout: { system: System; units: Units } | null | undefined): Lengths {
    return new Lengths(layout?.system ?? "metric", layout?.units ?? "m");
  }

  /** Unit symbol for the rulers' corner. */
  get rulerUnit(): string {
    return this.system === "imperial" ? "ft" : this.metricUnit;
  }

  private get metricUnit(): MetricUnit {
    return this.units in DECIMALS ? (this.units as MetricUnit) : "m";
  }

  private inches(v: number): number {
    return (v * MM[this.units]) / 25.4;
  }

  private metric(v: number): number {
    return (v * MM[this.units]) / MM[this.metricUnit];
  }

  /** A length with its unit: `30.00 m`, `98'-6 1/2"`. `precise` adds one more step of precision. */
  length(v: number, precise = false): string {
    if (this.system === "imperial") return feetInches(this.inches(v), precise ? 32 : 16);
    const u = this.metricUnit;
    return `${decimal(this.metric(v), DECIMALS[u] + (precise ? 1 : 0))} ${u}`;
  }

  /** A dimension label on the drawing. Metric drawings omit the unit, as on paper. */
  dimension(v: number): string {
    if (this.system === "imperial") return feetInches(this.inches(v), 16);
    const u = this.metricUnit;
    return decimal(this.metric(v), DECIMALS[u]);
  }

  /** An area given in square drawing units. */
  area(v: number): string {
    if (this.system === "imperial") {
      const sqft = (v * MM[this.units] ** 2) / 304.8 ** 2;
      return `${decimal(sqft, sqft >= 100 ? 0 : 2, ",")} ft²`;
    }
    const sqm = (v * MM[this.units] ** 2) / 1e6;
    return `${decimal(sqm, 2)} m²`;
  }

  /** Compact ruler/scale-bar label: `25`, `10'`, `6"`, `2'-6"`. */
  tick(v: number): string {
    if (this.system === "metric") {
      const n = Number(this.metric(v).toPrecision(10));
      return String(Math.abs(n) < 1e-9 ? 0 : n).replace("-", "−");
    }
    const inches = Math.round(this.inches(v) * 16) / 16;
    if (inches === 0) return "0";
    if (Math.abs(inches) >= 12 && inches % 12 === 0) return `${group(inches / 12, ",")}'`.replace("-", "−");
    return feetInches(inches, 16);
  }

  /** Grid spacing in drawing units, so minor lines are at least `minPx` apart. */
  gridSteps(pxPerUnit: number, minPx = 14): { minor: number; major: number } {
    const raw = minPx / pxPerUnit;
    if (this.system === "imperial") {
      const toUnits = 25.4 / MM[this.units];
      const ladder = IMPERIAL_STEPS;
      const i = ladder.findIndex((s) => s * toUnits >= raw);
      const minor = ladder[i === -1 ? ladder.length - 1 : i];
      const major =
        ladder.find((s) => s >= minor * 4 && Number.isInteger(Math.round((s / minor) * 1e6) / 1e6)) ??
        minor * 10;
      return { minor: minor * toUnits, major: major * toUnits };
    }
    const toUnits = MM[this.metricUnit] / MM[this.units];
    const rawDisplay = raw / toUnits;
    const pow = Math.pow(10, Math.floor(Math.log10(rawDisplay)));
    const lead = [1, 2, 5, 10].find((n) => n * pow >= rawDisplay) ?? 10;
    const minor = lead * pow;
    return { minor: minor * toUnits, major: minor * (lead === 5 ? 2 : 5) * toUnits };
  }

  /** A round scale-bar length (drawing units) close to, but not over, `targetPx`. */
  scaleBar(pxPerUnit: number, targetPx = 150): number {
    const target = targetPx / pxPerUnit;
    if (this.system === "imperial") {
      const toUnits = 25.4 / MM[this.units];
      const fits = IMPERIAL_STEPS.filter((s) => s * toUnits <= target);
      return (fits.pop() ?? IMPERIAL_STEPS[0]) * toUnits;
    }
    const toUnits = MM[this.metricUnit] / MM[this.units];
    const t = target / toUnits;
    const pow = Math.pow(10, Math.floor(Math.log10(t)));
    const step =
      [1, 2, 5, 10]
        .map((n) => n * pow)
        .filter((n) => n <= t)
        .pop() ?? pow;
    return step * toUnits;
  }
}

const THIN_SPACE = "\u2009";

/** Thousands grouping: a thin space for metric (SI style), a comma for imperial. */
function group(n: number, separator = THIN_SPACE): string {
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, separator);
}

function decimal(value: number, decimals: number, separator?: string): string {
  const [int, frac] = Math.abs(value).toFixed(decimals).split(".");
  const sign = value < 0 && Number(Math.abs(value).toFixed(decimals)) !== 0 ? "\u2212" : "";
  return `${sign}${group(Number(int), separator)}${frac ? "." + frac : ""}`;
}

/** `98'-6 1/2"`, `6 1/2"`, `1/2"`, `30'-0"`, rounded to 1/denominator of an inch. */
export function feetInches(totalInches: number, denominator = 16): string {
  let n = Math.round(Math.abs(totalInches) * denominator);
  const sign = totalInches < 0 && n !== 0 ? "−" : "";
  const feet = Math.floor(n / (12 * denominator));
  n -= feet * 12 * denominator;
  const whole = Math.floor(n / denominator);
  let num = n - whole * denominator;
  let den = denominator;
  while (num && num % 2 === 0) {
    num /= 2;
    den /= 2;
  }
  const fraction = num ? `${num}/${den}` : "";
  const inches = whole && fraction ? `${whole} ${fraction}` : fraction || String(whole);
  return feet ? `${sign}${group(feet, ",")}'-${inches}"` : `${sign}${inches}"`;
}

export function degrees(rad: number): string {
  let d = (rad * 180) / Math.PI;
  d = ((d % 360) + 360) % 360;
  return `${Number(d.toFixed(1))}°`;
}
