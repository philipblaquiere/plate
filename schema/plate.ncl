# The `plate` library: friendly helpers for writing rules. Every file can use it as `plate`.
#
# Each helper returns plain rule records (see `Rule` in plate.schema.json), so you can always
# write a rule by hand instead, or merge options into one: `plate.door "a" "b" & { width = 1.2 }`.
#
# Arguments named `a`/`b` are selectors: a space name ("kitchen"), a space type ("bedroom"
# matches bedroom_1, bedroom_2, …), a pattern ("bed*"), or a list of these. Where a port of the
# parent space is allowed, pass a role such as 'entrance or 'facade instead of a name.
#
# Note: parameters are called x/y rather than a/b because Nickel records are recursive:
# `{ a = a }` would refer to itself.
let side = fun field y =>
  if std.is_enum y then { port = y } else std.record.insert field y {}
in
{
  touches
    | doc "Each `a` shares a wall with a `b` (or with a port of the parent space, e.g. 'facade). `plate.touches \"kitchen\" \"living\"`"
    = fun x y => { rule = 'touch, a = x } & side "b" y,

  door
    | doc "Each `a` gets a door to a `b`, or to a port of the parent space such as 'entrance. `plate.door \"bedroom\" \"hall\"`"
    = fun x y => { rule = 'door, a = x } & side "b" y,

  window
    | doc "Each `a` gets a window on the facade. `plate.window \"bedroom\"`; for another port: `plate.window \"lounge\" & { port = 'outdoor }`"
    = fun x => { rule = 'window, a = x },

  apart
    | doc "No `a` touches a `b`, or a port of the parent space such as 'window. `plate.apart \"bedroom\" \"kitchen\"`, `plate.apart \"bed\" 'window`; keep a gap: `& { distance = 0.5 }`; the whole wall a port is on: `& { wall = true }`"
    = fun x y => { rule = 'apart, a = x } & side "b" y,

  near
    | doc "Each `a` is within `distance` of a `b`. `plate.near \"bath\" \"bedroom\" 3`"
    = fun x y d => { rule = 'near, a = x, b = y, distance = d },

  faces
    | doc "Each `a` (a fixture) faces a `b`: it's in front of it, across from its front. `plate.faces \"sofa\" \"tv\"`"
    = fun x y => { rule = 'faces, a = x, b = y },

  between
    | doc "`mid` separates the `a`s: each `a` touches `mid`, and no two `a`s touch. `plate.between \"bedroom\" \"hall\"`"
    = fun x mid => [
      { rule = 'touch, a = x, b = mid },
      { rule = 'apart, a = x, b = x },
    ],

  through
    | doc "Going from one `a` to another through doors always passes through `via`. `plate.through \"bedroom\" \"hall\"`"
    = fun x via_ => { rule = 'reach, a = x, b = x, via = via_ },

  reach
    | doc "Going from `a` to `b` through doors always passes through `via`. `plate.reach \"bedroom\" \"kitchen\" \"hall\"`"
    = fun x y via_ => { rule = 'reach, a = x, b = y, via = via_ },

  along
    | doc "Spaces `a` line up along the corridor `b` (spine_stack engine). `plate.along \"room\" \"corridor\"`; one side only: `& { sides = 'left }`"
    = fun x y => { rule = 'along, a = x, b = y },

  unique
    | doc "These instances get their own solution instead of sharing their typical. `plate.unique \"room_7\"`"
    = fun x => { rule = 'unique, a = x },

  instance
    | doc "Give particular instances a different plan. `plate.instance \"room_1\" accessible_room`"
    = fun x p => { rule = 'instance, a = x, plan = p },

  prefer
    | doc "Make rules preferences rather than requirements. `plate.prefer (plate.touches \"bath\" \"bedroom\")`; weigh it: `plate.prefer_by 3 (...)`"
    = fun r => if std.is_array r then std.array.map (fun x => x & { weight = 1 }) r else r & { weight = 1 },

  prefer_by
    | doc "Like `prefer`, with a weight: how much the rule matters compared with the goals (1 is normal)."
    = fun w r => if std.is_array r then std.array.map (fun x => x & { weight = w }) r else r & { weight = w },
}
