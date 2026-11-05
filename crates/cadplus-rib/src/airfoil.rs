//! Профиль крыла: чтение файлов .dat и встроенный Clark Y.
//!
//! Файл читает ядро yadro: форматы Selig и Lednicer определяются сами, комментарии,
//! CRLF, запятые и повторы точек не мешают, ошибки называются с номером строки.
//! Файл в долях хорды берётся как есть, файл в мм или повёрнутый приводится к хорде 1.
//!
//! Clark Y взят из базы координат профилей UIUC (UIUC Airfoil Coordinates Database,
//! файл clarky.dat, 121 точка). Максимальная толщина 11,7% на 28% хорды.

use yadro_core::{Airfoil as Foil, Vec2};

/// Встроенные координаты Clark Y (UIUC).
pub const CLARK_Y_DAT: &str = include_str!("../../../assets/airfoils/clarky.dat");

/// Профиль, приведённый к хорде 1: верх и низ от носка к задней кромке.
#[derive(Clone, Debug, PartialEq)]
pub struct Airfoil {
    /// Название из первой строки файла.
    pub name: String,
    /// Верхняя поверхность, x по возрастанию.
    pub upper: Vec<(f64, f64)>,
    /// Нижняя поверхность, x по возрастанию.
    pub lower: Vec<(f64, f64)>,
    /// Профиль ядра: по нему считаются толщина, средняя линия и контур.
    foil: Foil,
}

impl Airfoil {
    /// Clark Y из базы UIUC.
    pub fn clark_y() -> Self {
        Self::parse_dat(CLARK_Y_DAT).expect("встроенный файл корректен")
    }

    /// Разобрать файл .dat (Selig или Lednicer).
    pub fn parse_dat(text: &str) -> Result<Self, String> {
        let foil = Foil::parse_dat(text).map_err(|e| format!("Профиль: {e}"))?;
        Self::from_kernel(foil)
    }

    /// Из профиля ядра (например, NACA 4-й серии).
    pub fn from_kernel(foil: Foil) -> Result<Self, String> {
        // Для нагрузки и интерполяции — поверхности с x по возрастанию.
        let surface = |s: &[Vec2]| {
            let mut v: Vec<(f64, f64)> = s.iter().map(|p| (p.x, p.y)).collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            v.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-12);
            v
        };
        let af = Self {
            name: foil.name().to_string(),
            upper: surface(foil.upper()),
            lower: surface(foil.lower()),
            foil,
        };
        if af.thickness_at(0.3) <= 0.0 {
            return Err("Верх и низ профиля перепутаны или пересекаются".into());
        }
        Ok(af)
    }

    /// Профиль ядра.
    pub fn kernel(&self) -> &Foil {
        &self.foil
    }

    /// y верхней поверхности в точке x (доли хорды).
    pub fn upper_at(&self, x: f64) -> f64 {
        self.foil.upper_y(x)
    }

    /// y нижней поверхности.
    pub fn lower_at(&self, x: f64) -> f64 {
        self.foil.lower_y(x)
    }

    /// Относительная толщина.
    pub fn thickness_at(&self, x: f64) -> f64 {
        self.foil.thickness(x)
    }

    /// Средняя линия.
    pub fn camber_at(&self, x: f64) -> f64 {
        self.foil.camber(x)
    }

    /// Наибольшая толщина и где она: (толщина, x).
    pub fn max_thickness(&self) -> (f64, f64) {
        let (x, t) = self.foil.max_thickness();
        (t, x)
    }

    /// Таблица толщины (x, t) для проверки лонжеронов в расчёте крыла.
    pub fn thickness_table(&self) -> Vec<(f64, f64)> {
        (0..=100)
            .map(|k| k as f64 / 100.0)
            .map(|x| (x, self.thickness_at(x)))
            .collect()
    }

    /// Замкнутый контур нервюры с хордой `chord`, мм: от задней кромки по верху к носку
    /// и по низу обратно (без повтора первой точки).
    pub fn outline(&self, chord: f64) -> Vec<(f64, f64)> {
        let xf = Foil::placement(chord, 0.0, 0.0, Vec2::ZERO);
        let mut pts: Vec<(f64, f64)> = self
            .foil
            .outline(&xf)
            .into_iter()
            .map(|p| (p.x, p.y))
            .collect();
        // Если задняя кромка замкнута, последняя точка совпадает с первой — убираем повтор.
        if let (Some(a), Some(b)) = (pts.first(), pts.last())
            && (a.0 - b.0).abs() < 1e-9
            && (a.1 - b.1).abs() < 1e-9
        {
            pts.pop();
        }
        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clark_y_matches_reference() {
        let af = Airfoil::clark_y();
        let (t, x) = af.max_thickness();
        // Справочно: 11,7% на 28% хорды.
        assert!((t - 0.117).abs() < 0.001, "толщина {t}");
        assert!((x - 0.28).abs() < 0.02, "положение {x}");
        // У корня (хорда 220 мм) толщина около 25,7 мм — как в сводке.
        assert!((t * 220.0 - 25.7).abs() < 0.2);
    }

    #[test]
    fn lednicer_and_selig_give_same_shape() {
        let selig = "TEST\n1 0\n0.5 0.06\n0 0\n0.5 -0.02\n1 0\n";
        // Lednicer: верх и низ от носка.
        let led = "TEST\n3 3\n\n0 0\n0.5 0.06\n1 0\n\n0 0\n0.5 -0.02\n1 0\n";
        let a = Airfoil::parse_dat(selig).unwrap();
        let b = Airfoil::parse_dat(led).unwrap();
        assert!((a.thickness_at(0.5) - 0.08).abs() < 1e-12);
        assert!((b.thickness_at(0.5) - 0.08).abs() < 1e-12);
    }

    #[test]
    fn errors_name_the_line() {
        let e = Airfoil::parse_dat("X\n1 0\n0.5 0.1\nоops\n0 0\n").unwrap_err();
        assert!(e.contains("строка 4"), "{e}");
    }

    #[test]
    fn naca_from_kernel() {
        let af = Airfoil::from_kernel(Foil::naca4_code("2412", 81, true).unwrap()).unwrap();
        let (t, _) = af.max_thickness();
        assert!((t - 0.12).abs() < 0.002, "толщина {t}");
        assert!(af.camber_at(0.4) > 0.015);
    }

    #[test]
    fn outline_is_closed_polygon() {
        let af = Airfoil::clark_y();
        let o = af.outline(200.0);
        assert!(o.len() > 100);
        let area: f64 = o
            .iter()
            .zip(o.iter().cycle().skip(1))
            .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
            .sum::<f64>()
            .abs()
            / 2.0;
        // Площадь Clark Y ≈ 0,08 c².
        assert!(
            (area / 200.0_f64.powi(2) - 0.08).abs() < 0.005,
            "площадь {area}"
        );
    }
}
