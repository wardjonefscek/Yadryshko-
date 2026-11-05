//! Сверка с эталоном (NFR-09): MBB-балка top88 с фильтром плотности.
//!
//! Эталонные числа получены независимой реализацией top88 (Andreassen et al., 2011,
//! ft = 2, прямой разреженный решатель) вне проекта.

use cadplus_topopt::{Cell, Control, Error, Problem, Settings, solve};

fn rel(a: f64, b: f64) -> f64 {
    ((a - b) / b).abs()
}

fn run(nelx: usize, nely: usize, volfrac: f64) -> cadplus_topopt::Solution {
    let p = Problem::mbb(nelx, nely);
    let s = Settings {
        volfrac,
        max_iter: 1000,
        ..Settings::default()
    };
    solve(&p, &s, |_, _| Control::Continue).unwrap()
}

#[test]
fn mbb_60x20_matches_top88() {
    let sol = run(60, 20, 0.5);
    let first = sol.history[0].compliance;
    assert!(
        rel(first, 1007.022100716524) < 1e-9,
        "первая итерация: {first}"
    );
    assert!(
        rel(sol.compliance, 218.803221073103) < 1e-3,
        "итог: {}",
        sol.compliance
    );
    assert!(
        sol.iterations.abs_diff(127) <= 2,
        "итераций: {}",
        sol.iterations
    );
    assert!(sol.converged);
    let vol = sol.density.iter().sum::<f64>() / sol.density.len() as f64;
    assert!((vol - 0.5).abs() < 1e-3);
}

#[test]
fn mbb_30x10_matches_top88() {
    let sol = run(30, 10, 0.4);
    assert!(rel(sol.history[0].compliance, 1922.958583981776) < 1e-9);
    assert!(
        rel(sol.compliance, 359.873178284406) < 1e-3,
        "итог: {}",
        sol.compliance
    );
    assert!(
        sol.iterations.abs_diff(86) <= 2,
        "итераций: {}",
        sol.iterations
    );
}

#[test]
fn stiffness_scales_with_modulus_and_thickness() {
    // Податливость первой итерации обратно пропорциональна E·t.
    let mut p = Problem::mbb(20, 10);
    let s = Settings {
        max_iter: 1,
        ..Settings::default()
    };
    let c1 = solve(&p, &s, |_, _| Control::Continue).unwrap().compliance;
    p.e0 = 2000.0;
    p.thickness = 3.0;
    let c2 = solve(&p, &s, |_, _| Control::Continue).unwrap().compliance;
    assert!(rel(c2, c1 / 6000.0) < 1e-9);
}

#[test]
fn passive_cells_keep_their_values() {
    let mut p = Problem::mbb(30, 10);
    for i in 10..14 {
        for j in 3..7 {
            let e = p.elem(i, j);
            p.cells[e] = Cell::Void;
        }
    }
    for i in 0..30 {
        let e = p.elem(i, 0);
        p.cells[e] = Cell::Solid;
    }
    let s = Settings {
        volfrac: 0.4,
        ..Settings::default()
    };
    let sol = solve(&p, &s, |_, _| Control::Continue).unwrap();
    for (e, c) in p.cells.iter().enumerate() {
        match c {
            Cell::Solid => assert_eq!(sol.density[e], 1.0),
            Cell::Void => assert_eq!(sol.density[e], 0.0),
            Cell::Active => assert!((0.0..=1.0).contains(&sol.density[e])),
        }
    }
    // Объём считается только по оптимизируемым элементам.
    let act: Vec<f64> = p
        .cells
        .iter()
        .zip(&sol.density)
        .filter(|(c, _)| **c == Cell::Active)
        .map(|(_, d)| *d)
        .collect();
    let vol = act.iter().sum::<f64>() / act.len() as f64;
    assert!((vol - 0.4).abs() < 2e-3, "доля объёма {vol}");
}

#[test]
fn cancel_stops_quickly() {
    let p = Problem::mbb(60, 20);
    let sol = solve(&p, &Settings::default(), |it, _| {
        if it.iter >= 3 {
            Control::Stop
        } else {
            Control::Continue
        }
    })
    .unwrap();
    assert_eq!(sol.iterations, 3);
    assert!(sol.cancelled && !sol.converged);
}

#[test]
fn unsupported_part_is_singular() {
    let mut p = Problem::mbb(10, 5);
    p.fixed.truncate(1); // одно закрепление по x — тело может двигаться по y
    let r = solve(&p, &Settings::default(), |_, _| Control::Continue);
    assert_eq!(r.unwrap_err(), Error::Singular);
}
