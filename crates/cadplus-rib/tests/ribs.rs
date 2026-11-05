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
