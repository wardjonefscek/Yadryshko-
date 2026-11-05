//! Расчётная задача нервюры: сетка, роли элементов, закрепления и нагрузка.
//!
//! - вне контура и в отверстиях — пустые элементы;
//! - полоса вдоль контура (кромка под обтяжку) и кольца вокруг отверстий — сплошные (FR-03);
//! - закрепление — узлы на кромках отверстий под лонжероны (нервюра опирается на лонжерон);
//! - нагрузка — вертикальная сила на верхний контур с распределением по хорде
//!   из тонкой теории профиля: Δp ∝ √((1 − x̄) / x̄).

use crate::geom::{RibGeometry, distance_to_polygon, point_in_polygon};
use cadplus_topopt::{Cell, Problem};

/// Прямоугольная сетка элементов над нервюрой.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Элементов по x.
    pub nelx: usize,
    /// Элементов по y.
    pub nely: usize,
    /// Размер элемента, мм.
    pub h: f64,
    /// x левого края сетки, мм.
    pub x0: f64,
    /// y верхнего края сетки, мм.
    pub ytop: f64,
}

impl Grid {
    /// Центр элемента `(i, j)`; `j = 0` — верхний ряд.
    pub fn center(&self, i: usize, j: usize) -> (f64, f64) {
        (
            self.x0 + (i as f64 + 0.5) * self.h,
            self.ytop - (j as f64 + 0.5) * self.h,
        )
    }

    /// Положение узла.
    pub fn node_pos(&self, i: usize, j: usize) -> (f64, f64) {
        (self.x0 + i as f64 * self.h, self.ytop - j as f64 * self.h)
    }
}

/// Готовая задача и сведения для обратного перевода в геометрию.
#[derive(Clone, Debug)]
pub struct RibSetup {
    /// Геометрия нервюры.
    pub geometry: RibGeometry,
    /// Сетка.
    pub grid: Grid,
    /// Задача топооптимизации.
    pub problem: Problem,
}

/// Параметры построения сетки.
#[derive(Clone, Copy, Debug)]
pub struct GridParams {
    /// Размер элемента, мм.
    pub element_mm: f64,
    /// Ширина сплошной полосы вдоль контура, мм.
    pub edge_band_mm: f64,
    /// Ширина сплошного кольца вокруг отверстий, мм.
    pub ring_mm: f64,
    /// Модуль упругости материала нервюры, МПа.
    pub e_mpa: f64,
    /// Коэффициент Пуассона.
    pub nu: f64,
    /// Толщина нервюры, мм.
    pub thickness_mm: f64,
}

/// Построить задачу.
pub fn build(g: &RibGeometry, p: &GridParams) -> Result<RibSetup, String> {
    let h = p.element_mm;
    if !(0.1..=5.0).contains(&h) {
        return Err("Размер элемента сетки должен быть от 0,1 до 5 мм".into());
    }
    if g.holes.is_empty() {
        return Err(format!(
            "Нервюра {}: через станцию не проходит лонжерон — закрепить нервюру не на чем.",
            g.index + 1
        ));
    }
    let (ymin, ymax) = g
        .outline
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q.1), b.max(q.1)));
    let grid = Grid {
        nelx: ((g.chord + 2.0 * h) / h).ceil() as usize,
        nely: ((ymax - ymin + 2.0 * h) / h).ceil() as usize,
        h,
        x0: -h,
        ytop: ymax + h,
    };
    let (nelx, nely) = (grid.nelx, grid.nely);
    let mut cells = vec![Cell::Void; nelx * nely];
    let mut hole_cell = vec![false; nelx * nely];
    for i in 0..nelx {
        for j in 0..nely {
            let c = grid.center(i, j);
            let e = i * nely + j;
            if !point_in_polygon(c, &g.outline) {
                continue;
            }
            if g.holes.iter().any(|hl| hl.contains(c)) {
                hole_cell[e] = true;
                continue;
            }
            let near_edge = distance_to_polygon(c, &g.outline) <= p.edge_band_mm;
            let near_hole = g.holes.iter().any(|hl| hl.distance(c) <= p.ring_mm);
            cells[e] = if near_edge || near_hole {
                Cell::Solid
            } else {
                Cell::Active
            };
        }
    }
    if !cells.contains(&Cell::Active) {
        return Err(format!(
            "Нервюра {}: после кромки и колец не осталось области для оптимизации — уменьшите полосу или кольца.",
            g.index + 1
        ));
    }

    // Узлы, принадлежащие материалу и отверстиям.
    let nn = (nelx + 1) * (nely + 1);
    let mut in_material = vec![false; nn];
    let mut on_hole = vec![false; nn];
    for i in 0..nelx {
        for j in 0..nely {
            let e = i * nely + j;
            for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let n = (i + di) * (nely + 1) + (j + dj);
                if cells[e] != Cell::Void {
                    in_material[n] = true;
                }
                if hole_cell[e] {
                    on_hole[n] = true;
                }
            }
        }
    }
    let mut fixed = Vec::new();
    for n in 0..nn {
        if in_material[n] && on_hole[n] {
            fixed.push(2 * n);
            fixed.push(2 * n + 1);
        }
    }
    if fixed.is_empty() {
        return Err(format!(
            "Нервюра {}: не удалось найти кромку отверстия для закрепления.",
            g.index + 1
        ));
    }

    // Нагрузка на верхний контур.
    let samples = 400;
    let mut weights = Vec::with_capacity(samples);
    for k in 0..samples {
        let xr = 0.005 + 0.99 * (k as f64 + 0.5) / samples as f64;
        weights.push((xr, ((1.0 - xr) / xr).sqrt()));
    }
    let wsum: f64 = weights.iter().map(|w| w.1).sum();
    let mut loads: Vec<(usize, f64)> = Vec::new();
    for (xr, w) in weights {
        let x = xr * g.chord;
        let y = g.upper_at(x);
        let node = nearest_material_node(&grid, &in_material, (x, y)).ok_or_else(|| {
            format!(
                "Нервюра {}: не удалось приложить нагрузку к контуру.",
                g.index + 1
            )
        })?;
        let f = g.force_n * w / wsum;
        match loads.iter_mut().find(|l| l.0 == 2 * node + 1) {
            Some(l) => l.1 += f,
            None => loads.push((2 * node + 1, f)),
        }
    }

    let problem = Problem {
        nelx,
        nely,
        cells,
        loads,
        fixed,
        e0: p.e_mpa,
        nu: p.nu,
        thickness: p.thickness_mm,
    };
    Ok(RibSetup {
        geometry: g.clone(),
        grid,
        problem,
    })
}

fn nearest_material_node(grid: &Grid, in_material: &[bool], p: (f64, f64)) -> Option<usize> {
    let i0 = ((p.0 - grid.x0) / grid.h).round() as isize;
    let j0 = ((grid.ytop - p.1) / grid.h).round() as isize;
    let mut best: Option<(f64, usize)> = None;
    for di in -3..=3 {
        for dj in -3..=3 {
            let (i, j) = (i0 + di, j0 + dj);
            if i < 0 || j < 0 || i > grid.nelx as isize || j > grid.nely as isize {
                continue;
            }
            let n = i as usize * (grid.nely + 1) + j as usize;
            if !in_material[n] {
                continue;
            }
            let q = grid.node_pos(i as usize, j as usize);
            let d = (q.0 - p.0).powi(2) + (q.1 - p.1).powi(2);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, n));
            }
        }
    }
    best.map(|b| b.1)
}
