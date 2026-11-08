//! The connect pass: each service's networks, once its parts are placed.
//!
//! A **feed** network (`connect`, the default role) joins terminals (parts, or tagged fixtures)
//! back to a source, one network per scope (a panel per flat). Terminals are taken room by room
//! from the source out and cut into groups (circuits) by `max`, by load (at most 80 % of the
//! breaker) and by voltage; `dedicated` terminals get a group each. Each group is a tree: its
//! terminals joined by the shortest runs between them (boxes branch, as real ones do), plus one
//! home run back to the source. Each joint of the tree is one cable, made of the thinnest medium
//! that meets its group's breaker and conductors (or the one a `feed` or the network names).
//!
//! A **control** network (`role = 'control`) joins, in each room, the switches that serve it
//! (`Placed::serving`: the room they're in, or the rooms a region's box serves) to its lights.
//! The lights' load goes on the circuit of the room's first switch, and they're drawn in that
//! circuit's service view as well as their own.
//!
//! Runs are laid on [`super::route`]: in the walls, up over doors, across ceilings. Every
//! network is also drawn as the schematic (`'arcs`, the electrical convention); `draw` says which
//! one exports keep.

use std::collections::{BTreeMap, HashMap};

use crate::geometry::{point_segment_distance, Point};
use crate::layout::{Circuit, Element, ElementKind, RunInfo, RunMark};
use crate::plan::{tag_of, Selector};
use crate::service::{Draw, Feed, Load, Medium, Network, Role};

use super::fit::{Face, Fixture, Instance, Issues, Model, Placed, Room};
use super::num;
use super::route::{Along, Edge, Route, Spot};

/// Something a network joins: a placed part, or a fixture.
#[derive(Debug, Clone)]
struct Node {
    at: Point,
    room: String,
    label: String,
    part: Option<usize>,
    /// The fixture it is, or is on (an element index).
    fixture: Option<usize>,
    /// The room it's in, as an index into the model's rooms.
    in_room: Option<usize>,
    /// The face it's against (room index, face index), if it's against a wall.
    face: Option<(usize, usize)>,
    ceiling: bool,
    above: Option<f64>,
    load: Option<Load>,
}

impl Node {
    fn spot(&self) -> Spot {
        Spot { at: self.at, face: self.face, room: self.in_room, above: self.above, ceiling: self.ceiling }
    }
}

/// Shortest ways from one node: distances, and the way back.
type Ways = (Vec<f64>, Vec<Option<(usize, usize)>>);

/// Standard breaker sizes, in amps.
const BREAKERS: &[f64] = &[15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];
/// A circuit carries at most this share of its breaker (continuous loads).
const CONTINUOUS: f64 = 0.8;
/// Cables side by side in one cavity are drawn this far apart.
const LANE_MM: f64 = 25.0;

pub fn run(
    model: &Model,
    instances: &[Instance],
    placed: &mut [Placed],
    elements: &mut Vec<Element>,
    issues: &mut Issues,
) -> Vec<Circuit> {
    // A part on a fixture with no load of its own takes the fixture's, for its service.
    for p in placed.iter_mut() {
        if p.load.is_none() {
            if let Some(info) = p.fixture.and_then(|f| elements[f].space.as_ref()) {
                p.load = info.load.get(&p.service).copied();
            }
        }
    }
    let mut out = Out::default();
    // Switches and what they control, per room.
    let controls = controls(model, instances, placed, elements);
    for c in &controls {
        let lights: f64 = c.lights.iter().filter_map(|&k| placed[k].load).map(|l| l.amps()).sum();
        if let Some(&first) = c.switches.first() {
            *out.extra.entry(first).or_default() += lights;
        }
    }
    for inst in instances {
        let mut number = 0;
        for (ni, net) in inst.def.connect.iter().enumerate() {
            if net.role != Role::Feed {
                continue;
            }
            let pointer = format!("{}/connect/{ni}", inst.def.source.clone().unwrap_or_default());
            let job = Job { model, inst, ni, net, pointer: &pointer, pool: pool(inst, net, instances) };
            job.feed(placed, elements, &mut out, issues, &mut number);
        }
    }
    // Control networks, once the circuits that feed them are known.
    let mut by_net: BTreeMap<(usize, usize), Vec<&Control>> = BTreeMap::new();
    for c in &controls {
        by_net.entry((c.inst, c.ni)).or_default().push(c);
    }
    for ((ii, ni), list) in by_net {
        let inst = &instances[ii];
        let net = &inst.def.connect[ni];
        let pointer = format!("{}/connect/{ni}", inst.def.source.clone().unwrap_or_default());
        let job = Job { model, inst, ni, net, pointer: &pointer, pool: pool(inst, net, instances) };
        job.control(&list, placed, elements, &mut out);
    }
    elements.extend(out.runs);
    out.circuits
}

/// What the pass builds up.
#[derive(Default)]
struct Out {
    runs: Vec<Element>,
    circuits: Vec<Circuit>,
    /// Load a part carries for others (a switch, its room's lights), in amps.
    extra: BTreeMap<usize, f64>,
    /// The circuit feeding a part of another service: (service, network, group, breaker).
    fed: BTreeMap<usize, (String, String, String, Option<f64>)>,
    /// Cables already drawn along each stretch, to draw the next beside them.
    lanes: HashMap<(i64, i64, i64, i64, u8), u32>,
    /// Cables drawn so far.
    cables: u32,
}

/// The media a network can use: its service's own. A control network with none uses another
/// service's in scope (the switch legs of lighting are wired with power's cable).
fn pool<'a>(inst: &'a Instance, net: &Network, instances: &'a [Instance]) -> &'a BTreeMap<String, Medium> {
    if !inst.def.media.is_empty() || net.role != Role::Control {
        return &inst.def.media;
    }
    instances
        .iter()
        .find(|i| {
            !i.def.media.is_empty()
                && (inst.prefix.starts_with(&i.prefix) || i.prefix.starts_with(&inst.prefix))
        })
        .map_or(&inst.def.media, |i| &i.def.media)
}

