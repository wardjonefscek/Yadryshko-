//! Сечения лонжеронов. Изгиб — в вертикальной плоскости (подъёмная сила),
//! высота сечения `h` направлена по толщине профиля, ширина `b` — по хорде.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Поперечное сечение лонжерона, все размеры в мм.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Section {
    /// Круглая трубка. Стенка ≥ D/2 означает сплошной стержень.
    Tube {
        /// Наружный диаметр.
        d_out: f64,
        /// Толщина стенки.
        wall: f64,
    },
    /// Сплошная прямоугольная рейка.
    Rect {
        /// Ширина по хорде.
        b: f64,
        /// Высота по толщине профиля.
        h: f64,
    },
    /// Прямоугольная труба (коробка).
    RectTube {
        /// Ширина по хорде.
        b: f64,
        /// Высота по толщине профиля.
        h: f64,
        /// Толщина стенки.
        wall: f64,
    },
}

impl Section {
    /// Проверка размеров.
    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        match *self {
            Section::Tube { d_out, wall } => {
                if !ok(d_out) || !ok(wall) {
                    return Err("Трубка: диаметр и стенка должны быть больше нуля".into());
                }
            }
            Section::Rect { b, h } => {
                if !ok(b) || !ok(h) {
                    return Err("Рейка: ширина и высота должны быть больше нуля".into());
                }
            }
            Section::RectTube { b, h, wall } => {
                if !ok(b) || !ok(h) || !ok(wall) {
                    return Err("Коробка: размеры должны быть больше нуля".into());
                }
                if 2.0 * wall >= b.min(h) {
                    return Err("Коробка: стенка слишком толстая — это уже сплошная рейка".into());
                }
            }
        }
        Ok(())
    }

    /// Площадь сечения, мм².
    pub fn area(&self) -> f64 {
        match *self {
            Section::Tube { d_out, wall } => {
                let d_in = (d_out - 2.0 * wall).max(0.0);
                PI / 4.0 * (d_out * d_out - d_in * d_in)
            }
            Section::Rect { b, h } => b * h,
            Section::RectTube { b, h, wall } => b * h - (b - 2.0 * wall) * (h - 2.0 * wall),
        }
    }

    /// Момент инерции относительно горизонтальной оси, мм⁴.
    pub fn inertia(&self) -> f64 {
        match *self {
            Section::Tube { d_out, wall } => {
                let d_in = (d_out - 2.0 * wall).max(0.0);
                PI / 64.0 * (d_out.powi(4) - d_in.powi(4))
            }
            Section::Rect { b, h } => b * h.powi(3) / 12.0,
            Section::RectTube { b, h, wall } => {
                let bi = b - 2.0 * wall;
                let hi = h - 2.0 * wall;
                (b * h.powi(3) - bi * hi.powi(3)) / 12.0
            }
        }
    }

    /// Высота сечения по толщине профиля, мм.
    pub fn height(&self) -> f64 {
        match *self {
            Section::Tube { d_out, .. } => d_out,
            Section::Rect { h, .. } | Section::RectTube { h, .. } => h,
        }
    }

    /// Ширина сечения по хорде, мм.
    pub fn width(&self) -> f64 {
        match *self {
            Section::Tube { d_out, .. } => d_out,
            Section::Rect { b, .. } | Section::RectTube { b, .. } => b,
        }
    }

    /// Расстояние от нейтральной оси до крайнего волокна, мм.
    pub fn c_max(&self) -> f64 {
        self.height() / 2.0
    }

    /// Круглое ли сечение (тогда в нервюре круглое отверстие, иначе — прямоугольный вырез).
    pub fn is_round(&self) -> bool {
        matches!(self, Section::Tube { .. })
    }

    /// Короткая подпись: «трубка Ø10×1».
    pub fn label(&self) -> String {
        match *self {
            Section::Tube { d_out, wall } if 2.0 * wall >= d_out => {
                format!("стержень Ø{}", fmt(d_out))
            }
            Section::Tube { d_out, wall } => format!("трубка Ø{}×{}", fmt(d_out), fmt(wall)),
            Section::Rect { b, h } => format!("рейка {}×{}", fmt(b), fmt(h)),
            Section::RectTube { b, h, wall } => {
                format!("коробка {}×{}×{}", fmt(b), fmt(h), fmt(wall))
            }
        }
    }
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tube_10x1() {
        let s = Section::Tube {
            d_out: 10.0,
            wall: 1.0,
        };
        // π/64 · (10⁴ − 8⁴) = 289.81 мм⁴; π/4 · (100 − 64) = 28.27 мм².
        assert!((s.inertia() - 289.8119).abs() < 1e-3);
        assert!((s.area() - 28.2743).abs() < 1e-3);
        assert_eq!(s.label(), "трубка Ø10×1");
    }

    #[test]
    fn rect_and_box() {
        let r = Section::Rect { b: 5.0, h: 10.0 };
        assert!((r.inertia() - 5.0 * 1000.0 / 12.0).abs() < 1e-9);
        let bx = Section::RectTube {
            b: 10.0,
            h: 10.0,
            wall: 1.0,
        };
        assert!((bx.inertia() - (10_000.0 - 4096.0) / 12.0).abs() < 1e-9);
        assert!(
            Section::RectTube {
                b: 4.0,
                h: 10.0,
                wall: 2.0
            }
            .validate()
            .is_err()
        );
    }
}
