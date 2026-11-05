//! Cad+: 2D-топооптимизация методом SIMP (FR-09).
//!
//! Алгоритм повторяет классический top88 (Andreassen, Clausen, Schevenels, Lazarov, Sigmund, 2011):
//! билинейные квадратные элементы плоского напряжённого состояния, штраф `ρ^p`, фильтр плотности,
//! обновление методом оптимальности (OC) с бисекцией множителя. Дополнительно:
//! пассивные элементы (сплошные и пустые — запретные зоны, FR-03), толщина и модуль упругости
//! в физических единицах, прямой ленточный решатель вместо разреженного.
//!
//! Нумерация как в top88: элементы и узлы — по столбцам, строка `j = 0` — верх сетки.
//! Узел `(i, j)` имеет номер `i·(nely + 1) + j`, его степени свободы — `2·номер` (x) и `2·номер + 1` (y).

pub mod banded;

use banded::Banded;

/// Роль элемента.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// Оптимизируемый.
    Active,
    /// Всегда сплошной (кромка нервюры, кольцо вокруг отверстия).
    Solid,
    /// Всегда пустой (вне контура, отверстие).
    Void,
}

/// Расчётная задача на прямоугольной сетке.
#[derive(Clone, Debug)]
pub struct Problem {
    /// Число элементов по x.
    pub nelx: usize,
    /// Число элементов по y.
    pub nely: usize,
    /// Роль каждого элемента, индекс `i·nely + j`.
    pub cells: Vec<Cell>,
    /// Узловые силы: (степень свободы, сила, Н).
    pub loads: Vec<(usize, f64)>,
    /// Закреплённые степени свободы.
    pub fixed: Vec<usize>,
    /// Модуль упругости сплошного материала, МПа.
    pub e0: f64,
    /// Коэффициент Пуассона.
    pub nu: f64,
    /// Толщина пластины, мм (для квадратного элемента размер стороны не влияет на жёсткость).
    pub thickness: f64,
}

impl Problem {
    /// Номер узла.
    pub fn node(&self, i: usize, j: usize) -> usize {
        i * (self.nely + 1) + j
    }

    /// Число степеней свободы.
    pub fn ndof(&self) -> usize {
        2 * (self.nelx + 1) * (self.nely + 1)
    }

    /// Номер элемента.
    pub fn elem(&self, i: usize, j: usize) -> usize {
        i * self.nely + j
    }

    /// Классическая MBB-балка из top88 (половина с симметрией): сила вниз в левом верхнем углу.
    pub fn mbb(nelx: usize, nely: usize) -> Self {
        let ndof = 2 * (nelx + 1) * (nely + 1);
        let mut fixed: Vec<usize> = (0..=nely).map(|j| 2 * j).collect();
        fixed.push(ndof - 1);
        Self {
            nelx,
            nely,
            cells: vec![Cell::Active; nelx * nely],
            loads: vec![(1, -1.0)],
            fixed,
            e0: 1.0,
            nu: 0.3,
            thickness: 1.0,
        }
    }
}

/// Параметры оптимизации.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Целевая доля объёма среди оптимизируемых элементов.
    pub volfrac: f64,
    /// Показатель штрафа p.
    pub penal: f64,
    /// Радиус фильтра в элементах.
    pub rmin: f64,
    /// Предел изменения плотности за итерацию.
    pub move_limit: f64,
    /// Остановка, когда наибольшее изменение плотности меньше этого значения.
    pub tol_change: f64,
    /// Предельное число итераций.
    pub max_iter: usize,
    /// Относительная жёсткость пустого элемента.
    pub emin_ratio: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volfrac: 0.5,
            penal: 3.0,
            rmin: 1.5,
            move_limit: 0.2,
            tol_change: 0.01,
            max_iter: 300,
            emin_ratio: 1e-9,
        }
    }
}

/// Сведения об одной итерации.
#[derive(Clone, Copy, Debug)]
pub struct Iteration {
    /// Номер с единицы.
    pub iter: usize,
    /// Податливость, Н·мм.
    pub compliance: f64,
    /// Доля объёма среди оптимизируемых элементов.
    pub volume: f64,
    /// Наибольшее изменение плотности.
    pub change: f64,
}

/// Что делать после итерации.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Продолжать.
    Continue,
    /// Остановить (отмена пользователем).
    Stop,
}