/// The parts and fixtures in this instance a selector names.
fn nodes(
    model: &Model,
    inst: &Instance,
    sel: &Selector,
    placed: &[Placed],
    elements: &[Element],
    runs: &dyn Fn(&Face) -> bool,
) -> Vec<Node> {
    let mut out = Vec::new();
    for item in sel.items() {
        if let Some(tag) = tag_of(item) {
            for (r, room) in model.rooms.iter().enumerate().filter(|(_, r)| inst.covers(&r.path)) {
                for f in &room.fixtures {
                    let Some(info) = &elements[f.element].space else { continue };
                    if crate::plan::has_tag(&info.tags, info.kind, tag) {
                        let (at, face) = joint(
                            room,
                            f,
                            &elements[f.element].points,
                            model.units.tolerance() * 100.0,
                            runs,
                        );
                        let face = face.map(|k| (r, k));
                        out.push(Node {
                            at,
                            room: room.path.clone(),
                            label: info.label.clone(),
                            part: None,
                            fixture: Some(f.element),
                            in_room: Some(r),
                            face,
                            ceiling: false,
                            above: None,
                            load: info.load.get(&inst.service).copied(),
                        });
                    }
                }
            }
            continue;
        }
        let (service, part) = item.split_once('.').unwrap_or((inst.service.as_str(), item));
        for (k, p) in placed.iter().enumerate() {
            if p.service == service && p.part == part && inst.covers(&p.room) {
                out.push(part_node(model, k, p));
            }
        }
    }
    out
}

/// For a source about to be placed: from each of `spots`, the longest run the network would
/// take to the terminals it feeds in `rooms` (all of its terminals, if none are there). A `one`
/// placement uses it to keep a network's runs within its `limit`.
pub(super) fn reach(
    model: &Model,
    inst: &Instance,
    net: &Network,
    placed: &[Placed],
    elements: &[Element],
    rooms: &[usize],
    spots: &[Spot],
) -> Vec<f64> {
    let need = net.via.cavity.map_or(0.0, |c| c.0.to(model.units));
    let runs = |face: &Face| {
        let blocked = face
            .wall
            .and_then(|w| elements[w].wall.as_ref())
            .is_some_and(|w| w.cross == crate::assembly::Cross::No.name());
        face.cavity + 1e-9 >= need && !blocked
    };
    let all = nodes(model, inst, &net.to, placed, elements, &runs);
    let here: Vec<&Node> = all.iter().filter(|n| n.in_room.is_some_and(|r| rooms.contains(&r))).collect();
    let terminals: Vec<&Node> = if here.is_empty() { all.iter().collect() } else { here };
    if terminals.is_empty() {
        return vec![0.0; spots.len()];
    }
    let mut every: Vec<Spot> = spots.to_vec();
    every.extend(terminals.iter().map(|n| n.spot()));
    let route = Route::build(model, inst, &net.via, elements, &every);
    let mut far = vec![0.0_f64; spots.len()];
    for t in &route.spots[spots.len()..] {
        let (dist, _) = route.shortest(&[*t]);
        for (k, id) in route.spots[..spots.len()].iter().enumerate() {
            far[k] = far[k].max(dist[*id]);
        }
    }
    far
}

/// Where a fixture joins a network: the middle of its back, or, when that's on a wall the
/// network can't run in, the nearest point of its outline that is on a wall it can. A sink whose
/// counter backs on a plain partition drains into the plumbing wall its end stands against.
fn joint(
    room: &Room,
    f: &Fixture,
    outline: &[Point],
    tol: f64,
    runs: &dyn Fn(&Face) -> bool,
) -> (Point, Option<usize>) {
    let mid = Point::new((f.back.0.x + f.back.1.x) / 2.0, (f.back.0.y + f.back.1.y) / 2.0);
    let on_mid = room
        .faces
        .iter()
        .enumerate()
        .filter(|(_, face)| face.walled)
        .map(|(k, face)| (k, point_segment_distance(mid, face.a, face.b)))
        .filter(|(_, d)| *d <= tol)
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(k, _)| k);
    if on_mid.is_none_or(|k| runs(&room.faces[k])) {
        return (mid, on_mid);
    }
    // Each side of the outline along a wall the network runs in, at its point nearest the middle.
    let n = outline.len();
    let better = room
        .faces
        .iter()
        .enumerate()
        .filter(|(_, face)| face.walled && runs(face))
        .flat_map(|(k, face)| {
            (0..n).filter_map(move |i| {
                let (s0, d0) = face.locate(outline[i]);
                let (s1, d1) = face.locate(outline[(i + 1) % n]);
                if d0.max(d1) > tol {
                    return None;
                }
                let (lo, hi) = (s0.min(s1).max(0.0), s0.max(s1).min(face.len()));
                (hi - lo > tol).then(|| {
                    let p = face.point(face.locate(mid).0.clamp(lo, hi));
                    (p.distance(mid), k, p)
                })
            })
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    match better {
        Some((_, k, p)) => (p, Some(k)),
        None => (mid, on_mid),
    }
}

fn part_node(model: &Model, k: usize, p: &Placed) -> Node {
    Node {
        at: p.at,
        room: p.room.clone(),
        label: p.label.clone(),
        part: Some(k),
        fixture: p.fixture,
        in_room: model.rooms.iter().position(|r| r.path == p.room),
        face: p.face,
        ceiling: p.mount == crate::service::Mount::Ceiling,
        above: p.above,
        load: p.load,
    }
}

/// One room's switches and the lights they control, for a control network.
struct Control {
    inst: usize,
    ni: usize,
    switches: Vec<usize>,
    lights: Vec<usize>,
}

fn controls(model: &Model, instances: &[Instance], placed: &[Placed], elements: &[Element]) -> Vec<Control> {
    let mut out = Vec::new();
    for (ii, inst) in instances.iter().enumerate() {
        for (ni, net) in inst.def.connect.iter().enumerate().filter(|(_, n)| n.role == Role::Control) {
            let from = nodes(model, inst, &net.from, placed, elements, &|_| true);
            let to = nodes(model, inst, &net.to, placed, elements, &|_| true);
            let mut rooms: Vec<&str> = to.iter().map(|n| n.room.as_str()).collect();
            rooms.dedup();
            let mut seen: Vec<&str> = Vec::new();
            for room in rooms {
                if seen.contains(&room) {
                    continue;
                }
                seen.push(room);
                // The switches for this room, wherever they're mounted (a box by a region's
                // way in has one for each of its rooms).
                let switches: Vec<usize> = from
                    .iter()
                    .filter_map(|n| n.part)
                    .filter(|&k| placed[k].serving().contains(&room))
                    .collect();
                let lights: Vec<usize> =
                    to.iter().filter(|n| n.room == room).filter_map(|n| n.part).collect();
                if !switches.is_empty() && !lights.is_empty() {
                    out.push(Control { inst: ii, ni, switches, lights });
                }
            }
        }
    }
    out
}

struct Job<'a> {
    model: &'a Model,
    inst: &'a Instance,
    ni: usize,
    net: &'a Network,
    pointer: &'a str,
    pool: &'a BTreeMap<String, Medium>,
}

