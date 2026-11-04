import { describe, expect, it } from "vitest";
import { inline, renderMarkdown } from "./markdown";
import { buildReference, GUIDES, reference, search, snippetText } from "./model";

const entry = (id: string) => {
  const e = reference.entries.get(id);
  if (!e) throw new Error(`no entry ${id}`);
  return e;
};

describe("the reference, generated from the schema, helpers and guides", () => {
  it("has a page for the plate, each starting mode, spaces, plans and every rule kind", () => {
    expect(entry("def:#").fields.map((f) => f.name)).toEqual(
      expect.arrayContaining(["name", "system", "units", "start", "spaces", "rules", "goals"]),
    );
    expect(entry("def:Start").variants).toEqual(["def:BoundaryStart", "def:SpineStart"]);
    expect(entry("def:BoundaryStart")).toMatchObject({ title: "Boundary", subtitle: "mode = 'boundary" });
    expect(entry("def:Rules").variants).toHaveLength(11);
    expect(entry("def:RuleDoor").section).toBe("Rules");
    expect(
      entry("def:Space")
        .fields.find((f) => f.name === "kind")
        ?.values.map((v) => v.value),
    ).toEqual(["room", "zone", "circulation", "void", "vertical", "fixture"]);
  });

  it("folds helper definitions into the page that explains them", () => {
    expect(reference.entries.has("def:Spaces")).toBe(false);
    expect(reference.entryFor("Spaces")).toBe("def:Space");
    expect(reference.entryFor("PortNeed")).toBe("def:Needs");
    const ports = entry("def:Needs").fields.find((f) => f.name === "ports")!;
    expect(ports.type).toBe("{ name = 'required | 'optional | 'forbidden }");
    expect(ports.values.map((v) => v.value)).toEqual(["required", "optional", "forbidden"]);
  });

  it("links types, uses and guides", () => {
    const area = entry("def:Space").fields.find((f) => f.name === "area")!;
    expect(area.typeLink).toBe("def:AreaRange");
    expect(entry("def:AreaRange").usedBy).toContainEqual({ entry: "def:Space", field: "area" });
    expect(entry("def:Space").fields.find((f) => f.name === "typical")!.guides).toEqual(["typicals"]);
    expect(entry("def:Port").values.find((v) => v.value === "wet")?.doc).toContain("Plumbing");
  });

  it("connects rule kinds and the helpers that write them", () => {
    expect(entry("def:RuleTouch").helpers).toEqual(["helper:touches", "helper:between"]);
    expect(entry("helper:between").rules).toEqual(["def:RuleTouch", "def:RuleApart"]);
    // A service's placements are written with helpers too.
    expect(entry("helper:every").rules).toEqual(["def:PlaceEvery"]);
    expect(entry("def:PlaceEvery").template).toMatch(/^plate\.every /);
    expect(entry("helper:wall").rules).toEqual(["def:RuleWall"]);
    expect(entry("def:RuleReach").template).toBe('plate.reach "${1:a}" "${2:b}" "${3:via}"');
    expect(entry("helper:door")).toMatchObject({ title: "plate.door", subtitle: "plate.door a b" });
    expect(entry("helper:door").examples[0].code).toBe('plate.door "bedroom" "hall"');
    expect(entry("def:Rules").helpers).toContain("helper:prefer");
  });

  it("shows examples from x-examples and builds templates", () => {
    expect(entry("def:Start").examples).toHaveLength(3);
    expect(entry("def:Space").fields.find((f) => f.name === "count")!.examples[0].code).toBe("count = 3,");
    expect(snippetText(entry("def:SpineStart").template!)).toBe(
      "{\n  mode = 'spine,\n  points = [\n    [0, 0],\n  ],\n}",
    );
  });

  it("reads guides from docs/reference, in file order", () => {
    expect(GUIDES[0].slug).toBe("writing");
    const typicals = entry("guide:typicals");
    expect(typicals).toMatchObject({ kind: "guide", title: "Typicals", section: "Guides" });
    expect(typicals.summary).toMatch(/^Rooms whose slots match/);
    // Every guide a schema node points at exists.
    for (const e of reference.entries.values()) {
      for (const g of [...e.guides, ...e.fields.flatMap((f) => f.guides)])
        expect(reference.entries.has(`guide:${g}`), `${e.id} → ${g}`).toBe(true);
    }
  });

  it("takes another schema (a user-supplied solver brings its own)", () => {
    const ref = buildReference(
      {
        type: "object",
        properties: { size: { description: "How big.", $ref: "#/$defs/Size" } },
        $defs: { Size: { description: "A size.", enum: ["s", "m"] } },
      },
      "{}",
      [],
    );
    expect(ref.entries.get("def:#")!.fields[0]).toMatchObject({ name: "size", typeLink: "def:Size" });
    expect(ref.entries.get("def:Size")!.values.map((v) => v.value)).toEqual(["s", "m"]);
  });
});

describe("search", () => {
  it("finds fields, values, rules and helpers by name", () => {
    expect(
      search(reference, "door")
        .slice(0, 2)
        .map((h) => h.entry),
    ).toEqual(expect.arrayContaining(["helper:door", "def:RuleDoor"]));
    expect(search(reference, "plate.door")[0]).toMatchObject({ entry: "helper:door" });
    expect(search(reference, "typical").map((h) => h.entry)).toEqual(
      expect.arrayContaining(["guide:typicals", "def:Space"]),
    );
    expect(search(reference, "wet")[0]).toMatchObject({ entry: "def:Port", label: "'wet" });
    expect(search(reference, "aspect")[0]).toMatchObject({ entry: "def:Space", field: "aspect" });
    expect(search(reference, "")).toEqual([]);
  });
});

describe("markdown", () => {
  it("renders the parts the guides use, escaped", () => {
    const html = renderMarkdown(
      "# Title\n\nSome `code` and **bold** <b>.\n\n- one\n- two\n\n```nickel\n{ a = 1 }\n```",
    );
    expect(html).toContain("<h3>Title</h3>");
    expect(html).toContain("<code>code</code>");
    expect(html).toContain("<strong>bold</strong> &lt;b&gt;.");
    expect(html).toContain("<ul><li>one</li><li>two</li></ul>");
    expect(html).toContain('<pre class="ref-code" data-lang="nickel"><code>{ a = 1 }</code></pre>');
  });

  it("links topics and code that names one", () => {
    expect(inline("See [Ports](guide:ports).")).toBe(
      'See <a href="#" class="ref-link" data-ref="guide:ports">Ports</a>.',
    );
    expect(inline("[x](docs/ARCHITECTURE.md)")).toBe("x");
    expect(
      inline("`plate.door`", { codeRef: (c) => (c === "plate.door" ? "helper:door" : undefined) }),
    ).toContain('data-ref="helper:door"');
  });
});
