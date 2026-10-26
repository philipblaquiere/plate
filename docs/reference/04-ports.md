# Ports

A port is a role given to an edge of a space: where the entrance is, which walls face outside, which are shared with a neighbour. Rules in a plan use ports to talk about the outside world without naming it.

## Where ports come from

- **The boundary.** Its edges are `'facade` unless `start.ports` says otherwise. Right-click an edge in the drawing to set its role.
- **The parent plan.** When spaces are placed, each one gets ports from what is around it: `'entrance` on the corridor, `'facade` on the outside, `'party` between neighbours of the same type, `'wet` on every other party wall (so bathrooms land back to back), `'partition` elsewhere.
- **Doors and windows.** Each door the parent places gives the spaces on both sides a `'door` port, and each window gives its space a `'window` port: just the opening, not the whole wall.

## Using them

```nickel
rules = [
  plate.door "entry" 'entrance,              # a door onto the room's entrance edge
  plate.window "sleep",                      # a window on a 'facade edge
  plate.window "lounge" & { port = 'outdoor },
  plate.prefer (plate.touches "bath" 'wet),  # bathrooms back to back
  plate.apart "bed" 'window,                 # keep the bed off the window
],
```

`needs.ports` says which ports a plan requires: `{ entrance = 'required, facade = 'required, blank = 'forbidden }`.

## Refining roles

Roles are dotted paths. `'facade.storefront` is a kind of `'facade`: anything that asks for a facade accepts it, and you can still ask for a storefront specifically.
