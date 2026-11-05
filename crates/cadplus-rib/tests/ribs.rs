//! Проверки нервюр: постановка задачи, запретные зоны, технология, экспорт.

use cadplus_aero::{Analysis, Input, analyze, material, preset};
use cadplus_rib::geom::{HoleShape, distance_to_polygon, point_in_polygon};
use cadplus_rib::{
    Manufacturing, RibSettings, airfoil::Airfoil, export, finalize, grid_params, optimize,
    prepare_all, setup,
};
use cadplus_topopt::Control;

fn analysis() -> (Analysis, Vec<cadplus_aero::Spar>) {
    let (wing, load, spars, ribs) = (
        preset::wing(),
        preset::load_case(),
        preset::spars(),
        preset::ribs(),
    );
    let lib = material::default_library();
    let table = Airfoil::clark_y().thickness_table();
    let a = analyze(&Input {
        wing: &wing,
        load: &load,
        spars: &spars,
        materials: &lib,
        ribs: &ribs,
        thickness_table: &table,
    })
    .unwrap();
    (a, spars)
}

fn quick() -> RibSettings {
    RibSettings {
        element_mm: 1.0,
        max_iter: 40,
        ..RibSettings::default()
    }
}

#[test]
fn every_station_has_a_support_hole() {
    let (a, spars) = analysis();
    let ribs = prepare_all(
        &Airfoil::clark_y(),
        &a,
        &spars,
        &RibSettings::default(),
        0.0,
    );
    assert_eq!(ribs.len(), 6);
    for (g, warns) in &ribs {
        assert_eq!(g.holes.len(), 1, "нервюра {}: {:?}", g.index + 1, warns);
        // Ø10 + посадка 0,1 у трубки, Ø6 + 0,1 у внешнего лонжерона.
        let HoleShape::Circle { d } = g.holes[0].shape else {
            panic!()
        };
        let expect = if g.z <= 250.0 { 10.1 } else { 6.1 };
        assert!((d - expect).abs() < 1e-9);
    }
}

#[test]
fn loads_and_supports_are_consistent() {
    let (a, spars) = analysis();
    let s = quick();
    let (g, _) = prepare_all(&Airfoil::clark_y(), &a, &spars, &s, 0.0).remove(2);
    let st = setup::build(&g, &grid_params(&s, 20_000.0, 0.15, 2.0)).unwrap();
    let total: f64 = st.problem.loads.iter().map(|l| l.1).sum();
    assert!(
        (total - g.force_n).abs() < 1e-9,
        "сила {total} против {}",
        g.force_n
    );
    // Все нагрузки — вертикальные (нечётные степени свободы), закрепления есть.
    assert!(st.problem.loads.iter().all(|l| l.0 % 2 == 1));
    assert!(!st.problem.fixed.is_empty());
}

#[test]
fn pockets_respect_edge_band_and_rings() {
    let (a, spars) = analysis();
    let s = quick();
    let (g, _) = prepare_all(&Airfoil::clark_y(), &a, &spars, &s, 0.0).remove(1);
    let st = setup::build(&g, &grid_params(&s, 20_000.0, 0.15, 2.0)).unwrap();
    let sol = optimize(&st, &s, |_, _| Control::Continue).unwrap();
    let r = finalize(&st, &sol.density, Some(&sol), &s, 2.0, 1.8);
    assert!(!r.pockets.is_empty());
    assert!(r.mass_after_g < r.mass_before_g);
    let tol = s.element_mm; // дискретизация сетки
    for p in &r.pockets {
        for &q in p {
            assert!(point_in_polygon(q, &g.outline));
            assert!(
                distance_to_polygon(q, &g.outline) >= s.edge_band_mm - tol,
                "вырез залез в кромку"
            );
            for h in &g.holes {
                assert!(
                    h.distance(q) >= s.ring_mm - tol,
                    "вырез залез в кольцо вокруг отверстия"
                );
            }
        }
    }
}

#[test]
fn fdm_adds_print_tolerance_to_holes() {
    let (a, spars) = analysis();
    let s = RibSettings {
        manufacturing: Manufacturing::Fdm {
            nozzle_mm: 0.4,
            bed_mm: (256.0, 256.0),
        },
        ..RibSettings::default()
    };
    let (g, _) = prepare_all(&Airfoil::clark_y(), &a, &spars, &s, 0.2).remove(0);
    let HoleShape::Circle { d } = g.holes[0].shape else {
        panic!()
    };
    assert!((d - (10.0 + 0.1 + 0.2)).abs() < 1e-9);
}

#[test]
fn station_without_spar_cannot_be_set_up() {
    let (a, mut spars) = analysis();
    spars.truncate(1); // только трубка до 250 мм
    let s = quick();
    let (g, warns) = prepare_all(&Airfoil::clark_y(), &a, &spars, &s, 0.0).remove(5);
    assert!(warns.iter().any(|w| w.contains("не к чему закрепить")));
    assert!(setup::build(&g, &grid_params(&s, 20_000.0, 0.15, 2.0)).is_err());
}

#[test]
fn dxf_has_every_part_twice_and_fits_the_sheet() {
    let (a, spars) = analysis();
    let s = RibSettings::default();
    let ribs = prepare_all(&Airfoil::clark_y(), &a, &spars, &s, 0.0);
    let parts: Vec<export::Part> = ribs
        .iter()
        .map(|(g, _)| export::Part {
            geometry: g,
            pockets: &[],
        })
        .collect();
    let (dxf, warns) = export::to_dxf(&parts, (600.0, 600.0));
    assert!(warns.is_empty(), "{warns:?}");
    assert!(dxf.contains("AC1009"));
    assert_eq!(dxf.matches("POLYLINE\n8\nCONTOUR").count(), 12);
    assert_eq!(dxf.matches("CIRCLE\n8\nHOLES").count(), 12);
    let (_, small) = export::to_dxf(&parts, (200.0, 200.0));
    assert!(
        !small.is_empty(),
        "на поле 200×200 нервюры 220 мм не помещаются"
    );
}

