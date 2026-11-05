//! Экспорт нервюр: DXF R12 для ЧПУ и SVG в мм — через лист ядра yadro.
//!
//! Слои: `CONTOUR` — наружный контур, `HOLES` — отверстия под лонжероны (окружности —
//! объектом CIRCLE), `POCKETS` — облегчающие вырезы, `LABELS` — подписи (в CAM этот
//! слой не резать). Вырезы идут дугами и отрезками: ядро восстанавливает их из
//! ломаной с допуском [`POCKET_FIT_TOL_MM`] — G-кода меньше, фреза идёт ровнее.
//! Детали раскладываются столбцами с зазором и проверяются на размер рабочего поля.

use crate::geom::{Pt, RibGeometry};
use yadro_core::fit::fit_region;
use yadro_core::{Path, Region, Ring, Sheet, Vec2};

/// Допуск восстановления дуг у облегчающих вырезов, мм. Граница выреза идёт по
/// полю плотности с шагом сетки 0,5–1 мм; 20 мкм — в десятки раз точнее и фрезы,
/// и сетки, а участков выходит в разы меньше, чем при 5 мкм.
pub const POCKET_FIT_TOL_MM: f64 = 0.02;

/// Деталь для раскладки.
#[derive(Clone, Copy, Debug)]
pub struct Part<'a> {
    /// Геометрия нервюры.
    pub geometry: &'a RibGeometry,
    /// Облегчающие вырезы.
    pub pockets: &'a [Vec<Pt>],
}

/// Раскладка: смещения деталей и общий размер, мм.
pub struct Layout {
    /// Смещение каждой копии: (номер детали, dx, dy).
    pub placements: Vec<(usize, f64, f64)>,
    /// Ширина и высота раскладки.
    pub size: (f64, f64),
}

/// Разложить детали столбцами снизу вверх; каждая нервюра — столько копий, сколько нужно.
pub fn layout(parts: &[Part], sheet: (f64, f64), gap: f64) -> Layout {
    let mut placements = Vec::new();
    let (mut x, mut y, mut col_w, mut total_w, mut total_h) = (gap, gap, 0.0_f64, 0.0_f64, 0.0_f64);
    for (k, p) in parts.iter().enumerate() {
        let (ymin, ymax) = p
            .geometry
            .outline
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q.1), b.max(q.1)));
        let h = ymax - ymin;
        for _ in 0..p.geometry.quantity.max(1) {
            if y + h + gap > sheet.1 && y > gap {
                x += col_w + gap;
                y = gap;
                col_w = 0.0;
            }
            placements.push((k, x, y - ymin));
            y += h + gap;
            col_w = col_w.max(p.geometry.chord);
            total_w = total_w.max(x + col_w + gap);
            total_h = total_h.max(y);
        }
    }
    Layout {
        placements,
        size: (total_w, total_h),
    }
}

/// Вырезы детали дугами и отрезками (один раз на деталь, а не на каждую копию).
fn pocket_paths(pockets: &[Vec<Pt>]) -> Vec<Path> {
    let rings: Vec<Ring> = pockets
        .iter()
        .filter_map(|p| {
            let pts: Vec<Vec2> = p.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
            Ring::from_mm(&pts).ok()
        })
        .collect();
    fit_region(&Region::from_rings_nonzero(&rings), POCKET_FIT_TOL_MM)
}

/// Лист ядра с раскладкой; `label` — подпись детали.
fn sheet_of(parts: &[Part], lay: &Layout, label: impl Fn(&RibGeometry) -> String) -> Sheet {
    let pockets: Vec<Vec<Path>> = parts.iter().map(|p| pocket_paths(p.pockets)).collect();
    let mut sheet = Sheet::new();
    for &(k, dx, dy) in &lay.placements {
        let g = parts[k].geometry;
        let d = Vec2::new(dx, dy);
        let outline: Vec<Vec2> = g.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        sheet.add_path("CONTOUR", Path::polygon(&outline).translated(d));
        for h in &g.holes {
            sheet.add_path("HOLES", h.path().translated(d));
        }
        for p in &pockets[k] {
            sheet.add_path("POCKETS", p.translated(d));
        }
        // Подпись — в зазоре под деталью, чтобы не пересекать контуры.
        let ymin = g.outline.iter().map(|q| q.1).fold(f64::MAX, f64::min);
        sheet.add_text(
            "LABELS",
            Vec2::new(dx + 2.0, dy + ymin - 3.0),
            2.0,
            &label(g),
        );
    }
    sheet
}

/// DXF R12. Возвращает текст файла и предупреждения (не помещается на поле станка).
pub fn to_dxf(parts: &[Part], sheet: (f64, f64)) -> (String, Vec<String>) {
    let lay = layout(parts, sheet, 5.0);
    let mut warnings = Vec::new();
    if lay.size.0 > sheet.0 + 1e-9 || lay.size.1 > sheet.1 + 1e-9 {
        warnings.push(format!(
            "Раскладка {:.0}×{:.0} мм больше рабочего поля {:.0}×{:.0} мм — разделите на несколько листов.",
            lay.size.0, lay.size.1, sheet.0, sheet.1
        ));
    }
    // В DXF R12 подписи латиницей: не все станки понимают кириллицу.
    let s = sheet_of(parts, &lay, RibGeometry::ascii_label);
    (s.to_dxf(), warnings)
}

/// SVG в мм (открывается без масштабирования программами для лазера и плоттера).
pub fn to_svg(parts: &[Part], sheet: (f64, f64)) -> String {
    let lay = layout(parts, sheet, 5.0);
    sheet_of(parts, &lay, |g| {
        format!("Нервюра {} · z = {:.0} мм", g.index + 1, g.z)
    })
    .to_svg()
}
