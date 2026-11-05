//! Cad+: нервюры — от нагрузки из расчёта крыла до контура для ЧПУ или печати.
//!
//! 1. [`geom::build`] — контур профиля, отверстия под лонжероны с посадкой, проверка стенок.
//! 2. [`setup::build`] — сетка, запретные зоны (кромка, кольца), закрепление на лонжеронах,
//!    нагрузка на верхний контур.
//! 3. [`optimize`] — топооптимизация SIMP ([`cadplus_topopt`]).
//! 4. [`finalize`] — облегчающие вырезы с учётом технологии: контур по полю плотности,
//!    только места, куда проходит фреза (или сопло); масса до и после.
//! 5. [`export`] — DXF и SVG.
//!
//! Геометрию строит собственное ядро yadro (`yadro_core`): профиль из .dat, деталь
//! «контур минус отверстия», вырезы из поля плотности, проверку под инструмент,
//! восстановление дуг и файлы для станка. Все операции — точные, на сетке 1 мкм.

pub mod airfoil;
pub mod export;
pub mod geom;
pub mod setup;

use cadplus_topopt::{Cell, Control, Iteration, Settings, Solution};
use geom::{Pt, RibGeometry};
use serde::{Deserialize, Serialize};
use setup::{Grid, RibSetup};
use yadro_core::{ContourOptions, DensityField, Region, Vec2};

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

/// Во сколько раз уплотнять поле плотности перед построением контура: граница
/// получается плавной, а не по клеткам сетки.
const CONTOUR_REFINE: usize = 2;

/// Перевести плотности в вырезы с учётом технологии и посчитать массу.
///
/// 1. Поле плотности → область материала (ядро: marching squares с уплотнением).
///    Вне оптимизируемой области (кромка, кольца, отверстия, снаружи) плотность
///    считается единицей — там всегда материал.
/// 2. Вырезы = деталь минус материал; островки материала внутри выреза убираются
///    (они ни к чему не крепятся).
/// 3. Инструмент: остаётся только то, куда целиком проходит круг радиуса фрезы или
///    сопла (opening), — узкие щели закрываются, углы вырезов скругляются.
/// 4. Вырезы меньше круга двойного диаметра инструмента не делаются.
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
    let part0 = g.region();
    let mut warnings = Vec::new();
    // Ряд j = 0 у Cad+ — верхний, у ядра — нижний.
    let origin = Vec2::new(grid.x0, grid.ytop - grid.nely as f64 * grid.h);
    let field = DensityField::from_fn(grid.nelx, grid.nely, grid.h, origin, |i, jb| {
        let e = i * grid.nely + (grid.nely - 1 - jb);
        if cells[e] == Cell::Active {
            density[e]
        } else {
            1.0
        }
    });
    let opts = ContourOptions {
        threshold: settings.threshold,
        refine: CONTOUR_REFINE,
        ..ContourOptions::default()
    };
    let material = match field.and_then(|f| f.contour(&opts)) {
        Ok(m) => m,
        Err(e) => {
            warnings.push(format!(
                "Нервюра {}: контур по полю плотности не построен ({e}) — нервюра сплошная.",
                g.index + 1
            ));
            part0.clone()
        }
    };
    let raw = part0.difference(&material).drop_small(0.0, f64::INFINITY);
    let r = settings.manufacturing.tool_radius_mm();
    let millable = raw
        .opening(r)
        .unwrap_or_else(|_| Region::empty())
        .drop_small(0.0, f64::INFINITY);
    let min_area = 4.0 * std::f64::consts::PI * r * r;
    let pockets_region = millable.drop_small(min_area, 0.0);
    let part = part0.difference(&pockets_region);
    let pockets: Vec<Vec<Pt>> = pockets_region
        .rings()
        .iter()
        .map(|ring| ring.to_mm().into_iter().map(|p| (p.x, p.y)).collect())
        .collect();
    let before = part0.area_mm2();
    let after = part.area_mm2();
    let mass = |a: f64| a * thickness_mm * density_g_cm3 / 1000.0;
    // Скругление углов вырезов фрезой — не повод для предупреждения; закрытая щель — да.
    let closed = raw.area_mm2() - millable.area_mm2();
    if closed > 4.0 * r * r {
        warnings.push(format!(
            "Нервюра {}: {closed:.0} мм² узких вырезов закрыто материалом — туда не проходит инструмент.",
            g.index + 1
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
