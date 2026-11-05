//! Расчёт консоли как балки, заделанной у корневой нервюры.
//!
//! 1. Расчётная подъёмная сила `L = n · f · m · g`, на консоль — `L / 2`.
//! 2. Погонная нагрузка `q(z)` по выбранной схеме (по умолчанию — Шренк:
//!    среднее трапециевидного и эллиптического распределений).
//! 3. Перерезывающая сила `Q(z)` и изгибающий момент `M(z)` — интегрированием от конца.
//! 4. Момент делится между лонжеронами, проходящими через станцию, пропорционально `EI`.
//!    Напряжение `σ = M·c / I`, запас = предел прочности / σ.
//! 5. Прогиб — двойным интегрированием кривизны `M / ΣEI` от заделки.
//! 6. Нагрузка на каждую нервюру — интеграл `q` по её полосе (между серединами шага).
//! 7. Соединительная трубка: смятие кромок отверстий в нервюрах от пары сил,
//!    которой консоль передаёт корневой момент на трубку.

use crate::material::{Material, find};
use crate::wing::{Spar, Wing};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Ускорение свободного падения, м/с².
pub const G: f64 = 9.81;

/// Схема распределения нагрузки по размаху.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Distribution {
    /// Шренк: среднее трапециевидного и эллиптического (стандарт для моделей).
    Schrenk,
    /// Эллиптическое.
    Elliptic,
    /// Пропорционально хорде.
    Trapezoidal,
    /// Равномерное (для проверочных задач).
    Uniform,
}

/// Расчётный случай.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoadCase {
    /// Полётная масса, кг.
    pub mass_kg: f64,
    /// Эксплуатационная перегрузка n.
    pub load_factor: f64,
    /// Коэффициент безопасности f.
    pub safety_factor: f64,
    /// Распределение по размаху.
    pub distribution: Distribution,
}

impl LoadCase {
    /// Проверка.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.mass_kg > 0.0 && self.mass_kg < 50.0) {
            return Err("Масса должна быть от 0 до 50 кг".into());
        }
        if !(1.0..=20.0).contains(&self.load_factor) {
            return Err("Перегрузка вне диапазона 1…20".into());
        }
        if !(1.0..=5.0).contains(&self.safety_factor) {
            return Err("Коэффициент безопасности вне диапазона 1…5".into());
        }
        Ok(())
    }

    /// Расчётная подъёмная сила всего крыла, Н.
    pub fn design_lift_n(&self) -> f64 {
        self.load_factor * self.safety_factor * self.mass_kg * G
    }
}

/// Нервюры: шаг и материал (нужны для нагрузок на нервюры и смятия отверстий).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RibLayout {
    /// Шаг нервюр по размаху, мм. Первая нервюра — у корня.
    pub pitch_mm: f64,
    /// Толщина нервюры, мм.
    pub thickness_mm: f64,
    /// Материал нервюр.
    pub material: String,
    /// Минимальная стенка нервюры над и под лонжероном, мм.
    pub min_wall_mm: f64,
}

/// Всё, что нужно для расчёта.
#[derive(Clone, Debug)]
pub struct Input<'a> {
    /// Крыло.
    pub wing: &'a Wing,
    /// Расчётный случай.
    pub load: &'a LoadCase,
    /// Лонжероны и трубки одной консоли.
    pub spars: &'a [Spar],
    /// Библиотека материалов.
    pub materials: &'a [Material],
    /// Нервюры.
    pub ribs: &'a RibLayout,
    /// Относительная толщина профиля t/c как функция положения по хорде x/c
    /// (таблица точек). Нужна для проверки, помещается ли лонжерон в профиль.
    pub thickness_table: &'a [(f64, f64)],
}

/// Значения на одной станции расчётной сетки.
#[derive(Clone, Debug, Serialize)]
pub struct Station {
    /// Расстояние от корня, мм.
    pub z: f64,
    /// Хорда, мм.
    pub chord: f64,
    /// Погонная нагрузка, Н/мм.
    pub q: f64,
    /// Перерезывающая сила, Н.
    pub shear: f64,
    /// Изгибающий момент, Н·мм.
    pub moment: f64,
    /// Напряжение в каждом лонжероне (None — лонжерон здесь не проходит), МПа.
    pub spar_stress: Vec<Option<f64>>,
    /// Прогиб, мм (None — за участком без лонжерона прогиб не определён).
    pub deflection: Option<f64>,
}