/// Результат.
#[derive(Clone, Debug)]
pub struct Solution {
    /// Физические плотности после фильтра, индекс `i·nely + j`.
    pub density: Vec<f64>,
    /// Податливость на последней итерации, Н·мм.
    pub compliance: f64,
    /// Число итераций.
    pub iterations: usize,
    /// Сошёлся ли расчёт (иначе — лимит итераций или отмена).
    pub converged: bool,
    /// Отменён пользователем.
    pub cancelled: bool,
    /// История итераций.
    pub history: Vec<Iteration>,
}

/// Ошибка постановки или расчёта.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Неверные входные данные.
    Invalid(String),
    /// Матрица жёсткости вырождена: конструкция не закреплена.
    Singular,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Invalid(s) => write!(f, "{s}"),
            Error::Singular => write!(f, "Деталь не закреплена: матрица жёсткости вырождена"),
        }
    }
}

/// Матрица жёсткости квадратного элемента для E = 1 (как в top88).
pub fn element_stiffness(nu: f64) -> [[f64; 8]; 8] {
    let a11 = [
        [12., 3., -6., -3.],
        [3., 12., 3., 0.],
        [-6., 3., 12., -3.],
        [-3., 0., -3., 12.],
    ];
    let a12 = [
        [-6., -3., 0., 3.],
        [-3., -6., -3., -6.],
        [0., -3., -6., 3.],
        [3., -6., 3., -6.],
    ];
    let b11 = [
        [-4., 3., -2., 9.],
        [3., -4., -9., 4.],
        [-2., -9., -4., -3.],
        [9., 4., -3., -4.],
    ];
    let b12 = [
        [2., -3., 4., -9.],
        [-3., 2., 9., -2.],
        [4., 9., 2., 3.],
        [-9., -2., 3., 2.],
    ];
    let mut ke = [[0.0; 8]; 8];
    let k = 1.0 / (1.0 - nu * nu) / 24.0;
    for r in 0..4 {
        for c in 0..4 {
            ke[r][c] = k * (a11[r][c] + nu * b11[r][c]);
            ke[r][c + 4] = k * (a12[r][c] + nu * b12[r][c]);
            ke[r + 4][c] = k * (a12[c][r] + nu * b12[c][r]);
            ke[r + 4][c + 4] = k * (a11[r][c] + nu * b11[r][c]);
        }
    }
    ke
}

/// Степени свободы элемента в порядке top88.
fn edofs(nely: usize, i: usize, j: usize) -> [usize; 8] {
    let n1 = i * (nely + 1) + j; // левый верхний
    let n2 = (i + 1) * (nely + 1) + j; // правый верхний
    let lb = 2 * (n1 + 1); // левый нижний, x
    let rb = 2 * (n2 + 1); // правый нижний, x
    [
        lb,
        lb + 1,
        rb,
        rb + 1,
        2 * n2,
        2 * n2 + 1,
        2 * n1,
        2 * n1 + 1,
    ]
}

/// Разреженная матрица фильтра: для каждого элемента — соседи и веса.
struct Filter {
    start: Vec<usize>,
    idx: Vec<u32>,
    w: Vec<f64>,
    hs: Vec<f64>,
}

impl Filter {
    fn new(nelx: usize, nely: usize, rmin: f64) -> Self {
        let r = rmin.ceil() as isize - 1;
        let mut start = Vec::with_capacity(nelx * nely + 1);
        let (mut idx, mut w) = (Vec::new(), Vec::new());
        let mut hs = vec![0.0; nelx * nely];
        start.push(0);
        for i1 in 0..nelx as isize {
            for j1 in 0..nely as isize {
                let e1 = (i1 as usize) * nely + j1 as usize;
                for i2 in (i1 - r).max(0)..=(i1 + r).min(nelx as isize - 1) {
                    for j2 in (j1 - r).max(0)..=(j1 + r).min(nely as isize - 1) {
                        let d = (((i1 - i2).pow(2) + (j1 - j2).pow(2)) as f64).sqrt();
                        let v = (rmin - d).max(0.0);
                        if v > 0.0 {
                            idx.push(((i2 as usize) * nely + j2 as usize) as u32);
                            w.push(v);
                            hs[e1] += v;
                        }
                    }
                }
                start.push(idx.len());
            }
        }
        Self { start, idx, w, hs }
    }

    /// y = H·x (H симметрична).
    fn apply(&self, x: &[f64], y: &mut [f64]) {
        for (e, ye) in y.iter_mut().enumerate() {
            let mut s = 0.0;
            for k in self.start[e]..self.start[e + 1] {
                s += self.w[k] * x[self.idx[k] as usize];
            }
            *ye = s;
        }
    }
}

