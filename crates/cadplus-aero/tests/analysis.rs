//! Проверки расчёта крыла на задачах с известным ответом.

use cadplus_aero::analysis::{Distribution, G, Input, LoadCase, analyze};
use cadplus_aero::material::{CARBON_TUBE, default_library};
use cadplus_aero::{Section, Spar, Status, Wing, preset};
use std::f64::consts::PI;

fn rel(a: f64, b: f64) -> f64 {
    ((a - b) / b).abs()
}

fn rect_wing() -> Wing {
    Wing {
        span_mm: 1000.0,
        root_chord_mm: 200.0,
        tip_chord_mm: 200.0,
        dihedral_deg: 0.0,
    }
}

fn tube(z0: f64, z1: f64) -> Spar {
    Spar {
        name: "трубка".into(),
        section: Section::Tube {
            d_out: 10.0,
            wall: 1.0,
        },
        material: CARBON_TUBE.into(),
        z_start: z0,
        z_end: z1,
        chord_pos: 0.3,
        joiner: false,
        status: Status::Approved,
    }
}

fn load(d: Distribution) -> LoadCase {
    LoadCase {
        mass_kg: 1.0,
        load_factor: 1.0,
        safety_factor: 1.0,
        distribution: d,
    }
}

fn run(wing: &Wing, lc: &LoadCase, spars: &[Spar]) -> cadplus_aero::Analysis {
    let lib = default_library();
    let ribs = preset::ribs();
    analyze(&Input {
        wing,
        load: lc,
        spars,
        materials: &lib,
        ribs: &ribs,
        thickness_table: &[],
    })
    .unwrap()
}

#[test]
fn user_wing_area_and_mac() {
    let w = preset::wing();
    // Сводка: 0,19 м² и САХ ≈ 192 мм.
    assert!((w.area_mm2() - 190_000.0).abs() < 1e-6);
    assert!((w.mac_mm() - 191.58).abs() < 0.01, "САХ = {}", w.mac_mm());
}

#[test]
fn uniform_cantilever_matches_formulas() {
    // Равномерная нагрузка на консоль длиной s: M₀ = q s²/2, прогиб конца = q s⁴ / (8 EI).
    let w = rect_wing();
    let lc = load(Distribution::Uniform);
    let a = run(&w, &lc, &[tube(0.0, 500.0)]);
    let s = 500.0;
    let q = lc.design_lift_n() / 2.0 / s;
    assert!(rel(a.root_shear_n, q * s) < 1e-9);
    assert!(rel(a.root_moment_nmm, q * s * s / 2.0) < 1e-5);
    let ei = 70_000.0
        * Section::Tube {
            d_out: 10.0,
            wall: 1.0,
        }
        .inertia();
    let expect = q * s.powi(4) / (8.0 * ei);
    assert!(
        rel(a.tip_deflection_mm.unwrap(), expect) < 1e-4,
        "прогиб {} против {expect}",
        a.tip_deflection_mm.unwrap()
    );
}

#[test]
fn elliptic_root_moment() {
    // Эллиптическая нагрузка: M₀ = 2 L s / (3π) для консоли длиной s, L — подъёмная сила крыла.
    let w = rect_wing();
    let lc = load(Distribution::Elliptic);
    let a = run(&w, &lc, &[tube(0.0, 500.0)]);
    let l = lc.design_lift_n();
    assert!((l - G).abs() < 1e-12);
    let expect = 2.0 * l * 500.0 / (3.0 * PI);
    assert!(
        rel(a.root_moment_nmm, expect) < 1e-3,
        "M₀ = {} против {expect}",
        a.root_moment_nmm
    );
}

#[test]
fn every_distribution_carries_half_lift() {
    let w = preset::wing();
    for d in [
        Distribution::Schrenk,
        Distribution::Elliptic,
        Distribution::Trapezoidal,
        Distribution::Uniform,
    ] {
        let lc = load(d);
        let a = run(&w, &lc, &[tube(0.0, 500.0)]);
        assert!(
            rel(a.root_shear_n, lc.design_lift_n() / 2.0) < 1e-9,
            "{d:?}"
        );
        let ribs: f64 = a.ribs.iter().map(|r| r.force_n).sum();
        assert!(
            rel(ribs, a.half_lift_n) < 1e-6,
            "{d:?}: сумма по нервюрам {ribs}"
        );
    }
}

#[test]
fn rib_strips_for_uniform_load() {
    // Шаг 100 мм на консоли 500 мм: 6 нервюр, крайние собирают по половине полосы.
    let w = rect_wing();
    let lc = load(Distribution::Uniform);
    let a = run(&w, &lc, &[tube(0.0, 500.0)]);
    let z: Vec<f64> = a.ribs.iter().map(|r| r.z).collect();
    assert_eq!(z, vec![0.0, 100.0, 200.0, 300.0, 400.0, 500.0]);
    let q = a.half_lift_n / 500.0;
    let expect = [50.0, 100.0, 100.0, 100.0, 100.0, 50.0];
    for (r, e) in a.ribs.iter().zip(expect) {
        assert!(rel(r.force_n, q * e) < 1e-6);
    }
}

#[test]
fn two_equal_spars_share_moment() {
    let w = rect_wing();
    let lc = load(Distribution::Schrenk);
    let one = run(&w, &lc, &[tube(0.0, 500.0)]);
    let two = run(&w, &lc, &[tube(0.0, 500.0), tube(0.0, 500.0)]);
    assert!(
        rel(
            two.spars[0].max_stress_mpa,
            one.spars[0].max_stress_mpa / 2.0
        ) < 1e-9
    );
    assert!(
        rel(
            two.tip_deflection_mm.unwrap(),
            one.tip_deflection_mm.unwrap() / 2.0
        ) < 1e-9
    );
}

#[test]
fn gap_without_spar_is_reported() {
    let w = preset::wing();
    let lc = preset::load_case();
    let a = run(&w, &lc, &[tube(0.0, 250.0)]);
    assert!(a.tip_deflection_mm.is_none());
    assert!(
        a.warnings.iter().any(|m| m.contains("некому воспринять")),
        "{:?}",
        a.warnings
    );
}

#[test]
fn joiner_bearing_from_couple() {
    let w = preset::wing();
    let lc = preset::load_case();
    let spars = preset::spars();
    let a = run(&w, &lc, &spars);
    let j = &a.joiners[0];
    assert_eq!(j.ribs_on_tube, vec![0.0, 100.0, 200.0]);
    assert!((j.arm_mm - 200.0).abs() < 1e-9);
    assert!(rel(j.bearing_force_n, a.root_moment_nmm / 200.0) < 1e-9);
    assert!(rel(j.bearing_stress_mpa, j.bearing_force_n / (10.0 * 2.0)) < 1e-12);
    // Внешний лонжерон-кандидат стыкуется встык: перекрытия нет.
    assert!(j.overlap_mm.abs() < 1e-9);
}

#[test]
fn user_plane_numbers_are_sane() {
    let w = preset::wing();
    let lc = preset::load_case();
    let a = run(&w, &lc, &preset::spars());
    // L = 4 · 1,5 · 0,7 · 9,81 = 41,2 Н на крыло.
    assert!((a.design_lift_n - 41.202).abs() < 1e-3);
    // Центр давления консоли — между 40 и 45% полуразмаха.
    let arm = a.root_moment_nmm / a.root_shear_n;
    assert!((200.0..225.0).contains(&arm), "плечо {arm}");
    assert!(a.tip_deflection_mm.unwrap() > 0.0);
}