/// Итог по лонжерону.
#[derive(Clone, Debug, Serialize)]
pub struct SparSummary {
    /// Название.
    pub name: String,
    /// Наибольшее напряжение, МПа.
    pub max_stress_mpa: f64,
    /// Где оно достигается, мм от корня.
    pub z_at_max: f64,
    /// Запас прочности: предел прочности / наибольшее напряжение (нагрузка уже с коэффициентом f).
    pub margin: f64,
    /// Масса в пределах консоли, г.
    pub mass_g: f64,
    /// Высота лонжерона и доступная толщина профиля в самом тонком месте его участка, мм.
    pub fit: (f64, f64),
}

/// Нагрузка на нервюру.
#[derive(Clone, Debug, Serialize)]
pub struct RibLoad {
    /// Номер от корня, с нуля.
    pub index: usize,
    /// Станция, мм от корня.
    pub z: f64,
    /// Хорда, мм.
    pub chord: f64,
    /// Полоса крыла, с которой нервюра собирает нагрузку, мм.
    pub strip: (f64, f64),
    /// Расчётная сила на нервюру, Н.
    pub force_n: f64,
}

/// Проверка соединительной трубки.
#[derive(Clone, Debug, Serialize)]
pub struct JoinerCheck {
    /// Название трубки.
    pub name: String,
    /// Конец трубки в консоли, мм.
    pub end_z: f64,
    /// Момент у корня, который трубка передаёт в фюзеляж, Н·мм.
    pub root_moment: f64,
    /// Момент в конце трубки, который должен перейти на другой лонжерон, Н·мм.
    pub moment_at_end: f64,
    /// Нервюры, через которые проходит трубка, мм от корня.
    pub ribs_on_tube: Vec<f64>,
    /// Плечо пары сил между крайними нервюрами на трубке, мм.
    pub arm_mm: f64,
    /// Сила на отверстие крайней нервюры (приближение «две опоры»), Н.
    pub bearing_force_n: f64,
    /// Напряжение смятия кромки отверстия, МПа.
    pub bearing_stress_mpa: f64,
    /// Запас по смятию.
    pub bearing_margin: f64,
    /// Длина перекрытия с другими лонжеронами за концом трубки, мм.
    pub overlap_mm: f64,
}

/// Результат расчёта консоли.
#[derive(Clone, Debug, Serialize)]
pub struct Analysis {
    /// Расчётная подъёмная сила всего крыла, Н.
    pub design_lift_n: f64,
    /// На одну консоль, Н.
    pub half_lift_n: f64,
    /// Нагрузка на крыло (без перегрузки), г/дм².
    pub wing_loading_g_dm2: f64,
    /// Площадь, дм².
    pub area_dm2: f64,
    /// САХ, мм.
    pub mac_mm: f64,
    /// Перерезывающая сила у корня, Н.
    pub root_shear_n: f64,
    /// Изгибающий момент у корня, Н·мм.
    pub root_moment_nmm: f64,
    /// Станции.
    pub stations: Vec<Station>,
    /// Лонжероны.
    pub spars: Vec<SparSummary>,
    /// Прогиб конца консоли, мм.
    pub tip_deflection_mm: Option<f64>,
    /// Нагрузки на нервюры.
    pub ribs: Vec<RibLoad>,
    /// Проверки соединительных трубок.
    pub joiners: Vec<JoinerCheck>,
    /// Предупреждения: что нельзя пропускать.
    pub warnings: Vec<String>,
}

/// Число интервалов расчётной сетки по полуразмаху.
pub const N: usize = 1000;

/// Доля корневого момента, ниже которой участок без лонжерона не считается проблемой.
const GAP_THRESHOLD: f64 = 0.005;

