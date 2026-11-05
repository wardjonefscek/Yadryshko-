//! Контуры облегчающих вырезов по полю на сетке: морфологическое «открытие» под фрезу,
//! марширующие квадраты, сглаживание Чайкина и упрощение Дугласа — Пекера.
//!
//! Временная реализация до ступени 1 ядра yadro, где запланирован точный «контур из поля плотности».

use crate::geom::Pt;
use crate::setup::Grid;
use std::collections::HashMap;

/// Морфологическое открытие маски радиусом `r_px` (в элементах): остаются только места,
/// куда целиком помещается круг этого радиуса. Для ЧПУ это множество точек, достижимых фрезой.
pub fn open(mask: &[bool], nelx: usize, nely: usize, r_px: f64) -> Vec<bool> {
    let r = r_px.floor() as isize;
    let offsets: Vec<(isize, isize)> = (-r..=r)
        .flat_map(|di| (-r..=r).map(move |dj| (di, dj)))
        .filter(|&(di, dj)| ((di * di + dj * dj) as f64) <= r_px * r_px)
        .collect();
    let at = |m: &[bool], i: isize, j: isize| -> bool {
        i >= 0
            && j >= 0
            && (i as usize) < nelx
            && (j as usize) < nely
            && m[i as usize * nely + j as usize]
    };
    let mut eroded = vec![false; mask.len()];
    for i in 0..nelx as isize {
        for j in 0..nely as isize {
            eroded[i as usize * nely + j as usize] =
                offsets.iter().all(|&(di, dj)| at(mask, i + di, j + dj));
        }
    }
    let mut opened = vec![false; mask.len()];
    for i in 0..nelx as isize {
        for j in 0..nely as isize {
            if eroded[i as usize * nely + j as usize] {
                for &(di, dj) in &offsets {
                    let (a, b) = (i + di, j + dj);
                    if a >= 0 && b >= 0 && (a as usize) < nelx && (b as usize) < nely {
                        opened[a as usize * nely + b as usize] = true;
                    }
                }
            }
        }
    }
    opened
}

/// Замкнутые контуры области `mask` (элементы), мм.
pub fn trace(mask: &[bool], grid: &Grid) -> Vec<Vec<Pt>> {
    let (nelx, nely) = (grid.nelx, grid.nely);
    // Значение в узле — доля соседних элементов маски.
    let val = |i: usize, j: usize| -> f64 {
        let mut s = 0.0;
        for (di, dj) in [(0isize, 0isize), (-1, 0), (0, -1), (-1, -1)] {
            let (a, b) = (i as isize + di, j as isize + dj);
            if a >= 0
                && b >= 0
                && (a as usize) < nelx
                && (b as usize) < nely
                && mask[a as usize * nely + b as usize]
            {
                s += 1.0;
            }
        }
        s / 4.0
    };
    let nx = nelx + 1;
    let ny = nely + 1;
    let v: Vec<f64> = (0..nx)
        .flat_map(|i| (0..ny).map(move |j| (i, j)))
        .map(|(i, j)| val(i, j))
        .collect();
    let vat = |i: usize, j: usize| v[i * ny + j];
    // Ребро между двумя узлами — ключ точки пересечения.
    type Key = (usize, usize);
    let node = |i: usize, j: usize| i * ny + j;
    let key = |a: usize, b: usize| -> Key { if a < b { (a, b) } else { (b, a) } };
    let mut points: HashMap<Key, Pt> = HashMap::new();
    let mut adj: HashMap<Key, Vec<Key>> = HashMap::new();
    let lvl = 0.5;
    for i in 0..nx - 1 {
        for j in 0..ny - 1 {
            // Углы клетки: 0 — (i, j), 1 — (i+1, j), 2 — (i+1, j+1), 3 — (i, j+1).
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let vals: Vec<f64> = corners.iter().map(|&(a, b)| vat(a, b)).collect();
            let code = vals
                .iter()
                .enumerate()
                .fold(0u8, |c, (k, &x)| c | (((x > lvl) as u8) << k));
            if code == 0 || code == 15 {
                continue;
            }
            let edge = |k: usize| -> Key {
                let (a, b) = (corners[k], corners[(k + 1) % 4]);
                key(node(a.0, a.1), node(b.0, b.1))
            };
            let mut cross = |k: usize| -> Key {
                let kk = edge(k);
                points.entry(kk).or_insert_with(|| {
                    let (a, b) = (corners[k], corners[(k + 1) % 4]);
                    let (va, vb) = (vals[k], vals[(k + 1) % 4]);
                    let t = if (vb - va).abs() > 1e-12 {
                        (lvl - va) / (vb - va)
                    } else {
                        0.5
                    };
                    let pa = grid.node_pos(a.0, a.1);
                    let pb = grid.node_pos(b.0, b.1);
                    (pa.0 + t * (pb.0 - pa.0), pa.1 + t * (pb.1 - pa.1))
                });
                kk
            };
            // Пары рёбер, через которые проходит линия уровня.
            let center = vals.iter().sum::<f64>() / 4.0 > lvl;
            let pairs: Vec<(usize, usize)> = match code {
                1 | 14 => vec![(3, 0)],
                2 | 13 => vec![(0, 1)],
                3 | 12 => vec![(3, 1)],
                4 | 11 => vec![(1, 2)],
                6 | 9 => vec![(0, 2)],
                7 | 8 => vec![(3, 2)],
                5 => {
                    if center {
                        vec![(3, 2), (0, 1)]
                    } else {
                        vec![(3, 0), (1, 2)]
                    }
                }
                10 => {
                    if center {
                        vec![(3, 0), (1, 2)]
                    } else {
                        vec![(0, 1), (3, 2)]
                    }
                }
                _ => vec![],
            };
            for (a, b) in pairs {
                let (ka, kb) = (cross(a), cross(b));
                adj.entry(ka).or_default().push(kb);
                adj.entry(kb).or_default().push(ka);
            }
        }
    }
    // Сшиваем отрезки в замкнутые контуры.
    let mut used: HashMap<Key, bool> = HashMap::new();
    let mut loops = Vec::new();
    let mut keys: Vec<Key> = adj.keys().copied().collect();
    keys.sort();
    for start in keys {
        if used.get(&start).copied().unwrap_or(false) {
            continue;
        }
        let mut ring = vec![start];
        used.insert(start, true);
        let mut prev = start;
        let mut cur = adj[&start][0];
        while cur != start {
            used.insert(cur, true);
            ring.push(cur);
            let next = adj[&cur]
                .iter()
                .copied()
                .find(|&n| n != prev)
                .unwrap_or(start);
            prev = cur;
            cur = next;
            if ring.len() > points.len() + 1 {
                break;
            }
        }
        if ring.len() >= 3 {
            loops.push(ring.iter().map(|k| points[k]).collect());
        }
    }
    loops
}