/// A group being formed: a circuit.
struct Group {
    members: Vec<usize>,
    volts: f64,
    /// Whether any member says what it draws.
    loaded: bool,
    load: f64,
    rating: Option<f64>,
    conductors: u32,
    /// A medium a feed asks for, and the feed's pointer.
    medium: Option<(String, String)>,
    dedicated: bool,
}

/// One cable to draw: its pieces, where it turns up or through a wall, and its length.
struct Laid {
    pieces: Vec<(Along, Vec<Point>)>,
    marks: Vec<RunMark>,
    length: f64,
}

impl Job<'_> {
    fn network(&self) -> String {
        self.net.label.clone().unwrap_or_else(|| self.inst.service.clone())
    }

    /// Does a feed select this terminal?
    fn selects(&self, f: &Feed, t: &Node, placed: &[Placed], elements: &[Element]) -> bool {
        if f.network.as_ref().is_some_and(|n| Some(n) != self.net.label.as_ref()) {
            return false;
        }
        let fixture = t.fixture.and_then(|e| elements[e].space.as_ref());
        f.feed.items().iter().any(|item| {
            if let Some(info) = fixture {
                let one = Selector::One((*item).to_owned());
                if one.matches_tagged(&info.name, &info.type_key, &info.tags, info.kind) {
                    return true;
                }
            }
            t.part.is_some_and(|k| {
                let p = &placed[k];
                let (service, part) = item.split_once('.').unwrap_or((self.inst.service.as_str(), item));
                p.service == service && p.part == part
            })
        })
    }

    /// The thinnest medium with at least this rating and these conductors.
    fn thinnest(&self, rating: Option<f64>, conductors: u32) -> Option<String> {
        self.pool
            .iter()
            .filter(|(_, m)| m.rating.0 + 1e-9 >= rating.unwrap_or(0.0) && m.conductors >= conductors)
            .min_by(|a, b| a.1.rating.0.total_cmp(&b.1.rating.0).then(a.1.conductors.cmp(&b.1.conductors)))
            .map(|(k, _)| k.clone())
    }

    fn colour(&self, medium: Option<&str>) -> Option<&'static str> {
        medium.and_then(|m| self.pool.get(m)).and_then(|m| m.colour).map(|c| c.name())
    }

    fn feed(
        &self,
        placed: &mut [Placed],
        elements: &[Element],
        out: &mut Out,
        issues: &mut Issues,
        number: &mut u32,
    ) {
        let (model, inst, net, pointer) = (self.model, self.inst, self.net, self.pointer);
        let units = model.units;
        let m = 1000.0 / units.to_mm();
        let scope = inst.space.clone().unwrap_or_default();
        let runs = |face: &Face| self.runs_in(face, elements);
        let sources = nodes(model, inst, &net.from, placed, elements, &runs);
        let terminals = nodes(model, inst, &net.to, placed, elements, &runs);
        if terminals.is_empty() {
            return;
        }
        let Some(source) = sources.first().cloned() else {
            issues.add(
                pointer,
                format!("{}: nothing to connect to, no {} here", inst.service, net.from.items().join(" or ")),
                &scope,
                Some("Place the source part (a `one` placement), or tag the fixture it starts from.".into()),
            );
            return;
        };
        let network = self.network();
        self.shallow(&sources, &terminals, elements, issues);

        // Terminals in order: from the source out, room by room.
        let mut order: Vec<usize> = Vec::new();
        let mut left: Vec<usize> = (0..terminals.len()).collect();
        let (mut at, mut room) = (source.at, source.room.clone());
        while !left.is_empty() {
            let pos = left
                .iter()
                .enumerate()
                .map(|(pos, &t)| {
                    let away = if terminals[t].room == room { 0.0 } else { 1000.0 * m };
                    (pos, terminals[t].at.distance(at) + away)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                .map_or(0, |x| x.0);
            let t = left.remove(pos);
            at = terminals[t].at;
            room = terminals[t].room.clone();
            order.push(t);
        }

        // Groups: by count, load and voltage; dedicated terminals alone.
        let feeds: Vec<(usize, &Feed)> = inst.def.feeds.iter().enumerate().collect();
        let feed_pointer = |i: usize| format!("{}/feeds/{i}", inst.def.source.clone().unwrap_or_default());
        let max = net.max.map(|n| n.max(1) as usize);
        let mut groups: Vec<Group> = Vec::new();
        let mut open: BTreeMap<String, usize> = BTreeMap::new();
        let extra = out.extra.clone();
        let amps_of = |t: &Node| {
            t.load.map_or(0.0, |l| l.amps()) + t.part.and_then(|k| extra.get(&k)).copied().unwrap_or(0.0)
        };
        for &t in &order {
            let node = &terminals[t];
            let mine: Vec<&(usize, &Feed)> =
                feeds.iter().filter(|(_, f)| self.selects(f, node, placed, elements)).collect();
            let dedicated = net.dedicated || mine.iter().any(|(_, f)| f.dedicated);
            let rating = mine
                .iter()
                .filter_map(|(_, f)| f.rating.map(|r| r.0))
                .fold(None, |a: Option<f64>, r| Some(a.map_or(r, |a| a.max(r))));
            let medium = mine.iter().find_map(|(i, f)| f.medium.clone().map(|m| (m, feed_pointer(*i))));
            let amps = amps_of(node);
            let volts = node.load.map_or(120.0, |l| l.volts());
            let conductors = node.load.and_then(|l| l.conductors).unwrap_or(2);
            let key = format!("{volts}|{rating:?}|{medium:?}|{conductors}");
            let cap = rating.or(net.rating.map(|r| r.0)).map(|r| r * CONTINUOUS);
            let loaded = node.load.is_some() || amps > 0.0;
            if !dedicated {
                if let Some(&g) = open.get(&key) {
                    let grp = &groups[g];
                    let room_ok = max.is_none_or(|n| grp.members.len() < n);
                    let load_ok = cap.is_none_or(|c| grp.load + amps <= c + 1e-9);
                    if room_ok && load_ok {
                        let grp = &mut groups[g];
                        grp.members.push(t);
                        grp.load += amps;
                        grp.loaded |= loaded;
                        continue;
                    }
                }
            }
            groups.push(Group {
                members: vec![t],
                volts,
                loaded,
                load: amps,
                rating,
                conductors,
                medium,
                dedicated,
            });
            if !dedicated {
                open.insert(key, groups.len() - 1);
            }
        }

        // The route, and every group laid on it.
        let spots: Vec<Spot> = sources.iter().chain(&terminals).map(Node::spot).collect();
        let route = Route::build(model, inst, &net.via, elements, &spots);
        let source_ids = &route.spots[..sources.len()];
        let term_ids = &route.spots[sources.len()..];
        let starts: Vec<usize> =
            if net.draw == Draw::Arcs { source_ids[..1].to_vec() } else { source_ids.to_vec() };
        let (dist0, prev0) = route.shortest(&starts);
        let limit = net.limit.map(|l| l.0.to(units));
        for grp in &groups {
            *number += 1;
            let label = format!("{number}");
            // The breaker: as asked, else as the loads need.
            let biggest = grp.members.iter().map(|&t| amps_of(&terminals[t])).fold(0.0, f64::max);
            let rating = grp.rating.or(net.rating.map(|r| r.0)).or_else(|| {
                (grp.load > 0.0).then(|| {
                    let need = biggest.max(grp.load / CONTINUOUS);
                    BREAKERS.iter().copied().find(|&b| b + 1e-9 >= need).unwrap_or(need)
                })
            });
            // A terminal asking for a circuit (`current`) asks for that breaker.
            let rating = match (grp.dedicated, rating) {
                (true, r) => {
                    let asked = grp
                        .members
                        .iter()
                        .filter_map(|&t| terminals[t].load.and_then(|l| l.current))
                        .map(|a| a.0)
                        .fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))));
                    match (grp.rating, asked) {
                        (Some(r), _) => Some(r),
                        (None, Some(a)) => Some(a),
                        (None, None) => r,
                    }
                }
                (false, r) => r,
            };
            if let Some(r) =
                rating.filter(|r| grp.load > r * if grp.dedicated { 1.0 } else { CONTINUOUS } + 1e-9)
            {
                let what = &terminals[grp.members[0]].label;
                issues.add(
                    pointer,
                    format!("circuit {label} carries {} A, more than its {} A breaker allows", num(grp.load), num(r)),
                    &terminals[grp.members[0]].room,
                    Some(format!("Give the {what} its own circuit (`plate.feed` with `dedicated = true`), or raise the network's `rating`.")),
                );
            }
            // The medium: as asked, else the thinnest that does.
            let asked =
                grp.medium.clone().or_else(|| net.medium.clone().map(|m| (m, format!("{pointer}/medium"))));
            let medium = match asked {
                Some((name, at)) => {
                    if let Some(md) = self.pool.get(&name) {
                        if let Some(r) = rating.filter(|r| md.rating.0 + 1e-9 < *r) {
                            issues.add(
                                &at,
                                format!("{name} is rated {} A, but circuit {label} is on {} A", num(md.rating.0), num(r)),
                                &terminals[grp.members[0]].room,
                                Some("Name a heavier medium, or leave `medium` out to pick the thinnest that does.".into()),
                            );
                        }
                        if md.conductors < grp.conductors {
                            issues.add(
                                &at,
                                format!(
                                    "{name} has {} conductors; circuit {label} needs {}",
                                    md.conductors, grp.conductors
                                ),
                                &terminals[grp.members[0]].room,
                                None,
                            );
                        }
                    }
                    Some(name)
                }
                None => {
                    let found = self.thinnest(rating, grp.conductors);
                    if found.is_none() && !self.pool.is_empty() {
                        issues.add(
                            pointer,
                            format!(
                                "no medium carries circuit {label}: {} A with {} conductors",
                                num(rating.unwrap_or(0.0)),
                                grp.conductors
                            ),
                            &terminals[grp.members[0]].room,
                            Some("Add a heavier medium to the service's `media`.".into()),
                        );
                    }
                    found
                }
            };
            for &t in &grp.members {
                if let Some(k) = terminals[t].part {
                    placed[k].group = Some(label.clone());
                    placed[k].network = Some(network.clone());
                    if placed[k].service != inst.service {
                        placed[k].fed_by = Some(inst.service.clone());
                        out.fed.insert(k, (inst.service.clone(), network.clone(), label.clone(), rating));
                    }
                }
            }

            // The tree: each terminal joined to the nearest one already in it.
            let ids: Vec<usize> = grp.members.iter().map(|&t| term_ids[t]).collect();
            let ways: Vec<Ways> = ids.iter().map(|&i| route.shortest(&[i])).collect();
            let mut joined = vec![false; ids.len()];
            let mut tree: Vec<(usize, usize)> = Vec::new();
            let mut homes: Vec<usize> = Vec::new();
            while let Some(first) = joined.iter().position(|j| !j) {
                // A new part of the tree starts from whichever is nearest the source.
                let start = (0..ids.len())
                    .filter(|&i| !joined[i])
                    .min_by(|&a, &b| dist0[ids[a]].total_cmp(&dist0[ids[b]]).then(a.cmp(&b)))
                    .unwrap_or(first);
                joined[start] = true;
                homes.push(start);
                loop {
                    let next = (0..ids.len())
                        .filter(|&i| joined[i])
                        .flat_map(|i| (0..ids.len()).filter(|&j| !joined[j]).map(move |j| (i, j)))
                        .map(|(i, j)| (i, j, ways[i].0[ids[j]]))
                        .filter(|x| x.2.is_finite())
                        .min_by(|a, b| a.2.total_cmp(&b.2).then(a.1.cmp(&b.1)));
                    let Some((i, j, _)) = next else { break };
                    joined[j] = true;
                    tree.push((i, j));
                }
            }
            let mut length = 0.0;
            let colour = self.colour(medium.as_deref());
            let info = |home: bool, draw: &'static str| RunInfo {
                service: inst.service.clone(),
                network: network.clone(),
                group: Some(label.clone()),
                home,
                draw,
                alt: (draw == "arcs") != (net.draw == Draw::Arcs),
                along: None,
                cable: None,
                length: None,
                medium: medium.clone(),
                colour,
                control: false,
                fed_by: None,
                layer: medium.as_deref().and_then(|m| self.pool.get(m)).and_then(|m| m.layer.clone()),
                marks: Vec::new(),
            };
            for &(i, j) in &tree {
                let steps = route.path(&ways[i].1, ids[j]);
                let laid = self.lay(&route, &steps, elements, &mut out.lanes);
                length += laid.length;
                self.emit(laid, info(false, "routed"), &scope, out);
                self.push(
                    arc(terminals[grp.members[i]].at, terminals[grp.members[j]].at),
                    info(false, "arcs"),
                    &scope,
                    out,
                );
            }
            for &h in &homes {
                let t = &terminals[grp.members[h]];
                if dist0[ids[h]].is_finite() {
                    // From the terminal back to the source, so the cable starts at the circuit.
                    let mut steps = route.path(&prev0, ids[h]);
                    reverse(&route, &mut steps, ids[h]);
                    let laid = self.lay(&route, &steps, elements, &mut out.lanes);
                    length += laid.length;
                    self.emit(laid, info(true, "routed"), &scope, out);
                } else if self.shallow_one(t, elements).is_none() {
                    let hint = match net.via.ceilings {
                        Some(_) => "No wall it can run in leads there: give the walls on the way a cavity deep enough.",
                        None => "No wall it can run in leads there: give the walls on the way a cavity, or let it cross ceilings (`via.ceilings`).",
                    };
                    issues.add(
                        pointer,
                        format!("can't reach a {} from the {}", t.label, source.label),
                        &t.room,
                        Some(hint.into()),
                    );
                }
                // The schematic's home run: an arrow toward the source.
                let d = t.at.distance(source.at);
                if d > 1e-9 {
                    let len = (0.6 * m).min(d * 0.5);
                    let u = Point::new((source.at.x - t.at.x) / d, (source.at.y - t.at.y) / d);
                    self.push(
                        vec![t.at, Point::new(t.at.x + u.x * len, t.at.y + u.y * len)],
                        info(true, "arcs"),
                        &scope,
                        out,
                    );
                }
            }
            if let Some(limit) = limit {
                for &t in &grp.members {
                    let d = dist0[term_ids[t]];
                    if d.is_finite() && d > limit + 1e-9 {
                        let what = &terminals[t].label;
                        issues.add(
                            pointer,
                            format!("a {what} is {} from the {} (at most {})", num(d), source.label, num(limit)),
                            &terminals[t].room,
                            Some(format!("Bring the {what} closer to the {} (a claim such as `plate.near`), or allow a longer run.", source.label)),
                        );
                    }
                }
            }
            let mut devices: Vec<(String, u32)> = Vec::new();
            for &t in &grp.members {
                let l = &terminals[t].label;
                match devices.iter_mut().find(|d| &d.0 == l) {
                    Some(d) => d.1 += 1,
                    None => devices.push((l.clone(), 1)),
                }
            }
            out.circuits.push(Circuit {
                service: inst.service.clone(),
                network: network.clone(),
                scope: scope.clone(),
                group: label,
                rating,
                voltage: grp.loaded.then_some(grp.volts),
                medium,
                devices,
                load: grp.load,
                length,
            });
        }
    }

    /// Switch legs: in each room, its switches and lights joined in a tree.
    fn control(&self, list: &[&Control], placed: &mut [Placed], elements: &[Element], out: &mut Out) {
        let (model, inst, net) = (self.model, self.inst, self.net);
        let network = net.label.clone().unwrap_or_else(|| "Switch legs".into());
        let scope = inst.space.clone().unwrap_or_default();
        let mut all: Vec<usize> = Vec::new();
        for c in list {
            all.extend(&c.switches);
            all.extend(&c.lights);
        }
        let spots: Vec<Spot> = all.iter().map(|&k| part_node(model, k, &placed[k]).spot()).collect();
        let route = Route::build(model, inst, &net.via, elements, &spots);
        let id_of = |k: usize| route.spots[all.iter().position(|&x| x == k).unwrap_or(0)];
        for c in list {
            let fed = c.switches.iter().find_map(|k| out.fed.get(k).cloned());
            for &l in &c.lights {
                if let Some((service, network, group, _)) = &fed {
                    placed[l].fed_by = Some(service.clone());
                    placed[l].group = Some(group.clone());
                    placed[l].network = Some(network.clone());
                }
            }
            let members: Vec<usize> = c.switches.iter().chain(&c.lights).copied().collect();
            let ids: Vec<usize> = members.iter().map(|&k| id_of(k)).collect();
            // A room switched from two places needs travellers: three conductors.
            let conductors = if c.switches.len() > 1 { 3 } else { 2 };
            let rating = fed.as_ref().and_then(|f| f.3);
            let medium = net.medium.clone().or_else(|| self.thinnest(rating, conductors));
            let colour = self.colour(medium.as_deref());
            let info = |draw: &'static str| RunInfo {
                service: inst.service.clone(),
                network: network.clone(),
                group: fed.as_ref().map(|f| f.2.clone()),
                home: false,
                draw,
                alt: (draw == "arcs") != (net.draw == Draw::Arcs),
                along: None,
                cable: None,
                length: None,
                medium: medium.clone(),
                colour,
                control: true,
                fed_by: fed.as_ref().map(|f| f.0.clone()),
                layer: None,
                marks: Vec::new(),
            };
            let ways: Vec<_> = ids.iter().map(|&i| route.shortest(&[i])).collect();
            let mut joined = vec![false; ids.len()];
            joined[0] = true;
            loop {
                let next = (0..ids.len())
                    .filter(|&i| joined[i])
                    .flat_map(|i| (0..ids.len()).filter(|&j| !joined[j]).map(move |j| (i, j)))
                    .map(|(i, j)| (i, j, ways[i].0[ids[j]]))
                    .filter(|x| x.2.is_finite())
                    .min_by(|a, b| a.2.total_cmp(&b.2).then(a.1.cmp(&b.1)));
                let Some((i, j, _)) = next else { break };
                joined[j] = true;
                let steps = route.path(&ways[i].1, ids[j]);
                let laid = self.lay(&route, &steps, elements, &mut out.lanes);
                self.emit(laid, info("routed"), &scope, out);
                self.push(arc(placed[members[i]].at, placed[members[j]].at), info("arcs"), &scope, out);
            }
        }
    }

    /// Wall parts on walls the network can't run in: said once each.
    fn shallow(&self, sources: &[Node], terminals: &[Node], elements: &[Element], issues: &mut Issues) {
        let starts = if self.net.draw == Draw::Arcs { &sources[..1] } else { sources };
        let mut said: Vec<usize> = Vec::new();
        let need = self.net.via.cavity.map_or(0.0, |c| c.0.to(self.model.units));
        for t in starts.iter().chain(terminals) {
            let Some(cavity) = self.shallow_one(t, elements) else { continue };
            if let Some(k) = t.part {
                if said.contains(&k) {
                    continue;
                }
                said.push(k);
            }
            let (r, f) = t.face.expect("a shallow part is on a face");
            let face = &self.model.rooms[r].faces[f];
            let wall = face
                .wall
                .and_then(|w| elements[w].wall.as_ref())
                .map_or_else(|| "it".to_owned(), |w| w.label.clone());
            issues.add(
                self.pointer,
                format!("a {} is on a wall it can't run in ({wall} has a cavity of {}; it needs {})", t.label, num(cavity), num(need)),
                &t.room,
                Some("Give that wall's assembly a cavity, or keep the part off it (`wall.cavity` on a `one` placement).".into()),
            );
        }
    }

    /// The cavity of the wall a part is on, if the network can't run in it.
    fn shallow_one(&self, t: &Node, elements: &[Element]) -> Option<f64> {
        let (r, f) = t.face?;
        let face = &self.model.rooms[r].faces[f];
        (!self.runs_in(face, elements)).then_some(face.cavity)
    }

    /// Can the network run in the wall behind a face?
    fn runs_in(&self, face: &Face, elements: &[Element]) -> bool {
        let need = self.net.via.cavity.map_or(0.0, |c| c.0.to(self.model.units));
        let blocked = face
            .wall
            .and_then(|w| elements[w].wall.as_ref())
            .is_some_and(|w| w.cross == crate::assembly::Cross::No.name());
        face.cavity + 1e-9 >= need && !blocked
    }

    /// A way through the route as pieces to draw: where it turns up a wall or passes through
    /// one, beside the cables already drawn there.
    fn lay(
        &self,
        route: &Route,
        steps: &[(usize, Edge)],
        elements: &[Element],
        lanes: &mut HashMap<(i64, i64, i64, i64, u8), u32>,
    ) -> Laid {
        let units = self.model.units;
        let q = |v: f64| (v / (units.tolerance() * 100.0)).round() as i64;
        let key = |a: Point, b: Point, level: u8| {
            let (a, b) = if (q(a.x), q(a.y)) <= (q(b.x), q(b.y)) { (a, b) } else { (b, a) };
            (q(a.x), q(a.y), q(b.x), q(b.y), level)
        };
        // Its lane: beside every cable already along any stretch of it.
        let stretches: Vec<_> = steps
            .iter()
            .filter(|(_, e)| matches!(e.along, Along::Wall | Along::Header | Along::Ceiling))
            .map(|(n, e)| key(route.nodes[*n].at, route.nodes[e.to].at, e.along as u8))
            .collect();
        let lane = stretches.iter().map(|k| lanes.get(k).copied().unwrap_or(0)).max().unwrap_or(0);
        for k in stretches {
            *lanes.entry(k).or_default() += 1;
        }
        let offset = {
            let n = lane.div_ceil(2) as f64;
            let sign = if lane % 2 == 1 { 1.0 } else { -1.0 };
            sign * n * LANE_MM / units.to_mm()
        };
        let mut laid = Laid { pieces: Vec::new(), marks: Vec::new(), length: 0.0 };
        let mut last_ceiling: Option<usize> = None;
        for (n, e) in steps {
            laid.length += e.length;
            let (a, b) = (route.nodes[*n].at, route.nodes[e.to].at);
            if e.along == Along::Riser {
                laid.marks.push(RunMark { at: a, kind: "riser", along: None });
                continue;
            }
            // From a part on a wall up to the high band (or down from it): it climbs the wall.
            if e.along == Along::Drop {
                let (from, to) = (route.nodes[*n], route.nodes[e.to]);
                let wall = if to.level == 1 {
                    Some(b)
                } else if from.level == 1 {
                    Some(a)
                } else {
                    None
                };
                if let Some(w) = wall {
                    laid.marks.push(RunMark { at: w, kind: "riser", along: None });
                }
            }
            if e.along == Along::Ceiling {
                // Over a wall, from one room's ceiling to another's.
                if let (Some(prev), Some(room)) = (last_ceiling, e.room) {
                    if prev != room {
                        let node = route.nodes[*n];
                        if let Some(w) = node.wall.and_then(|w| elements[w].wall.as_ref()) {
                            let [c, d] = w.line;
                            let l = c.distance(d).max(1e-12);
                            let kind = if w.cross == crate::assembly::Cross::Firestop.name() {
                                "firestop"
                            } else {
                                "cross"
                            };
                            laid.marks.push(RunMark {
                                at: a,
                                kind,
                                along: Some(Point::new((d.x - c.x) / l, (d.y - c.y) / l)),
                            });
                        }
                    }
                }
                last_ceiling = e.room;
            } else if e.along != Along::Drop {
                last_ceiling = None;
            }
            let mut pts = vec![a];
            pts.extend(e.bend);
            pts.push(b);
            match laid.pieces.last_mut() {
                Some((along, p)) if *along == e.along && p.last().is_some_and(|l| l.distance(a) < 1e-9) => {
                    p.extend(pts.into_iter().skip(1));
                }
                _ => laid.pieces.push((e.along, pts)),
            }
        }
        if offset != 0.0 {
            for (along, p) in &mut laid.pieces {
                if *along != Along::Drop {
                    *p = shift(p, offset);
                }
            }
        }
        laid
    }

    /// One element per piece of a laid cable.
    fn emit(&self, laid: Laid, info: RunInfo, scope: &str, out: &mut Out) {
        let cable = out.cables;
        out.cables += 1;
        let n = laid.pieces.len();
        let mut marks = Some(laid.marks);
        for (i, (along, pts)) in laid.pieces.into_iter().enumerate() {
            let mut info = info.clone();
            info.along = Some(along.name());
            info.cable = Some(cable);
            info.length = Some(laid.length);
            // The circuit's number goes where its home run starts.
            info.home = info.home && i == 0;
            if i + 1 == n {
                info.marks = marks.take().unwrap_or_default();
            }
            self.push(pts, info, scope, out);
        }
    }

    fn push(&self, points: Vec<Point>, info: RunInfo, scope: &str, out: &mut Out) {
        let mut e = Element::new(
            format!("run:{}:{}:{scope}#{}", self.inst.service, self.ni, out.runs.len()),
            ElementKind::Run,
            points,
            false,
        );
        e.source = Some(self.pointer.to_owned());
        e.run = Some(info);
        out.runs.push(e);
    }
}