/// Посчитать консоль.
pub fn analyze(inp: &Input) -> Result<Analysis, String> {
    let wing = inp.wing;
    wing.validate()?;
    inp.load.validate()?;
    if !(inp.ribs.pitch_mm >= 10.0 && inp.ribs.pitch_mm <= wing.half_span()) {
        return Err("Шаг нервюр должен быть от 10 мм до длины консоли".into());
    }
    if !inp.ribs.thickness_mm.is_finite() || inp.ribs.thickness_mm <= 0.0 {
        return Err("Толщина нервюры должна быть больше нуля".into());
    }
    let mut spar_mats = Vec::new();
    for s in inp.spars {
        s.validate(wing)?;
        let m = find(inp.materials, &s.material).ok_or_else(|| {
            format!(
                "{}: материал «{}» не найден в библиотеке",
                s.name, s.material
            )
        })?;
        spar_mats.push(m);
    }
    let rib_mat = find(inp.materials, &inp.ribs.material).ok_or_else(|| {
        format!(
            "Материал нервюр «{}» не найден в библиотеке",
            inp.ribs.material
        )
    })?;

    let s = wing.half_span();
    let dz = s / N as f64;
    let lift = inp.load.design_lift_n();
    let half = lift / 2.0;
    let zs: Vec<f64> = (0..=N).map(|i| i as f64 * dz).collect();

    // 1–2. Погонная нагрузка и нормировка, чтобы интеграл по консоли был ровно L/2.
    let raw: Vec<f64> = zs
        .iter()
        .map(|&z| shape(wing, inp.load.distribution, z))
        .collect();
    let integral = trapz(&raw, dz);
    let q: Vec<f64> = raw.iter().map(|v| v * half / integral).collect();

    // 3. Q и M от конца к корню.
    let mut shear = vec![0.0; N + 1];
    let mut moment = vec![0.0; N + 1];
    for i in (0..N).rev() {
        shear[i] = shear[i + 1] + 0.5 * (q[i] + q[i + 1]) * dz;
        moment[i] = moment[i + 1] + 0.5 * (shear[i] + shear[i + 1]) * dz;
    }
    let m0 = moment[0];

    // 4. Распределение момента по лонжеронам.
    let ei: Vec<f64> = inp
        .spars
        .iter()
        .zip(&spar_mats)
        .map(|(sp, m)| m.e_mpa * sp.section.inertia())
        .collect();
    let mut stations = Vec::with_capacity(N + 1);
    let mut gap: Option<(f64, f64, f64)> = None; // начало, конец, наибольший момент
    let mut gaps = Vec::new();
    let mut curvature = vec![Some(0.0); N + 1];
    for i in 0..=N {
        let z = zs[i];
        let total_ei: f64 = inp
            .spars
            .iter()
            .zip(&ei)
            .filter(|(sp, _)| sp.covers(z))
            .map(|(_, e)| e)
            .sum();
        let stresses: Vec<Option<f64>> = inp
            .spars
            .iter()
            .zip(&ei)
            .map(|(sp, &e)| {
                (sp.covers(z) && total_ei > 0.0)
                    .then(|| moment[i] * (e / total_ei) * sp.section.c_max() / sp.section.inertia())
            })
            .collect();
        let significant = moment[i] > GAP_THRESHOLD * m0;
        if total_ei > 0.0 {
            curvature[i] = Some(moment[i] / total_ei);
        } else if significant {
            curvature[i] = None;
        }
        if total_ei == 0.0 && significant {
            gap = Some(match gap {
                Some((a, _, mm)) => (a, z, mm.max(moment[i])),
                None => (z, z, moment[i]),
            });
        } else if let Some(g) = gap.take() {
            gaps.push(g);
        }
        stations.push(Station {
            z,
            chord: wing.chord_at(z),
            q: q[i],
            shear: shear[i],
            moment: moment[i],
            spar_stress: stresses,
            deflection: None,
        });
    }
    if let Some(g) = gap.take() {
        gaps.push(g);
    }

    // 5. Прогиб: θ и v от заделки.
    let (mut theta, mut v) = (0.0_f64, 0.0_f64);
    stations[0].deflection = Some(0.0);
    let mut defined = curvature[0].is_some();
    for i in 0..N {
        match (defined, curvature[i], curvature[i + 1]) {
            (true, Some(k0), Some(k1)) => {
                let theta1 = theta + 0.5 * (k0 + k1) * dz;
                v += 0.5 * (theta + theta1) * dz;
                theta = theta1;
                stations[i + 1].deflection = Some(v);
            }
            _ => defined = false,
        }
    }
    let tip_deflection_mm = stations[N].deflection;

    let mut warnings = Vec::new();
    for (a, b, mm) in &gaps {
        warnings.push(format!(
            "Участок z = {:.0}–{:.0} мм: изгибающий момент до {:.2} Н·м некому воспринять — нужен лонжерон.",
            a,
            b,
            mm / 1000.0
        ));
    }
    if inp.spars.is_empty() {
        warnings.push("Не задано ни одного лонжерона.".into());
    }

    // Итоги по лонжеронам.
    let mut spars = Vec::new();
    for (j, (sp, m)) in inp.spars.iter().zip(&spar_mats).enumerate() {
        let (mut max_s, mut z_max) = (0.0_f64, sp.z_start);
        for st in &stations {
            if let Some(sig) = st.spar_stress[j]
                && sig > max_s
            {
                max_s = sig;
                z_max = st.z;
            }
        }
        let margin = if max_s > 0.0 {
            m.strength_mpa / max_s
        } else {
            f64::INFINITY
        };
        let len = sp.z_end - sp.z_start;
        let mass_g = sp.section.area() * len * m.density_g_cm3 / 1000.0;
        // Самое тонкое место профиля на участке лонжерона — у его внешнего конца.
        let chord_min = wing.chord_at(sp.z_end).min(wing.chord_at(sp.z_start));
        let t_rel = interp(inp.thickness_table, sp.chord_pos);
        let available = t_rel * chord_min - 2.0 * inp.ribs.min_wall_mm;
        if !inp.thickness_table.is_empty() && sp.section.height() > available {
            warnings.push(format!(
                "{}: высота {:.1} мм не помещается в профиль с учётом стенок нервюры (доступно {:.1} мм на z = {:.0} мм).",
                sp.name,
                sp.section.height(),
                available,
                sp.z_end
            ));
        }
        if margin < 1.0 {
            warnings.push(format!(
                "{}: напряжение {:.0} МПа больше предела прочности {:.0} МПа — запас {:.2}.",
                sp.name, max_s, m.strength_mpa, margin
            ));
        }
        if !m.verified {
            warnings.push(format!(
                "{}: свойства материала «{}» не проверены — результат оценочный.",
                sp.name, m.name
            ));
        }
        spars.push(SparSummary {
            name: sp.name.clone(),
            max_stress_mpa: max_s,
            z_at_max: z_max,
            margin,
            mass_g,
            fit: (sp.section.height(), available),
        });
    }

    // 6. Нагрузки на нервюры.
    let ribs = rib_loads(wing, &zs, &q, inp.ribs.pitch_mm);

    // 7. Соединительные трубки.
    let mut joiners = Vec::new();
    for sp in inp.spars.iter().filter(|s| s.joiner) {
        let on: Vec<f64> = ribs.iter().map(|r| r.z).filter(|&z| sp.covers(z)).collect();
        let arm = match (on.first(), on.last()) {
            (Some(a), Some(b)) => b - a,
            _ => 0.0,
        };
        let m_root = sample(&zs, &moment, sp.z_start);
        let m_end = sample(&zs, &moment, sp.z_end);
        let force = if arm > 0.0 {
            m_root / arm
        } else {
            f64::INFINITY
        };
        let bearing = force / (sp.section.width() * inp.ribs.thickness_mm);
        let overlap = inp
            .spars
            .iter()
            .filter(|o| !std::ptr::eq(*o, sp) && o.z_end > sp.z_end && o.z_start < sp.z_end)
            .map(|o| sp.z_end - o.z_start.max(sp.z_start))
            .fold(0.0_f64, f64::max);
        if sp.z_end < wing.half_span() - 1e-9
            && m_end > GAP_THRESHOLD * m0
            && overlap < inp.ribs.pitch_mm
        {
            warnings.push(format!(
                "{}: в конце трубки (z = {:.0} мм) момент {:.2} Н·м должен перейти на соседний лонжерон, а перекрытие {:.0} мм меньше шага нервюр — нужен стык (муфта, вклейка) или перекрытие. Расчёт прогиба считает стык неразрезным.",
                sp.name,
                sp.z_end,
                m_end / 1000.0,
                overlap
            ));
        }
        if on.len() < 2 {
            warnings.push(format!(
                "{}: проходит меньше чем через две нервюры — консоль не может передать на неё момент.",
                sp.name
            ));
        }
        let margin = rib_mat.bearing_mpa / bearing;
        if margin < 1.5 {
            warnings.push(format!(
                "{}: смятие кромки отверстия в нервюре {:.0} МПа, запас {:.2} — нужна втулка или накладка.",
                sp.name, bearing, margin
            ));
        }
        joiners.push(JoinerCheck {
            name: sp.name.clone(),
            end_z: sp.z_end,
            root_moment: m_root,
            moment_at_end: m_end,
            ribs_on_tube: on,
            arm_mm: arm,
            bearing_force_n: force,
            bearing_stress_mpa: bearing,
            bearing_margin: margin,
            overlap_mm: overlap,
        });
    }
    if !rib_mat.verified {
        warnings.push(format!(
            "Нервюры: свойства материала «{}» не проверены — результат оценочный.",
            rib_mat.name
        ));
    }

    Ok(Analysis {
        design_lift_n: lift,
        half_lift_n: half,
        wing_loading_g_dm2: inp.load.mass_kg * 1000.0 / (wing.area_mm2() / 1e4),
        area_dm2: wing.area_mm2() / 1e4,
        mac_mm: wing.mac_mm(),
        root_shear_n: shear[0],
        root_moment_nmm: m0,
        stations,
        spars,
        tip_deflection_mm,
        ribs,
        joiners,
        warnings,
    })
}

