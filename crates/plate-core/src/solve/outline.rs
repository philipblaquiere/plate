//! Outlines of sets of grid cells: the shape of a fill space, or of floor no space covers.
//!
//! Cells are given by a predicate on `(i, j)`; the result is one [`Region`] per 4-connected
//! group of cells, with its outer boundary (counter-clockwise) and its holes (clockwise), as
//! corners in cell coordinates with collinear points removed.

/// A connected group of cells: its outline, holes and how many cells it has.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Vec<(i32, i32)>,
    pub holes: Vec<Vec<(i32, i32)>>,
    pub cells: i32,
}

/// Label 4-connected groups of cells in an `nx × ny` grid: `labels[j * nx + i]` is the group of
/// a cell (or `usize::MAX`), and the second value is each group's size.
pub fn components(nx: i32, ny: i32, filled: impl Fn(i32, i32) -> bool) -> (Vec<usize>, Vec<i32>) {
    let mut labels = vec![usize::MAX; (nx.max(0) * ny.max(0)) as usize];
    let mut sizes = Vec::new();
    let at = |i: i32, j: i32| (j * nx + i) as usize;
    let mut stack = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            if labels[at(i, j)] != usize::MAX || !filled(i, j) {
                continue;
            }
            let label = sizes.len();
            let mut size = 0;
            labels[at(i, j)] = label;
            stack.push((i, j));
            while let Some((x, y)) = stack.pop() {
                size += 1;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (u, v) = (x + dx, y + dy);
                    if u >= 0 && v >= 0 && u < nx && v < ny && labels[at(u, v)] == usize::MAX && filled(u, v)
                    {
                        labels[at(u, v)] = label;
                        stack.push((u, v));
                    }
                }
            }
            sizes.push(size);
        }
    }
    (labels, sizes)
}

/// The outline of every 4-connected group of filled cells.
pub fn regions(nx: i32, ny: i32, filled: impl Fn(i32, i32) -> bool) -> Vec<Region> {
    let (labels, sizes) = components(nx, ny, &filled);
    (0..sizes.len())
        .map(|label| {
            let inside = |i: i32, j: i32| {
                i >= 0 && j >= 0 && i < nx && j < ny && labels[(j * nx + i) as usize] == label
            };
            let mut loops = trace(nx, ny, inside);
            // The loop with the largest area (counter-clockwise) is the outside; the rest are holes.
            loops.sort_by_key(|l| -area2(l));
            let mut it = loops.into_iter();
            let outer = it.next().unwrap_or_default();
            Region { outer, holes: it.collect(), cells: sizes[label] }
        })
        .collect()
}

/// Twice the signed area of a loop.
fn area2(points: &[(i32, i32)]) -> i64 {
    let n = points.len();
    (0..n)
        .map(|k| {
            let (a, b) = (points[k], points[(k + 1) % n]);
            a.0 as i64 * b.1 as i64 - b.0 as i64 * a.1 as i64
        })
        .sum()
}

/// Closed loops along the edges between filled and empty cells, with the filled side on the left
/// (so outlines run counter-clockwise and holes clockwise).
fn trace(nx: i32, ny: i32, filled: impl Fn(i32, i32) -> bool) -> Vec<Vec<(i32, i32)>> {
    use std::collections::HashMap;
    // Directed edges, by start vertex.
    let mut out: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    let mut count = 0usize;
    for j in 0..ny {
        for i in 0..nx {
            if !filled(i, j) {
                continue;
            }
            let mut edge = |a: (i32, i32), b: (i32, i32)| {
                out.entry(a).or_default().push(b);
                count += 1;
            };
            if !filled(i, j - 1) {
                edge((i, j), (i + 1, j));
            }
            if !filled(i + 1, j) {
                edge((i + 1, j), (i + 1, j + 1));
            }
            if !filled(i, j + 1) {
                edge((i + 1, j + 1), (i, j + 1));
            }
            if !filled(i - 1, j) {
                edge((i, j + 1), (i, j));
            }
        }
    }
    let mut loops = Vec::new();
    let mut starts: Vec<(i32, i32)> = out.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        while out.get(&start).is_some_and(|v| !v.is_empty()) {
            let mut path = vec![start];
            let mut prev = start;
            let mut cur = out.get_mut(&start).unwrap().pop().unwrap();
            let mut guard = 0;
            while cur != start && guard <= count {
                guard += 1;
                path.push(cur);
                let dir = (cur.0 - prev.0, cur.1 - prev.1);
                let next = {
                    let options = out.get_mut(&cur).expect("boundary edges form loops");
                    // Where two groups touch at a corner, turn left: that keeps to one group.
                    let rank = |to: &(i32, i32)| {
                        let d = (to.0 - cur.0, to.1 - cur.1);
                        let cross = dir.0 * d.1 - dir.1 * d.0;
                        let dot = dir.0 * d.0 + dir.1 * d.1;
                        if cross > 0 {
                            0
                        } else if dot > 0 {
                            1
                        } else {
                            2
                        }
                    };
                    let k = (0..options.len()).min_by_key(|&k| rank(&options[k])).unwrap();
                    options.swap_remove(k)
                };
                prev = cur;
                cur = next;
            }
            loops.push(simplify(path));
        }
    }
    loops
}

/// Drop corners that lie on a straight line between their neighbours.
fn simplify(path: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    let n = path.len();
    if n < 4 {
        return path;
    }
    (0..n)
        .filter(|&k| {
            let (a, b, c) = (path[(k + n - 1) % n], path[k], path[(k + 1) % n]);
            (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0) != 0
        })
        .map(|k| path[k])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region_of(rows: &[&str]) -> Vec<Region> {
        // Rows are given top to bottom; j = 0 is the bottom row.
        let ny = rows.len() as i32;
        let nx = rows[0].len() as i32;
        let cell = |i: i32, j: i32| {
            i >= 0 && j >= 0 && i < nx && j < ny && rows[(ny - 1 - j) as usize].as_bytes()[i as usize] == b'#'
        };
        regions(nx, ny, cell)
    }

    #[test]
    fn a_rectangle_is_four_corners() {
        let r = region_of(&["###", "###"]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].outer.len(), 4);
        assert!(r[0].holes.is_empty());
        assert_eq!(r[0].cells, 6);
        assert_eq!(area2(&r[0].outer), 12);
    }

    #[test]
    fn an_l_and_a_ring() {
        let l = region_of(&["#..", "###"]);
        assert_eq!(l[0].outer.len(), 6);
        let ring = region_of(&["###", "#.#", "###"]);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring[0].outer.len(), 4);
        assert_eq!(ring[0].holes.len(), 1);
        assert!(area2(&ring[0].holes[0]) < 0, "holes run clockwise");
    }

    #[test]
    fn cells_touching_at_a_corner_are_separate() {
        let r = region_of(&[".#", "#."]);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|g| g.outer.len() == 4 && g.cells == 1));
    }
}