/// Отображение степеней свободы на неизвестные системы: закреплённые и висящие
/// (узлы только пустых элементов) исключаются, порядок сохраняется — лента не растёт.
fn dof_map(p: &Problem) -> (Vec<Option<usize>>, usize, usize) {
    let ndof = p.ndof();
    let mut used = vec![false; ndof];
    for i in 0..p.nelx {
        for j in 0..p.nely {
            if p.cells[p.elem(i, j)] != Cell::Void {
                for d in edofs(p.nely, i, j) {
                    used[d] = true;
                }
            }
        }
    }
    for &d in &p.fixed {
        used[d] = false;
    }
    let mut map = vec![None; ndof];
    let mut n = 0;
    for d in 0..ndof {
        if used[d] {
            map[d] = Some(n);
            n += 1;
        }
    }
    // Полуширина ленты по реально связанным неизвестным.
    let mut b = 0;
    for i in 0..p.nelx {
        for j in 0..p.nely {
            let ids: Vec<usize> = edofs(p.nely, i, j).iter().filter_map(|&d| map[d]).collect();
            if let (Some(lo), Some(hi)) = (ids.iter().min(), ids.iter().max()) {
                b = b.max(hi - lo);
            }
        }
    }
    (map, n, b)
}

fn validate(p: &Problem, s: &Settings) -> Result<(), Error> {
    let bad = |m: &str| Err(Error::Invalid(m.into()));
    if p.nelx == 0 || p.nely == 0 || p.cells.len() != p.nelx * p.nely {
        return bad("Размер сетки не совпадает с числом элементов");
    }
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if !positive(p.e0) || !(0.0..0.5).contains(&p.nu) || !positive(p.thickness) {
        return bad("Свойства материала или толщина заданы неверно");
    }
    if !(0.05..=0.95).contains(&s.volfrac) {
        return bad("Доля объёма вне диапазона 5–95%");
    }
    if !(1.0..=5.0).contains(&s.penal) || !(1.0..=50.0).contains(&s.rmin) || s.max_iter == 0 {
        return bad("Параметры решателя вне допустимых диапазонов");
    }
    if p.cells.iter().all(|&c| c != Cell::Active) {
        return bad("Нет оптимизируемых элементов");
    }
    if p.loads.is_empty() {
        return bad("Не задано ни одной нагрузки");
    }
    if p.fixed.is_empty() {
        return Err(Error::Singular);
    }
    let ndof = p.ndof();
    if p.loads.iter().any(|&(d, _)| d >= ndof) || p.fixed.iter().any(|&d| d >= ndof) {
        return bad("Нагрузка или закрепление вне сетки");
    }
    Ok(())
}

