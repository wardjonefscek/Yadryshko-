//! Геометрия нервюры: контур профиля, отверстия под лонжероны, проверка стенок.

use crate::airfoil::Airfoil;
use cadplus_aero::analysis::RibLoad;
use cadplus_aero::{Section, Spar};
use serde::{Deserialize, Serialize};

/// Точка на плоскости нервюры, мм: x — от носка по хорде, y — вверх.
pub type Pt = (f64, f64);

/// Форма отверстия.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HoleShape {
    /// Круглое, диаметр с учётом посадки.
    Circle {
        /// Диаметр, мм.
        d: f64,
    },
    /// Прямоугольный вырез под рейку.
    Rect {
        /// Ширина по хорде, мм.
        w: f64,
        /// Высота, мм.
        h: f64,
    },
}

/// Отверстие в нервюре.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    /// Подпись: чей это вырез.
    pub label: String,
    /// Центр, мм.
    pub center: Pt,
    /// Форма.
    pub shape: HoleShape,
}

impl Hole {
    /// Точка внутри отверстия.
    pub fn contains(&self, p: Pt) -> bool {
        let (dx, dy) = (p.0 - self.center.0, p.1 - self.center.1);
        match self.shape {
            HoleShape::Circle { d } => dx * dx + dy * dy < (d / 2.0).powi(2),
            HoleShape::Rect { w, h } => dx.abs() < w / 2.0 && dy.abs() < h / 2.0,
        }
    }

    /// Расстояние от точки снаружи отверстия до его края.
    pub fn distance(&self, p: Pt) -> f64 {
        let (dx, dy) = (p.0 - self.center.0, p.1 - self.center.1);
        match self.shape {
            HoleShape::Circle { d } => ((dx * dx + dy * dy).sqrt() - d / 2.0).max(0.0),
            HoleShape::Rect { w, h } => {
                let ex = (dx.abs() - w / 2.0).max(0.0);
                let ey = (dy.abs() - h / 2.0).max(0.0);
                (ex * ex + ey * ey).sqrt()
            }
        }
    }

    /// Площадь, мм².
    pub fn area(&self) -> f64 {
        match self.shape {
            HoleShape::Circle { d } => std::f64::consts::PI * d * d / 4.0,
            HoleShape::Rect { w, h } => w * h,
        }
    }

    /// Половина высоты отверстия на расстоянии `dx` от центра по хорде (None — вне отверстия).
    fn half_height_at(&self, dx: f64) -> Option<f64> {
        match self.shape {
            HoleShape::Circle { d } => {
                let r = d / 2.0;
                (dx.abs() <= r).then(|| (r * r - dx * dx).max(0.0).sqrt())
            }
            HoleShape::Rect { w, h } => (dx.abs() <= w / 2.0).then_some(h / 2.0),
        }
    }

    /// Крайние x отверстия.
    fn x_range(&self) -> (f64, f64) {
        let half = match self.shape {
            HoleShape::Circle { d } => d / 2.0,
            HoleShape::Rect { w, .. } => w / 2.0,
        };
        (self.center.0 - half, self.center.0 + half)
    }
}

/// Нервюра одной станции.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RibGeometry {
    /// Номер от корня, с нуля.
    pub index: usize,
    /// Станция, мм от корневой нервюры.
    pub z: f64,
    /// Хорда, мм.
    pub chord: f64,
    /// Замкнутый контур, мм.
    pub outline: Vec<Pt>,
    /// Верхняя поверхность от носка, мм (сюда прикладывается нагрузка).
    pub upper: Vec<Pt>,
    /// Нижняя поверхность от носка, мм.
    pub lower: Vec<Pt>,
    /// Отверстия.
    pub holes: Vec<Hole>,
    /// Расчётная сила на нервюру, Н.
    pub force_n: f64,
    /// Сколько таких нервюр в самолёте (по одной в каждой консоли).
    pub quantity: usize,
}

impl RibGeometry {
    /// y верха в точке x, мм.
    pub fn upper_at(&self, x: f64) -> f64 {
        lerp(&self.upper, x)
    }

    /// y низа в точке x, мм.
    pub fn lower_at(&self, x: f64) -> f64 {
        lerp(&self.lower, x)
    }

    /// Площадь контура без отверстий, мм².
    pub fn outline_area(&self) -> f64 {
        polygon_area(&self.outline)
    }

    /// Короткая подпись латиницей для DXF: «R1 z0 x2».
    pub fn ascii_label(&self) -> String {
        format!("R{} z{:.0} x{}", self.index + 1, self.z, self.quantity)
    }
}

