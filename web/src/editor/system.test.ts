import { describe, expect, it } from "vitest";
import { withSystem } from "./system";

describe("withSystem", () => {
  it("replaces an existing system", () => {
    const src = "{\n  name = \"A\",\n  system = 'metric,\n  units = 'm,\n}";
    expect(withSystem(src, "imperial", "m")).toBe(
      "{\n  name = \"A\",\n  system = 'imperial,\n  units = 'm,\n}",
    );
  });

  it("inserts system after name and pins the current units", () => {
    const src = '{\n  name = "A",\n  outline = { kind = \'square, size = 30 },\n}';
    expect(withSystem(src, "imperial", "m")).toBe(
      "{\n  name = \"A\",\n  system = 'imperial,\n  units = 'm,\n  outline = { kind = 'square, size = 30 },\n}",
    );
  });

  it("inserts after the opening brace when there is no name", () => {
    const src = "{\n  outline = { kind = 'square, size = 30 },\n}";
    expect(withSystem(src, "metric")).toBe(
      "{\n  system = 'metric,\n  outline = { kind = 'square, size = 30 },\n}",
    );
  });
});
