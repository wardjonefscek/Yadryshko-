//! Профиль крыла: чтение файлов .dat (форматы Selig и Lednicer) и встроенный Clark Y.
//!
//! Clark Y взят из базы координат профилей UIUC (UIUC Airfoil Coordinates Database,
//! файл clarky.dat, 121 точка). Максимальная толщина 11,7% на 28% хорды.

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
}

impl Airfoil {
    /// Clark Y из базы UIUC.
    pub fn clark_y() -> Self {
        Self::parse_dat(CLARK_Y_DAT).expect("встроенный файл корректен")
    }

    /// Разобрать файл .dat. Selig: точки от задней кромки по верху к носку и по низу назад.
    /// Lednicer: вторая строка — число точек верха и низа, затем верх и низ от носка.
    pub fn parse_dat(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        let name = lines.next().ok_or("Пустой файл профиля")?.to_string();
        let rows: Vec<(f64, f64)> = lines
            .filter_map(|l| {
                let mut it = l
                    .split_whitespace()
                    .map(|v| v.replace(',', ".").parse::<f64>());
                match (it.next(), it.next()) {
                    (Some(Ok(x)), Some(Ok(y))) => Some((x, y)),
                    _ => None,
                }
            })
            .collect();
        if rows.len() < 5 {
            return Err("В файле профиля меньше 5 точек".into());
        }
        let (upper, lower) = if rows[0].0 > 1.5 && rows[0].1 > 1.5 {
            // Lednicer: первая строка — количества точек.
            let nu = rows[0].0.round() as usize;
            let nl = rows[0].1.round() as usize;
            if rows.len() < 1 + nu + nl {
                return Err("Формат Lednicer: точек меньше, чем указано в заголовке".into());
            }
            (rows[1..1 + nu].to_vec(), rows[1 + nu..1 + nu + nl].to_vec())
        } else {
            let le = rows
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.0.total_cmp(&b.1.0))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let mut up: Vec<(f64, f64)> = rows[..=le].to_vec();
            up.reverse();
            (up, rows[le..].to_vec())
        };
        let mut af = Self { name, upper, lower };
        af.normalize()?;
        Ok(af)
    }

    /// Привести хорду к 1 и упорядочить точки по x.
    fn normalize(&mut self) -> Result<(), String> {
        let all = self.upper.iter().chain(&self.lower);
        let (xmin, xmax) = all.fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let c = xmax - xmin;
        if !c.is_finite() || c <= 1e-9 {
            return Err("Профиль нулевой длины".into());
        }
        for p in self.upper.iter_mut().chain(self.lower.iter_mut()) {
            p.0 = (p.0 - xmin) / c;
            p.1 /= c;
        }
        for s in [&mut self.upper, &mut self.lower] {
            s.sort_by(|a, b| a.0.total_cmp(&b.0));
            s.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-12);
        }
        if self.thickness_at(0.3) <= 0.0 {
            return Err("Верх и низ профиля перепутаны или пересекаются".into());
        }
        Ok(())
    }

    /// y верхней поверхности в точке x (доли хорды).
    pub fn upper_at(&self, x: f64) -> f64 {
        lerp(&self.upper, x)
    }

    /// y нижней поверхности.
    pub fn lower_at(&self, x: f64) -> f64 {
        lerp(&self.lower, x)
    }

    /// Относительная толщина.
    pub fn thickness_at(&self, x: f64) -> f64 {
        self.upper_at(x) - self.lower_at(x)
    }

    /// Средняя линия.
    pub fn camber_at(&self, x: f64) -> f64 {
        0.5 * (self.upper_at(x) + self.lower_at(x))
    }

    /// Наибольшая толщина и где она.
    pub fn max_thickness(&self) -> (f64, f64) {
        (0..=1000)
            .map(|k| {
                let x = k as f64 / 1000.0;
                (self.thickness_at(x), x)
            })
            .fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a })
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
        let mut pts: Vec<(f64, f64)> = self
            .upper
            .iter()
            .rev()
            .map(|&(x, y)| (x * chord, y * chord))
            .collect();
        pts.extend(
            self.lower
                .iter()
                .skip(1)
                .map(|&(x, y)| (x * chord, y * chord)),
        );
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

fn lerp(pts: &[(f64, f64)], x: f64) -> f64 {
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
