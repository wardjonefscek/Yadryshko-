//! Cad+: нервюры — от нагрузки из расчёта крыла до контура для ЧПУ или печати.
//!
//! 1. [`geom::build`] — контур профиля, отверстия под лонжероны с посадкой, проверка стенок.
//! 2. [`setup::build`] — сетка, запретные зоны (кромка, кольца), закрепление на лонжеронах,
//!    нагрузка на верхний контур.
//! 3. [`optimize`] — топооптимизация SIMP ([`cadplus_topopt`]).
//! 4. [`finalize`] — облегчающие вырезы с учётом технологии: для ЧПУ остаются только места,
//!    куда проходит фреза; контуры, масса до и после.
//! 5. [`export`] — DXF и SVG.

pub mod airfoil;
pub mod contour;
pub mod export;
pub mod geom;
pub mod setup;

use cadplus_topopt::{Cell, Control, Iteration, Settings, Solution};
use geom::{Pt, RibGeometry, polygon_area};
use serde::{Deserialize, Serialize};
use setup::{Grid, RibSetup};

/// Технология изготовления нервюр.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Manufacturing {
    /// Фрезерование листа на ЧПУ.
    Cnc {
        /// Диаметр режущей части фрезы, мм.
        cutter_d_mm: f64,
        /// Рабочее поле станка, мм.
        sheet_mm: (f64, f64),
    },
    /// FDM-печать.
    Fdm {
        /// Диаметр сопла, мм.
        nozzle_mm: f64,
        /// Область печати, мм.
        bed_mm: (f64, f64),
    },
}

impl Manufacturing {
    /// Радиус «инструмента»: вырез меньше этого не изготовить.
    pub fn tool_radius_mm(&self) -> f64 {
        match self {
            Manufacturing::Cnc { cutter_d_mm, .. } => cutter_d_mm / 2.0,
            Manufacturing::Fdm { nozzle_mm, .. } => *nozzle_mm,
        }
    }

    /// Рабочее поле.
    pub fn field_mm(&self) -> (f64, f64) {
        match self {
            Manufacturing::Cnc { sheet_mm, .. } => *sheet_mm,
            Manufacturing::Fdm { bed_mm, .. } => *bed_mm,
        }
    }

    /// Это ЧПУ.
    pub fn is_cnc(&self) -> bool {
        matches!(self, Manufacturing::Cnc { .. })
    }
}

/// Настройки нервюр.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RibSettings {
    /// Размер элемента сетки, мм.
    pub element_mm: f64,
    /// Сплошная кромка вдоль контура (под обтяжку плёнкой), мм.
    pub edge_band_mm: f64,
    /// Сплошное кольцо вокруг отверстий, мм.
    pub ring_mm: f64,
    /// Доля материала, которая остаётся в оптимизируемой области.
    pub volfrac: f64,
    /// Радиус фильтра, мм: задаёт минимальную толщину перемычек.
    pub filter_mm: f64,
    /// Показатель штрафа SIMP.
    pub penal: f64,
    /// Предел итераций.
    pub max_iter: usize,
    /// Порог плотности: ниже — вырез.
    pub threshold: f64,
    /// Посадка отверстия под лонжерон (прибавка к диаметру), мм.
    pub fit_clearance_mm: f64,
    /// Минимальная стенка над и под лонжероном, мм.
    pub min_wall_mm: f64,
    /// Технология.
    pub manufacturing: Manufacturing,
}

impl Default for RibSettings {
    /// Под станок из сводки: фреза 2 мм, поле 600×600 мм.
    fn default() -> Self {
        Self {
            element_mm: 0.75,
            edge_band_mm: 3.0,
            ring_mm: 3.0,
            volfrac: 0.35,
            filter_mm: 2.0,
            penal: 3.0,
            max_iter: 300,
            threshold: 0.5,
            fit_clearance_mm: 0.1,
            min_wall_mm: 2.0,
            manufacturing: Manufacturing::Cnc {
                cutter_d_mm: 2.0,
                sheet_mm: (600.0, 600.0),
            },
        }
    }
}

impl RibSettings {
    /// Проверка диапазонов.
    pub fn validate(&self) -> Result<(), String> {
        let chk = |what: &str, v: f64, lo: f64, hi: f64| {
            if v.is_finite() && (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(format!("Нервюры: {what} = {v} вне диапазона {lo}…{hi}"))
            }
        };
        chk("размер элемента, мм", self.element_mm, 0.2, 3.0)?;
        chk("кромка, мм", self.edge_band_mm, 0.5, 20.0)?;
        chk("кольцо, мм", self.ring_mm, 0.5, 20.0)?;
        chk("доля материала", self.volfrac, 0.05, 0.95)?;
        chk("радиус фильтра, мм", self.filter_mm, 0.2, 20.0)?;
        chk("порог плотности", self.threshold, 0.1, 0.9)?;
        chk("посадка, мм", self.fit_clearance_mm, 0.0, 2.0)?;
        chk("минимальная стенка, мм", self.min_wall_mm, 0.0, 20.0)?;
        chk(
            "радиус инструмента, мм",
            self.manufacturing.tool_radius_mm(),
            0.1,
            10.0,
        )?;
        Ok(())
    }

    /// Параметры решателя.
    pub fn solver(&self) -> Settings {
        Settings {
            volfrac: self.volfrac,
            penal: self.penal,
            rmin: (self.filter_mm / self.element_mm).max(1.1),
            max_iter: self.max_iter,
            ..Settings::default()
        }
    }
}

