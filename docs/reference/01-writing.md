# Writing a plate

A plate is a [Nickel](https://nickel-lang.org) file: a record of fields, each written `name = value,`. The solver reads it and lays out the drawing every time you stop typing.

```nickel
{
  name = "Typical floor",
  system = 'metric,
  start = { mode = 'boundary, points = [[0, 0], [42, 0], [42, 30], [0, 30]] },
  spaces = {
    living = { area.min = 24 },
  },
  rules = [],
}
```

## The pieces

- **Records** `{ … }` hold fields. A trailing comma is fine.
- **Tags** start with a quote: `'metric`, `'boundary`, `'facade`. They pick one of a fixed set of values.
- **Strings** are in double quotes: `"Typical floor"`, `"98'-6"`.
- **Lists** `[ … ]`: points, rules, port roles.
- **Dotted names** are short for nested records: `area.min = 24` is `area = { min = 24 }`.
- **Comments** start with `#`.
- **`let`** names a value to reuse: `let w = 42 in { … }`.

## Plates and plans

A file with a `start` is a **plate**: something to draw. Any other file is a **plan** (or a library of them) that a plate imports. See [Plans and ports](guide:plans-and-ports) and [Imports and cascading](guide:imports-and-cascading).

Every file can use the `plate` helpers, such as `plate.door "bedroom" "hall"`. Type `plate.` in the editor for the list.

## Finding your way

This panel follows the cursor: click into any field to read about it. Search finds fields, values and rules by name. **Insert** puts an example at the cursor.