/// Форма распределения (без нормировки).
fn shape(wing: &Wing, d: Distribution, z: f64) -> f64 {
    let s = wing.half_span();
    let trap = wing.chord_at(z) / wing.area_mm2();
    let ell = 4.0 / (PI * wing.span_mm) * (1.0 - (z / s).powi(2)).max(0.0).sqrt();
    match d {
        Distribution::Schrenk => 0.5 * (trap + ell),
        Distribution::Elliptic => ell,
        Distribution::Trapezoidal => trap,
        Distribution::Uniform => 1.0,
    }
}

fn trapz(y: &[f64], dz: f64) -> f64 {
    y.windows(2).map(|w| 0.5 * (w[0] + w[1]) * dz).sum()
}

/// Значение функции, заданной на сетке, в точке `z` (линейная интерполяция).
fn sample(zs: &[f64], v: &[f64], z: f64) -> f64 {
    let dz = zs[1] - zs[0];
    let t = (z / dz).clamp(0.0, (zs.len() - 1) as f64);
    let i = (t.floor() as usize).min(zs.len() - 2);
    let f = t - i as f64;
    v[i] * (1.0 - f) + v[i + 1] * f
}

/// Интеграл q от a до b по сетке (с долями крайних интервалов).
fn integrate_q(zs: &[f64], q: &[f64], a: f64, b: f64) -> f64 {
    let n = 200;
    let h = (b - a) / n as f64;
    if h <= 0.0 {
        return 0.0;
    }
    (0..n)
        .map(|k| {
            let z0 = a + k as f64 * h;
            0.5 * (sample(zs, q, z0) + sample(zs, q, z0 + h)) * h
        })
        .sum()
}