/// Turn a way from the source into a way to it.
fn reverse(route: &Route, steps: &mut Vec<(usize, Edge)>, end: usize) {
    let mut nodes: Vec<usize> = steps.iter().map(|s| s.0).collect();
    nodes.push(end);
    let edges: Vec<Edge> = steps.iter().map(|s| s.1).collect();
    let mut out = Vec::with_capacity(edges.len());
    for k in (0..edges.len()).rev() {
        let mut e = edges[k];
        e.to = nodes[k];
        out.push((nodes[k + 1], e));
    }
    let _ = route;
    *steps = out;
}

/// A polyline moved sideways by `d` (to its left), corners mitred.
fn shift(p: &[Point], d: f64) -> Vec<Point> {
    let normal = |a: Point, b: Point| {
        let l = a.distance(b);
        if l <= 1e-12 {
            None
        } else {
            Some(Point::new(-(b.y - a.y) / l, (b.x - a.x) / l))
        }
    };
    let mut out = Vec::with_capacity(p.len());
    for i in 0..p.len() {
        let before = if i > 0 { normal(p[i - 1], p[i]) } else { None };
        let after = if i + 1 < p.len() { normal(p[i], p[i + 1]) } else { None };
        let n = match (before, after) {
            (Some(a), Some(b)) => {
                let m = Point::new(a.x + b.x, a.y + b.y);
                let l = (m.x * m.x + m.y * m.y).sqrt();
                if l < 1e-6 {
                    a
                } else {
                    let m = Point::new(m.x / l, m.y / l);
                    let c = (m.x * a.x + m.y * a.y).max(0.5);
                    Point::new(m.x / c, m.y / c)
                }
            }
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => Point::new(0.0, 0.0),
        };
        out.push(Point::new(p[i].x + n.x * d, p[i].y + n.y * d));
    }
    out
}

