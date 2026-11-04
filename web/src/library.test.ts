import { describe, expect, it } from "vitest";

import { libraryCard, onlyParams, placementText } from "./library";

const opts = { usedBy: ["5-duplex.ncl"], thumbnail: () => "<svg></svg>" };

describe("library card", () => {
  it("lists assemblies with their build-up and what they're for", () => {
    const html = libraryCard(
      "assemblies",
      "wood-frame",
      {
        partition: {
          on: ["partition", "link"],
          layers: [
            { material: "gypsum", thickness: "13 mm" },
            { material: "stud", thickness: "89 mm", cavity: true },
            { material: "gypsum", thickness: "13 mm" },
          ],
          pattern: "light",
        },
        quiet: { thickness: "165 mm", acoustic: 50 },
      },
      opts,
    );
    expect(html).toContain("115 mm");
    expect(html).toContain("cavity 89 mm");
    expect(html).toContain("on 'partition, 'link");
    expect(html).toContain("only where a rule asks");
    expect(html).toContain("STC 50");
    expect(html).toContain("lib-swatch-light");
    expect(html).toContain("5-duplex.ncl");
  });

  it("describes a service: parts, where they go, claims and networks", () => {
    const html = libraryCard(
      "service",
      "power",
      {
        label: "Power",
        scope: "#dwelling",
        parts: { outlet: { symbol: "receptacle", label: "receptacle" }, panel: { above: "1500 mm" } },
        place: [{ place: "every", part: "outlet", spaces: ["#room"], spacing: "3.6 m", min_run: "900 mm" }],
        claims: { rules: [{}], inside: { "#utility": { spaces: { heater: {} } } } },
        connect: [{ label: "Circuits", from: "panel", to: ["outlet"], max: 12, via: { cavity: "89 mm" } }],
      },
      { ...opts, usedBy: [] },
    );
    expect(html).toContain('<h2 class="lib-title">Power</h2>');
    expect(html).toContain("One for each #dwelling");
    expect(html).toContain("receptacle every 3.6 m along the walls of #room, walls under 900 mm skipped");
    expect(html).toContain("inside each #utility: <code>heater</code>");
    expect(html).toContain("at most 12 a group");
    expect(html).toContain("Not used by any plate yet");
  });

  it("says where one part goes", () => {
    expect(
      placementText(
        { place: "one", spaces: "#room", near: "door", apart: ["window", "#drain"] },
        "thermostat",
      ),
    ).toBe("thermostat: one in each #room, near 'door, away from 'window, #drain");
    expect(placementText({ place: "beside", opening: "window" }, "baseboard")).toBe(
      "baseboard under each window",
    );
  });

  it("shows a code's numbers as numbers to merge, not as an empty service", () => {
    const html = libraryCard(
      "service",
      "cec",
      { params: { wall_spacing: "3.6 m", per_circuit: 12 } },
      { ...opts, usedBy: [] },
    );
    expect(html).toContain("Numbers for a service");
    expect(html).toContain("<h3>Numbers</h3>");
    expect(html).toContain("<code>wall_spacing</code> 3.6 m");
    expect(html).toContain("<code>per_circuit</code> 12");
    expect(html).not.toContain("A service, for");
    expect(onlyParams({ params: {}, parts: {} })).toBe(false);
  });
});