/// Построить нервюру станции по профилю и лонжеронам.
///
/// `clearance` прибавляется к размеру отверстия: посадка на ЧПУ или допуск расширения при печати.
/// Возвращает геометрию и предупреждения (тонкие стенки, пересечения, прямые углы под фрезу).
pub fn build(
    af: &Airfoil,
    rib: &RibLoad,
    spars: &[Spar],
    clearance: f64,
    min_wall: f64,
    cnc: bool,
) -> (RibGeometry, Vec<String>) {
    let c = rib.chord;
    let scale = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| (x * c, y * c)).collect::<Vec<_>>();
    let mut warnings = Vec::new();
    let name = format!("Нервюра {} (z = {:.0} мм)", rib.index + 1, rib.z);
    let mut holes = Vec::new();
    for sp in spars.iter().filter(|s| s.covers(rib.z)) {
        let cx = sp.chord_pos * c;
        let cy = af.camber_at(sp.chord_pos) * c;
        let shape = match sp.section {
            Section::Tube { d_out, .. } => HoleShape::Circle {
                d: d_out + clearance,
            },
            Section::Rect { b, h } | Section::RectTube { b, h, .. } => {
                if cnc {
                    warnings.push(format!(
                        "{name}: прямоугольный вырез под «{}» — фреза оставит скругления в углах; нужны поднутрения («dog-bone») или скруглённая рейка.",
                        sp.name
                    ));
                }
                HoleShape::Rect {
                    w: b + clearance,
                    h: h + clearance,
                }
            }
        };
        holes.push(Hole {
            label: sp.name.clone(),
            center: (cx, cy),
            shape,
        });
    }
    let geom = RibGeometry {
        index: rib.index,
        z: rib.z,
        chord: c,
        outline: af.outline(c),
        upper: scale(&af.upper),
        lower: scale(&af.lower),
        holes,
        force_n: rib.force_n,
        quantity: 2,
    };
    for h in &geom.holes {
        let wall = wall_thickness(&geom, h);
        if wall < 0.0 {
            warnings.push(format!(
                "{name}: отверстие «{}» не помещается в профиль.",
                h.label
            ));
        } else if wall < min_wall {
            warnings.push(format!(
                "{name}: стенка над или под «{}» {:.1} мм — меньше минимальных {:.1} мм.",
                h.label, wall, min_wall
            ));
        }
    }
    for (a, h1) in geom.holes.iter().enumerate() {
        for h2 in geom.holes.iter().skip(a + 1) {
            let (l1, r1) = h1.x_range();
            let (l2, r2) = h2.x_range();
            if l1.max(l2) < r1.min(r2) + min_wall {
                warnings.push(format!(
                    "{name}: отверстия «{}» и «{}» слишком близко или пересекаются.",
                    h1.label, h2.label
                ));
            }
        }
    }
    if geom.holes.is_empty() {
        warnings.push(format!(
            "{name}: через станцию не проходит ни один лонжерон — нервюру не к чему закрепить."
        ));
    }
    (geom, warnings)
}

/// Наименьшая стенка между отверстием и контуром по вертикали, мм.
pub fn wall_thickness(g: &RibGeometry, h: &Hole) -> f64 {
    let (x0, x1) = h.x_range();
    (0..=40)
        .map(|k| x0 + (x1 - x0) * k as f64 / 40.0)
        .filter_map(|x| {
            h.half_height_at(x - h.center.0).map(|hh| {
                let top = g.upper_at(x) - (h.center.1 + hh);
                let bottom = (h.center.1 - hh) - g.lower_at(x);
                top.min(bottom)
            })
        })
        .fold(f64::INFINITY, f64::min)
}

/// Площадь многоугольника (по модулю), мм².
pub fn polygon_area(p: &[Pt]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Точка внутри многоугольника (метод лучей).
pub fn point_in_polygon(p: Pt, poly: &[Pt]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Расстояние от точки до замкнутой ломаной.
pub fn distance_to_polygon(p: Pt, poly: &[Pt]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| seg_dist(p, poly[i], poly[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let l2 = vx * vx + vy * vy;
    let t = if l2 > 0.0 {
        (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (qx, qy) = (a.0 + t * vx, a.1 + t * vy);
    ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt()
}

fn lerp(pts: &[Pt], x: f64) -> f64 {
    if x <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        if x <= w[1].0 {
            let t = (x - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + t * (w[1].1 - w[0].1);
        }
    }
    pts[pts.len() - 1].1
}