/// Сглаживание замкнутого контура (Чайкин): срезает углы ступенек сетки.
pub fn chaikin(p: &[Pt], rounds: usize) -> Vec<Pt> {
    let mut cur = p.to_vec();
    for _ in 0..rounds {
        let n = cur.len();
        let mut next = Vec::with_capacity(2 * n);
        for i in 0..n {
            let (a, b) = (cur[i], cur[(i + 1) % n]);
            next.push((0.75 * a.0 + 0.25 * b.0, 0.75 * a.1 + 0.25 * b.1));
            next.push((0.25 * a.0 + 0.75 * b.0, 0.25 * a.1 + 0.75 * b.1));
        }
        cur = next;
    }
    cur
}

/// Упрощение замкнутого контура с допуском `tol`, мм.
pub fn simplify(p: &[Pt], tol: f64) -> Vec<Pt> {
    if p.len() < 8 {
        return p.to_vec();
    }
    // Делим кольцо в двух далёких точках и упрощаем половины.
    let far = (1..p.len())
        .max_by(|&a, &b| dist2(p[0], p[a]).total_cmp(&dist2(p[0], p[b])))
        .unwrap_or(p.len() / 2);
    let mut a: Vec<Pt> = p[..=far].to_vec();
    let mut b: Vec<Pt> = p[far..].to_vec();
    b.push(p[0]);
    a = rdp(&a, tol);
    b = rdp(&b, tol);
    a.pop();
    b.pop();
    a.extend(b);
    a
}

fn rdp(p: &[Pt], tol: f64) -> Vec<Pt> {
    if p.len() < 3 {
        return p.to_vec();
    }
    let (s, e) = (p[0], p[p.len() - 1]);
    let (mut idx, mut dmax) = (0, 0.0);
    for (i, &q) in p.iter().enumerate().take(p.len() - 1).skip(1) {
        let d = line_dist(q, s, e);
        if d > dmax {
            dmax = d;
            idx = i;
        }
    }
    if dmax > tol {
        let mut left = rdp(&p[..=idx], tol);
        let right = rdp(&p[idx..], tol);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![s, e]
    }
}

fn dist2(a: Pt, b: Pt) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

fn line_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let l = (vx * vx + vy * vy).sqrt();
    if l < 1e-12 {
        return dist2(p, a).sqrt();
    }
    ((p.0 - a.0) * vy - (p.1 - a.1) * vx).abs() / l
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::polygon_area;

    fn grid(n: usize) -> Grid {
        Grid {
            nelx: n,
            nely: n,
            h: 1.0,
            x0: 0.0,
            ytop: n as f64,
        }
    }

    #[test]
    fn square_traces_to_one_loop_of_right_area() {
        let n = 20;
        let mut m = vec![false; n * n];
        for i in 5..15 {
            for j in 5..15 {
                m[i * n + j] = true;
            }
        }
        let loops = trace(&m, &grid(n));
        assert_eq!(loops.len(), 1);
        let a = polygon_area(&loops[0]);
        // Квадрат 10×10 со срезанными углами (контур по середине узловой доли): около 100.
        assert!((a - 100.0).abs() < 3.0, "площадь {a}");
    }

    #[test]
    fn opening_removes_narrow_slots() {
        // Щель шириной 2 элемента не проходит фрезу радиусом 1,5 элемента, широкий карман остаётся.
        let n = 30;
        let mut m = vec![false; n * n];
        for i in 2..28 {
            for j in 5..7 {
                m[i * n + j] = true;
            }
            for j in 12..22 {
                m[i * n + j] = true;
            }
        }
        let o = open(&m, n, n, 1.5);
        assert!(
            (2..28).all(|i| !o[i * n + 5] && !o[i * n + 6]),
            "узкая щель должна исчезнуть"
        );
        assert!(o[15 * n + 17], "широкий карман должен остаться");
    }

    #[test]
    fn simplify_keeps_square_corners() {
        let mut sq = Vec::new();
        for k in 0..10 {
            sq.push((k as f64, 0.0));
        }
        for k in 0..10 {
            sq.push((10.0, k as f64));
        }
        for k in 0..10 {
            sq.push((10.0 - k as f64, 10.0));
        }
        for k in 0..10 {
            sq.push((0.0, 10.0 - k as f64));
        }
        let s = simplify(&sq, 0.01);
        assert!(s.len() <= 6, "осталось {} точек", s.len());
        assert!((polygon_area(&s) - 100.0).abs() < 1e-9);
    }
}
