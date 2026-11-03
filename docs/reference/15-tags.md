# Tags

A tag is a label on a space that rules and services select it by, whatever it's called. Ports let a plan talk about its parent without naming it; tags let a service talk about spaces without naming them.

```nickel
spaces = {
  bedroom = { count = 2, area.min = 10, tags = ['sleep] },
  bath = { area.min = 4.5, tags = ['wet] },
  toilet = { kind = 'fixture, width = 0.4, depth = 0.7, tags = ['water, 'drain] },
},
```

Select by tag anywhere a space name goes, with a `#`:

```nickel
plate.door "#sleep" "hall",
plate.over "smoke_alarm" "#sleep",
```

- Tags refine like port roles: `'wet.shower` is a kind of `'wet`.
- Every space has its kind as a tag too: `"#room"`, `"#circulation"`, `"#fixture"`, …
- A tag no space has selects nothing, quietly: a service can say "every sleeping room" in a plan that has none.