fn rib_loads(wing: &Wing, zs: &[f64], q: &[f64], pitch: f64) -> Vec<RibLoad> {
    let s = wing.half_span();
    let mut stations: Vec<f64> = Vec::new();
    let mut z = 0.0;
    while z < s - 1e-6 {
        stations.push(z);
        z += pitch;
    }
    stations.push(s);
    // Если последняя нервюра ближе трети шага к концевой — объединяем.
    if stations.len() >= 3 {
        let k = stations.len();
        if stations[k - 1] - stations[k - 2] < pitch / 3.0 {
            stations.remove(k - 2);
        }
    }
    let k = stations.len();
    (0..k)
        .map(|i| {
            let a = if i == 0 {
                0.0
            } else {
                0.5 * (stations[i - 1] + stations[i])
            };
            let b = if i + 1 == k {
                s
            } else {
                0.5 * (stations[i] + stations[i + 1])
            };
            RibLoad {
                index: i,
                z: stations[i],
                chord: wing.chord_at(stations[i]),
                strip: (a, b),
                force_n: integrate_q(zs, q, a, b),
            }
        })
        .collect()
}

/// Линейная интерполяция по таблице (x, y), x по возрастанию.
pub fn interp(table: &[(f64, f64)], x: f64) -> f64 {
    if table.is_empty() {
        return 0.0;
    }
    if x <= table[0].0 {
        return table[0].1;
    }
    for w in table.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
            return y0 + (y1 - y0) * t;
        }
    }
    table[table.len() - 1].1
}
