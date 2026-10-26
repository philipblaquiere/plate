//! What every engine needs: space types resolved to numbers, instance names, rule targets and a
//! deterministic random number generator.

use crate::plan::{area_bounds, bounds, Bounds1, Plan, Selector, Space};
use crate::units::Units;

/// xorshift64*: small, fast and deterministic everywhere.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng(seed ^ 0x9E37_79B9_7F4A_7C15);
        for _ in 0..4 {
            r.next_u64();
        }
        r
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [0, n).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// Uniform in [-r, r].
    pub fn around(&mut self, r: i32) -> i32 {
        self.below((2 * r + 1) as usize) as i32 - r
    }
}

/// A stable 64-bit hash (FNV-1a), for seeds and typical keys.
pub fn hash64(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for b in part.bytes().chain([0xff]) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// A space type, with its size limits in the plan's units (including what its own plan needs).
#[derive(Debug, Clone)]
pub struct SpaceType {
    pub key: String,
    pub space: Space,
    pub area: Bounds1,
    pub side: Bounds1,
    pub width: Bounds1,
    pub depth: Bounds1,
    pub aspect: f64,
    pub min: u32,
    pub max: Option<u32>,
    /// Fixed corners (`at`), one per instance, in the plan's frame.
    pub pins: Vec<crate::geometry::Point>,
    /// A fixture placed as one run of this many, side by side (`row`): one rectangle to the
    /// engine, split when it's drawn.
    pub row: u32,
}

impl SpaceType {
    pub fn all(plan: &Plan, units: Units) -> Vec<SpaceType> {
        plan.spaces
            .iter()
            .map(|(key, space)| {
                let needs = space.plan.as_ref().map(|p| &p.needs);
                let width = bounds(space.width, units).intersect(bounds(needs.and_then(|n| n.width), units));
                let depth = bounds(space.depth, units).intersect(bounds(needs.and_then(|n| n.depth), units));
                let mut area = area_bounds(space.area).intersect(area_bounds(needs.and_then(|n| n.area)));
                // A space with a plan and no size of its own is sized by what's inside it.
                if let (Some(plan), None, None, None) = (&space.plan, space.area, space.width, space.depth) {
                    area = area.intersect(program_area(plan, units));
                }
                let pins = space.at.as_ref().map(|a| a.points(units)).unwrap_or_default();
                let (min, max) = if pins.is_empty() {
                    (space.count.min(), space.count.max())
                } else {
                    (pins.len() as u32, Some(pins.len() as u32))
                };
                // A row is one run: as wide as all of them together.
                let row = match (space.row, space.count) {
                    (true, crate::plan::Count::Exactly(n)) if n > 1 && space.is_fixture() => n,
                    _ => 1,
                };
                let (width, area, min, max) = if row > 1 {
                    let k = row as f64;
                    (
                        Bounds1 { min: width.min * k, max: width.max * k },
                        Bounds1 { min: area.min * k, max: area.max * k },
                        1,
                        Some(1),
                    )
                } else {
                    (width, area, min, max)
                };
                SpaceType {
                    key: key.clone(),
                    space: space.clone(),
                    area,
                    side: bounds(space.side, units),
                    width,
                    depth,
                    aspect: space.aspect.unwrap_or(f64::INFINITY),
                    min,
                    max,
                    pins,
                    row,
                }
            })
            .collect()
    }

    /// The area to aim for when nothing else says.
    pub fn target_area(&self, units: Units) -> f64 {
        let m2 = 1.0e6 / (units.to_mm() * units.to_mm());
        let from_dims = match (self.width.min > 0.0, self.depth.min > 0.0) {
            (true, true) => Some(self.width.target(0.0) * self.depth.target(0.0)),
            _ => None,
        };
        let default = from_dims.unwrap_or(match self.space.kind {
            crate::plan::SpaceKind::Circulation => 8.0 * m2,
            crate::plan::SpaceKind::Fixture => 1.0 * m2,
            _ => 12.0 * m2,
        });
        let side_min = self.side.min * self.side.min;
        self.area.target(default).max(side_min)
    }

    /// Instance name: `kitchen`, or `room_3` for numbered spaces (1-based). A row is named
    /// after its type: its pieces are numbered when it's split.
    pub fn instance_name(&self, i: usize) -> String {
        if self.row > 1 {
            self.key.clone()
        } else if self.space.count.numbered() {
            format!("{}_{}", self.key, i + 1)
        } else {
            self.key.clone()
        }
    }
}

/// The floor a plan's spaces take together, from their sizes and counts: the least and the most
/// (unbounded if any can grow without limit). Fixtures stand on floor: they don't add to it.
pub fn program_area(plan: &Plan, units: Units) -> Bounds1 {
    let (mut lo, mut hi) = (0.0, 0.0);
    for space in plan.spaces.values().filter(|s| !s.is_fixture()) {
        let a = area_bounds(space.area);
        let (w, d, side) =
            (bounds(space.width, units), bounds(space.depth, units), bounds(space.side, units));
        let one_min = if a.min > 0.0 {
            a.min
        } else if w.min > 0.0 && d.min > 0.0 {
            w.min * d.min
        } else {
            side.min * side.min
        };
        let one_max = if a.max.is_finite() {
            a.max
        } else if w.max.is_finite() && d.max.is_finite() {
            w.max * d.max
        } else {
            f64::INFINITY
        };
        let pins = space.at.as_ref().map_or(0, |a| a.len()) as u32;
        let (cmin, cmax) = if pins > 0 { (pins, Some(pins)) } else { (space.count.min(), space.count.max()) };
        lo += one_min * cmin as f64;
        hi += match cmax {
            Some(c) => one_max * c as f64,
            None => f64::INFINITY,
        };
    }
    // Leave the plan inside some room to move: aim no higher than three quarters of the way
    // from everything at its smallest to everything at its largest.
    let hi = if hi.is_finite() && hi > 0.0 { lo + 0.75 * (hi - lo) } else { f64::INFINITY };
    Bounds1 { min: lo, max: hi }
}

/// An instance placed by an engine: its type and name.
#[derive(Debug, Clone)]
pub struct Named {
    pub type_index: usize,
    pub name: String,
}

/// Indices of the instances a selector picks.
pub fn select(selector: &Selector, instances: &[Named], types: &[SpaceType]) -> Vec<usize> {
    instances
        .iter()
        .enumerate()
        .filter(|(_, inst)| selector.matches(&inst.name, &types[inst.type_index].key))
        .map(|(i, _)| i)
        .collect()
}

/// Does the selector name particular instances (`room_3`), as opposed to whole types?
pub fn names_instances(selector: &Selector, types: &[SpaceType]) -> bool {
    selector.items().iter().any(|item| {
        types.iter().any(|t| {
            t.space.count.numbered() && item.starts_with(&format!("{}_", t.key)) && !item.contains('*')
        })
    })
}

/// A number for messages: no trailing zeros, at most 2 decimals.
pub fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}