/// Плотность «как после оптимизации»: материал везде, кроме полосы высотой `w` мм
/// посередине профиля между x = 60 и 140 мм.
fn slot_density(st: &setup::RibSetup, w: f64) -> Vec<f64> {
    let (nelx, nely) = (st.grid.nelx, st.grid.nely);
    let g = &st.geometry;
    let mut d = vec![1.0; nelx * nely];
    for i in 0..nelx {
        for j in 0..nely {
            let (x, y) = st.grid.center(i, j);
            let mid = 0.5 * (g.upper_at(x) + g.lower_at(x));
            if (60.0..140.0).contains(&x) && (y - mid).abs() < w / 2.0 {
                d[i * nely + j] = 0.0;
            }
        }
    }
    d
}

fn region_of(p: &[Vec<(f64, f64)>]) -> yadro_core::Region {
    let rings: Vec<yadro_core::Ring> = p
        .iter()
        .map(|r| {
            let pts: Vec<yadro_core::Vec2> = r
                .iter()
                .map(|&(x, y)| yadro_core::Vec2::new(x, y))
                .collect();
            yadro_core::Ring::from_mm(&pts).unwrap()
        })
        .collect();
    yadro_core::Region::from_rings_nonzero(&rings)
}

#[test]
fn cutter_decides_what_is_cut() {
    let (a, spars) = analysis();
    let s = RibSettings {
        element_mm: 0.5,
        ..RibSettings::default()
    };
    let (g, _) = prepare_all(&Airfoil::clark_y(), &a, &spars, &s, 0.0).remove(0);
    let st = setup::build(&g, &grid_params(&s, 20_000.0, 0.15, 2.0)).unwrap();
    // Щель 1,5 мм уже фрезы Ø2 — закрыта, с предупреждением.
    let narrow = finalize(&st, &slot_density(&st, 1.5), None, &s, 2.0, 1.8);
    assert!(narrow.pockets.is_empty(), "{:?}", narrow.pockets.len());
    assert!(
        narrow
            .warnings
            .iter()
            .any(|w| w.contains("не проходит инструмент"))
    );
    // Сопло 0,4 мм при печати такую щель делает.
    let fdm = RibSettings {
        manufacturing: Manufacturing::Fdm {
            nozzle_mm: 0.4,
            bed_mm: (256.0, 256.0),
        },
        ..s.clone()
    };
    let printed = finalize(&st, &slot_density(&st, 1.5), None, &fdm, 2.0, 1.8);
    assert!(!printed.pockets.is_empty());
    // Широкий вырез 6 мм фреза делает; углы — со скруглением не меньше радиуса фрезы:
    // вырез совпадает со своим «открытием» кругом фрезы.
    let wide = finalize(&st, &slot_density(&st, 6.0), None, &s, 2.0, 1.8);
    assert_eq!(wide.pockets.len(), 1);
    let p = region_of(&wide.pockets);
    let again = p.opening(1.0).unwrap();
    let per: f64 = wide.pockets[0]
        .iter()
        .zip(wide.pockets[0].iter().cycle().skip(1))
        .map(|(a, b)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt())
        .sum();
    assert!(
        p.xor(&again).area_mm2() < per * 0.01,
        "фреза не проходит везде"
    );
    // Масса сходится точно: после = до − вырезы.
    assert!((wide.area_after_mm2 - (wide.area_before_mm2 - p.area_mm2())).abs() < 1e-6);
}

#[test]
fn dxf_reads_back_with_arcs() {
    let (a, spars) = analysis();
    let s = quick();
    let ribs = prepare_all(&Airfoil::clark_y(), &a, &spars, &s, 0.0);
    let (g, _) = &ribs[1];
    let st = setup::build(g, &grid_params(&s, 20_000.0, 0.15, 2.0)).unwrap();
    let r = finalize(&st, &slot_density(&st, 6.0), None, &s, 2.0, 1.8);
    let parts = vec![export::Part {
        geometry: g,
        pockets: &r.pockets,
    }];
    let (dxf, _) = export::to_dxf(&parts, (600.0, 600.0));
    let sheet = yadro_core::Sheet::from_dxf(&dxf).expect("ядро читает свой DXF");
    let count = |layer: &str| sheet.items().iter().filter(|i| i.layer == layer).count();
    assert_eq!(
        (count("CONTOUR"), count("HOLES"), count("POCKETS")),
        (2, 2, 2)
    );
    // Скругления выреза — дугами, а не сотнями отрезков.
    let pocket = &sheet
        .items()
        .iter()
        .find(|i| i.layer == "POCKETS")
        .unwrap()
        .path;
    assert!(pocket.vertices().iter().any(|v| v.bulge != 0.0));
    assert!(
        pocket.len() < r.pockets[0].len() / 3,
        "{} участков",
        pocket.len()
    );
    // Подписи — латиницей, в DXF есть.
    assert!(dxf.contains("TEXT\n8\nLABELS"));
    let svg = export::to_svg(&parts, (600.0, 600.0));
    assert!(svg.contains("mm\" height=") && svg.contains("Нервюра 2"));
}