/// An arc from `a` to `b`, bowed to the left, as a polyline.
fn arc(a: Point, b: Point) -> Vec<Point> {
    let d = a.distance(b);
    if d <= 1e-9 {
        return vec![a, b];
    }
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let n = Point::new(-(b.y - a.y) / d, (b.x - a.x) / d);
    let c = Point::new(mid.x + n.x * d * 0.2, mid.y + n.y * d * 0.2);
    (0..=12)
        .map(|k| {
            let t = k as f64 / 12.0;
            let (u, v, w) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            Point::new(u * a.x + v * c.x + w * b.x, u * a.y + v * c.y + w * b.y)
        })
        .collect()
}

/// Networks on a pinned flat (`testkit::pinned_flat`): 6 outlets in the living room, 4 in the
/// bedroom, one panel in the hall.
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{joint, Face, Fixture, Point, Room};

    use serde_json::json;

    use crate::solve::testkit::*;

    /// A 4 × 3 room whose bottom wall a network runs in (a cavity of 0.14) and whose other walls
    /// it can't (0.09), and a fixture `x0..x1 × y0..y1` with this back.
    fn room_and(body: [f64; 4], back: (Point, Point)) -> (Room, Fixture, Vec<Point>) {
        let corners =
            [Point::new(0.0, 0.0), Point::new(4.0, 0.0), Point::new(4.0, 3.0), Point::new(0.0, 3.0)];
        let inward =
            [Point::new(0.0, 1.0), Point::new(-1.0, 0.0), Point::new(0.0, -1.0), Point::new(1.0, 0.0)];
        let faces = (0..4)
            .map(|k| Face {
                a: corners[k],
                b: corners[(k + 1) % 4],
                wall: None,
                walled: true,
                roles: Vec::new(),
                cavity: if k == 0 { 0.14 } else { 0.09 },
                inward: inward[k],
                at: (0, k as f64),
            })
            .collect();
        let room = Room {
            element: 0,
            path: "room".into(),
            name: "room".into(),
            key: "room".into(),
            kind: crate::plan::SpaceKind::Room,
            tags: Vec::new(),
            loops: vec![corners.to_vec()],
            faces,
            openings: Vec::new(),
            fixtures: Vec::new(),
            area: 12.0,
        };
        let [x0, y0, x1, y1] = body;
        let outline = vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
        (room, Fixture { element: 0, back, against: None }, outline)
    }

    #[test]
    fn a_fixture_joins_where_it_meets_a_wall_the_network_runs_in() {
        let runs = |f: &Face| f.cavity >= 0.12;
        // Backed on the bottom wall: its back's middle.
        let (room, f, outline) = room_and([1.0, 0.0, 3.0, 0.6], (Point::new(3.0, 0.0), Point::new(1.0, 0.0)));
        let (at, face) = joint(&room, &f, &outline, 1e-3, &runs);
        assert_eq!((at, face), (Point::new(2.0, 0.0), Some(0)));
        // Backed on the left wall, its end on the bottom one: there, as near its back's middle as
        // it gets.
        let (room, f, outline) = room_and([0.0, 0.0, 0.6, 2.0], (Point::new(0.0, 2.0), Point::new(0.0, 0.0)));
        let (at, face) = joint(&room, &f, &outline, 1e-3, &runs);
        assert_eq!(face, Some(0));
        assert!(at.distance(Point::new(0.0, 0.0)) < 1e-9, "{at:?}");
        // Nowhere on a wall it runs in: its back's middle still (the network says it can't run).
        let (room, f, outline) = room_and([0.0, 1.0, 0.6, 2.5], (Point::new(0.0, 2.5), Point::new(0.0, 1.0)));
        let (at, face) = joint(&room, &f, &outline, 1e-3, &runs);
        assert_eq!((at, face), (Point::new(0.0, 1.75), Some(3)));
    }

    #[test]
    fn groups_hold_at_most_max_and_every_terminal_is_on_one() {
        let s = run(pinned_flat_with(json!({ "power": power() })));
        assert_met(&s);
        let mut groups: BTreeMap<String, usize> = BTreeMap::new();
        for o in parts(&s, "outlet") {
            let g =
                o.part.as_ref().unwrap().group.clone().unwrap_or_else(|| panic!("{} is on no circuit", o.id));
            *groups.entry(g).or_default() += 1;
        }
        // Ten outlets, at most six a circuit, rooms kept together.
        assert_eq!(groups.values().copied().collect::<Vec<_>>(), [6, 4], "{groups:?}");
        let circuits: Vec<_> = s.layout.circuits.iter().filter(|c| c.service == "power").collect();
        assert_eq!(circuits.len(), 2);
        assert_eq!(circuits.iter().map(|c| c.devices.iter().map(|d| d.1).sum::<u32>()).sum::<u32>(), 10);
    }

    #[test]
    fn each_circuit_has_one_home_run_in_each_drawing() {
        let s = run(pinned_flat_with(json!({ "power": power() })));
        for draw in ["routed", "arcs"] {
            let mut homes: BTreeMap<String, usize> = BTreeMap::new();
            for r in runs(&s, draw) {
                let info = r.run.as_ref().unwrap();
                if info.network == "General" && info.home {
                    *homes.entry(info.group.clone().unwrap()).or_default() += 1;
                }
            }
            assert_eq!(homes.values().copied().collect::<Vec<_>>(), [1, 1], "{draw}: {homes:?}");
        }
        // The drawing the network didn't ask for is marked, and exports leave it out.
        assert!(runs(&s, "arcs").iter().all(|r| r.run.as_ref().unwrap().alt && !r.exported()));
        assert!(runs(&s, "routed").iter().all(|r| !r.run.as_ref().unwrap().alt && r.exported()));
    }

    #[test]
    fn a_circuits_length_is_the_sum_of_its_cables() {
        let s = run(pinned_flat_with(json!({ "power": power() })));
        for c in s.layout.circuits.iter().filter(|c| c.service == "power") {
            let mut cables: BTreeMap<u32, f64> = BTreeMap::new();
            for r in runs(&s, "routed") {
                let info = r.run.as_ref().unwrap();
                if info.group.as_deref() == Some(&c.group) {
                    cables.insert(info.cable.unwrap(), info.length.unwrap());
                }
            }
            let sum: f64 = cables.values().sum();
            assert!((sum - c.length).abs() < 1e-6, "circuit {}: {sum} vs {}", c.group, c.length);
        }
    }
}
