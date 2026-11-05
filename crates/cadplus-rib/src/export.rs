//! Экспорт нервюр: DXF R12 для ЧПУ и SVG для просмотра.
//!
//! Слои DXF: `CONTOUR` — наружный контур, `HOLES` — отверстия под лонжероны (окружности),
//! `POCKETS` — облегчающие вырезы, `LABELS` — подписи (в CAM этот слой не резать).
//! Детали раскладываются столбцом с зазором и проверяются на размер рабочего поля станка.

use crate::geom::{HoleShape, Pt, RibGeometry};

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
    let mut s = String::new();
    s.push_str(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSBASE\n10\n0.0\n20\n0.0\n30\n0.0\n",
    );
    s.push_str(&format!(
        "9\n$EXTMIN\n10\n0.0\n20\n0.0\n30\n0.0\n9\n$EXTMAX\n10\n{:.3}\n20\n{:.3}\n30\n0.0\n0\nENDSEC\n",
        lay.size.0, lay.size.1
    ));
    s.push_str("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n4\n");
    for (name, color) in [("CONTOUR", 7), ("HOLES", 1), ("POCKETS", 5), ("LABELS", 3)] {
        s.push_str(&format!(
            "0\nLAYER\n2\n{name}\n70\n0\n62\n{color}\n6\nCONTINUOUS\n"
        ));
    }
    s.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    for &(k, dx, dy) in &lay.placements {
        let p = parts[k];
        let g = p.geometry;
        polyline(&mut s, "CONTOUR", &g.outline, dx, dy);
        for h in &g.holes {
            match h.shape {
                HoleShape::Circle { d } => s.push_str(&format!(
                    "0\nCIRCLE\n8\nHOLES\n10\n{:.4}\n20\n{:.4}\n30\n0.0\n40\n{:.4}\n",
                    h.center.0 + dx,
                    h.center.1 + dy,
                    d / 2.0
                )),
                HoleShape::Rect { w, h: hh } => {
                    let (cx, cy) = h.center;
                    let r = [
                        (cx - w / 2.0, cy - hh / 2.0),
                        (cx + w / 2.0, cy - hh / 2.0),
                        (cx + w / 2.0, cy + hh / 2.0),
                        (cx - w / 2.0, cy + hh / 2.0),
                    ];
                    polyline(&mut s, "HOLES", &r, dx, dy);
                }
            }
        }
        for pk in p.pockets {
            polyline(&mut s, "POCKETS", pk, dx, dy);
        }
        // Подпись — в зазоре под деталью, чтобы не пересекать контуры.
        let ymin = g.outline.iter().map(|q| q.1).fold(f64::MAX, f64::min);
        s.push_str(&format!(
            "0\nTEXT\n8\nLABELS\n10\n{:.3}\n20\n{:.3}\n30\n0.0\n40\n2.0\n1\n{}\n",
            dx + 2.0,
            dy + ymin - 3.0,
            g.ascii_label()
        ));
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    (s, warnings)
}

fn polyline(s: &mut String, layer: &str, pts: &[Pt], dx: f64, dy: f64) {
    s.push_str(&format!(
        "0\nPOLYLINE\n8\n{layer}\n66\n1\n10\n0.0\n20\n0.0\n30\n0.0\n70\n1\n"
    ));
    for &(x, y) in pts {
        s.push_str(&format!(
            "0\nVERTEX\n8\n{layer}\n10\n{:.4}\n20\n{:.4}\n30\n0.0\n",
            x + dx,
            y + dy
        ));
    }
    s.push_str(&format!("0\nSEQEND\n8\n{layer}\n"));
}

/// SVG для просмотра (ось y вверх, как в CAD).
pub fn to_svg(parts: &[Part], sheet: (f64, f64)) -> String {
    let lay = layout(parts, sheet, 5.0);
    let (w, h) = lay.size;
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w:.2} {h:.2}\" width=\"{:.0}mm\" height=\"{:.0}mm\">\n<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n<g transform=\"translate(0 {h:.2}) scale(1 -1)\" fill=\"none\" stroke-width=\"0.3\">\n",
        w, h
    );
    let path = |pts: &[Pt], dx: f64, dy: f64| -> String {
        let mut d = String::new();
        for (i, &(x, y)) in pts.iter().enumerate() {
            d.push_str(&format!(
                "{}{:.3} {:.3} ",
                if i == 0 { "M" } else { "L" },
                x + dx,
                y + dy
            ));
        }
        d.push('Z');
        d
    };
    let mut labels = String::new();
    for &(k, dx, dy) in &lay.placements {
        let g = parts[k].geometry;
        s.push_str(&format!(
            "<path d=\"{}\" stroke=\"#202020\"/>\n",
            path(&g.outline, dx, dy)
        ));
        for hl in &g.holes {
            match hl.shape {
                HoleShape::Circle { d } => s.push_str(&format!(
                    "<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" stroke=\"#c0392b\"/>\n",
                    hl.center.0 + dx,
                    hl.center.1 + dy,
                    d / 2.0
                )),
                HoleShape::Rect { w: rw, h: rh } => s.push_str(&format!(
                    "<rect x=\"{:.3}\" y=\"{:.3}\" width=\"{rw:.3}\" height=\"{rh:.3}\" stroke=\"#c0392b\"/>\n",
                    hl.center.0 - rw / 2.0 + dx,
                    hl.center.1 - rh / 2.0 + dy
                )),
            }
        }
        for pk in parts[k].pockets {
            s.push_str(&format!(
                "<path d=\"{}\" stroke=\"#1f5fbf\"/>\n",
                path(pk, dx, dy)
            ));
        }
        let ymin = g.outline.iter().map(|q| q.1).fold(f64::MAX, f64::min);
        labels.push_str(&format!(
            "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"2.6\" font-family=\"sans-serif\" fill=\"#555\">Нервюра {} · z = {:.0} мм</text>\n",
            dx + 2.0,
            h - (dy + ymin) + 3.0,
            g.index + 1,
            g.z
        ));
    }
    s.push_str("</g>\n");
    s.push_str(&labels);
    s.push_str("</svg>\n");
    s
}