/// Готовая нервюра.
#[derive(Clone, Debug, Serialize)]
pub struct RibResult {
    /// Геометрия.
    pub geometry: RibGeometry,
    /// Облегчающие вырезы, мм.
    pub pockets: Vec<Vec<Pt>>,
    /// Площадь до оптимизации (контур минус отверстия), мм².
    pub area_before_mm2: f64,
    /// Площадь после, мм².
    pub area_after_mm2: f64,
    /// Масса одной нервюры до, г.
    pub mass_before_g: f64,
    /// Масса одной нервюры после, г.
    pub mass_after_g: f64,
    /// Податливость в конце расчёта, Н·мм.
    pub compliance: f64,
    /// Итераций.
    pub iterations: usize,
    /// Сошёлся ли расчёт.
    pub converged: bool,
    /// Предупреждения.
    pub warnings: Vec<String>,
}

/// Запустить оптимизацию. `on_iter` получает сведения и плотности после каждой итерации.
pub fn optimize<F>(setup: &RibSetup, settings: &RibSettings, on_iter: F) -> Result<Solution, String>
where
    F: FnMut(&Iteration, &[f64]) -> Control,
{
    settings.validate()?;
    cadplus_topopt::solve(&setup.problem, &settings.solver(), on_iter).map_err(|e| e.to_string())
}

/// Перевести плотности в вырезы с учётом технологии и посчитать массу.
pub fn finalize(
    setup: &RibSetup,
    density: &[f64],
    sol: Option<&Solution>,
    settings: &RibSettings,
    thickness_mm: f64,
    density_g_cm3: f64,
) -> RibResult {
    let g = &setup.geometry;
    let grid: &Grid = &setup.grid;
    let cells = &setup.problem.cells;
    let pocket: Vec<bool> = cells
        .iter()
        .zip(density)
        .map(|(&c, &d)| c == Cell::Active && d < settings.threshold)
        .collect();
    // Вырез должен пропускать инструмент: открытие маски кругом радиуса инструмента
    // (с запасом в полэлемента на дискретизацию).
    let r_px = (settings.manufacturing.tool_radius_mm() + 0.5 * grid.h) / grid.h;
    let millable = contour::open(&pocket, grid.nelx, grid.nely, r_px);
    let mut pockets: Vec<Vec<Pt>> = contour::trace(&millable, grid)
        .into_iter()
        .map(|p| contour::simplify(&contour::chaikin(&p, 2), 0.02))
        .filter(|p| p.len() >= 3)
        .collect();
    // Вырезы меньше круга двойного диаметра инструмента почти не облегчают деталь,
    // а время обработки добавляют — оставляем там материал.
    let r = settings.manufacturing.tool_radius_mm();
    let min_area = 4.0 * std::f64::consts::PI * r * r;
    pockets.retain(|p| polygon_area(p) > min_area);
    let holes: f64 = g.holes.iter().map(|h| h.area()).sum();
    let before = g.outline_area() - holes;
    let cut: f64 = pockets.iter().map(|p| polygon_area(p)).sum();
    let after = before - cut;
    let mass = |a: f64| a * thickness_mm * density_g_cm3 / 1000.0;
    let mut warnings = Vec::new();
    let removed_by_tool = pocket
        .iter()
        .zip(&millable)
        .filter(|(a, b)| **a && !**b)
        .count();
    if removed_by_tool > 0 {
        warnings.push(format!(
            "Нервюра {}: {:.0} мм² узких вырезов закрыто материалом — туда не проходит инструмент.",
            g.index + 1,
            removed_by_tool as f64 * grid.h * grid.h
        ));
    }
    if let Some(s) = sol
        && !s.converged
    {
        warnings.push(format!(
            "Нервюра {}: расчёт {} за {} итераций — результат промежуточный.",
            g.index + 1,
            if s.cancelled {
                "остановлен"
            } else {
                "не сошёлся"
            },
            s.iterations
        ));
    }
    RibResult {
        geometry: g.clone(),
        pockets,
        area_before_mm2: before,
        area_after_mm2: after,
        mass_before_g: mass(before),
        mass_after_g: mass(after),
        compliance: sol.map_or(0.0, |s| s.compliance),
        iterations: sol.map_or(0, |s| s.iterations),
        converged: sol.is_some_and(|s| s.converged),
        warnings,
    }
}

/// Построить геометрию всех нервюр консоли по результатам расчёта крыла.
pub fn prepare_all(
    af: &airfoil::Airfoil,
    analysis: &cadplus_aero::Analysis,
    spars: &[cadplus_aero::Spar],
    settings: &RibSettings,
    print_tolerance_mm: f64,
) -> Vec<(RibGeometry, Vec<String>)> {
    let clearance = settings.fit_clearance_mm
        + if settings.manufacturing.is_cnc() {
            0.0
        } else {
            print_tolerance_mm
        };
    analysis
        .ribs
        .iter()
        .map(|r| {
            geom::build(
                af,
                r,
                spars,
                clearance,
                settings.min_wall_mm,
                settings.manufacturing.is_cnc(),
            )
        })
        .collect()
}

/// Параметры сетки из настроек и материала.
pub fn grid_params(
    settings: &RibSettings,
    e_mpa: f64,
    nu: f64,
    thickness_mm: f64,
) -> setup::GridParams {
    setup::GridParams {
        element_mm: settings.element_mm,
        edge_band_mm: settings.edge_band_mm,
        ring_mm: settings.ring_mm,
        e_mpa,
        nu,
        thickness_mm,
    }
}
