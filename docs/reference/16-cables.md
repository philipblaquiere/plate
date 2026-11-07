# Cables

A network's groups are circuits, and each circuit is made of something: a cable for power, later a pipe or a duct. The service lists what it can use as `media`, and every circuit gets the **thinnest one that meets everything asked of it**, the same rule walls use to pick an assembly.

```nickel
media = {
  "14/2" = { conductors = 2, rating = "15 A", colour = 'white },
  "12/2" = { conductors = 2, rating = "20 A", colour = 'yellow },
  "10/3" = { conductors = 3, rating = "30 A", colour = 'orange },
  "6/3" = { conductors = 3, rating = "55 A", colour = 'black },
},
```

`rating` is the largest breaker a medium may be on, from the code you build to. Keep media in a library file and merge it in: `build/media/nm-b.ncl` (US) and `build/media/nmd90.ncl` (Canada) come with the examples, and each code file brings its own.

## Loads

A fixture says what it needs from each service, as its spec sheet does:

```nickel
dryer = { kind = 'fixture, …, tags = ['laundry], load.power = { current = "30 A", voltage = 240, conductors = 3 } },
```

A part placed on it (`plate.on "outlet_240" "#laundry"`) carries that load. A part can have its own: `baseboard = { …, load = { watts = 1000, voltage = 240 } }`. `current` is the circuit it asks for, `watts` what it draws; `conductors` (not counting the ground) is 3 for a range or a dryer (two hots and a neutral). The voltage defaults to 120.

Loads never change the layout: they're left out of a plan's identity, like symbols.

## Circuits

Each network's groups are cut by `max`, by voltage, and by load: a group carries at most 80 % of its breaker. A network's `rating` is its groups' breaker; without it, a group gets the smallest standard breaker its loads need, or, on a `dedicated` network, the `current` its terminal asks for. Then the cable is the thinnest medium with at least that rating and enough conductors:

| Circuit                                                   | Cable |
| --------------------------------------------------------- | ----- |
| 15 A general receptacles and switches                     | 14/2  |
| 20 A counter, a fridge                                    | 12/2  |
| 30 A dryer, 240 V with a neutral                          | 10/3  |
| 40 A range (Canada, where a ≤ 12 kW range counts as 8 kW) | 8/3   |
| 50 A range (US)                                           | 6/3   |

## Feeds

`feeds` say how particular terminals are served, by part or by the fixture they're on:

```nickel
feeds = [
  plate.feed "fridge" & { rating = "20 A" },                    # its circuit's breaker
  plate.feed "#cooking" & { dedicated = true, medium = "8/3" }, # its own circuit, in this cable
],
```

A named medium that's too small for its circuit is a problem pointing at it. Feeds never change the layout.

## Switch legs

```nickel
connect = [{ label = "Switch legs", role = 'control, from = "switch", to = ["light", "downlight"], via.ceilings = "#room" }],
```

In each room, the switches are joined to the lights they control. A room with two switches gets 3-conductor cable (a 3-way pair). With a power service whose circuits reach the switches (`to = ["outlet", "lighting.switch"]`), the lights' load goes on the room's first switch's circuit, its cable matches that circuit's breaker, and the Power view draws the switches, lights and legs too. A control network with no media of its own uses the power service's.

## The panel schedule

A service view's legend lists its circuits for one scope (one panel), with their breaker, cable, load, length and what's on them. Each circuit's number is drawn where its home run starts; hover a cable for its circuit, medium and length.

Check the edition your jurisdiction enforces: a circuit the solver sizes is sized as written, not certified.