/// Решить задачу. `on_iter` вызывается после каждой итерации с её сведениями и текущими
/// плотностями; если он вернёт `Control::Stop`, расчёт прекращается (FR-10).
pub fn solve<F>(p: &Problem, s: &Settings, mut on_iter: F) -> Result<Solution, Error>
where
    F: FnMut(&Iteration, &[f64]) -> Control,
{
    validate(p, s)?;
    let ne = p.nelx * p.nely;
    let ke = element_stiffness(p.nu);
    let filter = Filter::new(p.nelx, p.nely, s.rmin);
    let (map, n, b) = dof_map(p);
    let mut k = Banded::zeros(n, b);
    let mut f = vec![0.0; n];
    for &(d, v) in &p.loads {
        if let Some(r) = map[d] {
            f[r] += v;
        }
    }
    let active: Vec<bool> = p.cells.iter().map(|&c| c == Cell::Active).collect();
    let n_active = active.iter().filter(|&&a| a).count() as f64;
    let fixed_value = |c: Cell| if c == Cell::Solid { 1.0 } else { 0.0 };

    let mut x: Vec<f64> = p
        .cells
        .iter()
        .map(|&c| {
            if c == Cell::Active {
                s.volfrac
            } else {
                fixed_value(c)
            }
        })
        .collect();
    let mut xphys = x.clone();
    let emin = s.emin_ratio * p.e0;
    let scale = p.thickness;
    let mut history = Vec::new();
    let (mut ce, mut dc, mut dv) = (vec![0.0; ne], vec![0.0; ne], vec![0.0; ne]);
    let mut tmp = vec![0.0; ne];
    let mut xnew = vec![0.0; ne];
    let mut u_full = vec![0.0; p.ndof()];
    let (mut change, mut compliance, mut iter) = (1.0_f64, 0.0, 0);
    let mut cancelled = false;

    while change > s.tol_change && iter < s.max_iter {
        iter += 1;
        // Сборка и решение K·U = F.
        k.clear();
        for i in 0..p.nelx {
            for j in 0..p.nely {
                let e = i * p.nely + j;
                if p.cells[e] == Cell::Void {
                    continue;
                }
                let ee = scale * (emin + xphys[e].powf(s.penal) * (p.e0 - emin));
                let ed = edofs(p.nely, i, j);
                for a in 0..8 {
                    let Some(ra) = map[ed[a]] else { continue };
                    for c in 0..8 {
                        let Some(rc) = map[ed[c]] else { continue };
                        if rc <= ra {
                            k.add_lower(ra, rc, ee * ke[a][c]);
                        }
                    }
                }
            }
        }
        k.cholesky().map_err(|_| Error::Singular)?;
        let mut u = f.clone();
        k.solve_in_place(&mut u);
        u_full.fill(0.0);
        for (d, m) in map.iter().enumerate() {
            if let Some(r) = m {
                u_full[d] = u[*r];
            }
        }
        // Податливость и чувствительности.
        compliance = 0.0;
        for i in 0..p.nelx {
            for j in 0..p.nely {
                let e = i * p.nely + j;
                let ed = edofs(p.nely, i, j);
                let ue: [f64; 8] = std::array::from_fn(|a| u_full[ed[a]]);
                let mut v = 0.0;
                for a in 0..8 {
                    let mut row = 0.0;
                    for c in 0..8 {
                        row += ke[a][c] * ue[c];
                    }
                    v += ue[a] * row;
                }
                ce[e] = scale * v;
                if p.cells[e] != Cell::Void {
                    compliance += (emin + xphys[e].powf(s.penal) * (p.e0 - emin)) * ce[e];
                }
                dc[e] = if active[e] {
                    -s.penal * (p.e0 - emin) * xphys[e].powf(s.penal - 1.0) * ce[e]
                } else {
                    0.0
                };
                dv[e] = if active[e] { 1.0 } else { 0.0 };
            }
        }
        // Фильтр плотности для чувствительностей: dc = H·(dc / Hs), dv = H·(dv / Hs).
        for e in 0..ne {
            tmp[e] = dc[e] / filter.hs[e];
        }
        filter.apply(&tmp, &mut dc);
        for e in 0..ne {
            tmp[e] = dv[e] / filter.hs[e];
        }
        filter.apply(&tmp, &mut dv);
        // OC с бисекцией множителя Лагранжа.
        let (mut l1, mut l2) = (0.0_f64, 1e9_f64);
        while (l2 - l1) / (l1 + l2) > 1e-3 {
            let lmid = 0.5 * (l2 + l1);
            for e in 0..ne {
                xnew[e] = if active[e] {
                    let be = (-dc[e] / dv[e] / lmid).max(0.0).sqrt();
                    (x[e] * be)
                        .min(x[e] + s.move_limit)
                        .min(1.0)
                        .max(x[e] - s.move_limit)
                        .max(0.0)
                } else {
                    x[e]
                };
            }
            filter.apply(&xnew, &mut xphys);
            for e in 0..ne {
                xphys[e] /= filter.hs[e];
                if !active[e] {
                    xphys[e] = fixed_value(p.cells[e]);
                }
            }
            let vol: f64 = (0..ne).filter(|&e| active[e]).map(|e| xphys[e]).sum();
            if vol > s.volfrac * n_active {
                l1 = lmid;
            } else {
                l2 = lmid;
            }
        }
        change = (0..ne).map(|e| (xnew[e] - x[e]).abs()).fold(0.0, f64::max);
        std::mem::swap(&mut x, &mut xnew);
        let volume = (0..ne)
            .filter(|&e| active[e])
            .map(|e| xphys[e])
            .sum::<f64>()
            / n_active;
        let it = Iteration {
            iter,
            compliance,
            volume,
            change,
        };
        history.push(it);
        if on_iter(&it, &xphys) == Control::Stop {
            cancelled = true;
            break;
        }
    }
    Ok(Solution {
        density: xphys,
        compliance,
        iterations: iter,
        converged: !cancelled && change <= s.tol_change,
        cancelled,
        history,
    })
}
