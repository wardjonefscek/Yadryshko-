//! MBB-балка top88: `cargo run --release -p cadplus-topopt --example mbb [файл.pgm]`.

use cadplus_topopt::{Control, Problem, Settings, solve};

fn main() {
    let p = Problem::mbb(60, 20);
    let s = Settings {
        volfrac: 0.5,
        max_iter: 1000,
        ..Settings::default()
    };
    let t = std::time::Instant::now();
    let sol = solve(&p, &s, |_, _| Control::Continue).expect("задача корректна");
    println!(
        "MBB 60×20: первая итерация c = {:.6}, итог c = {:.6}, итераций {}, {:.2} с",
        sol.history[0].compliance,
        sol.compliance,
        sol.iterations,
        t.elapsed().as_secs_f64()
    );
    if let Some(path) = std::env::args().nth(1) {
        let mut out = format!("P2\n{} {}\n255\n", p.nelx, p.nely);
        for j in 0..p.nely {
            for i in 0..p.nelx {
                let v = 255.0 * (1.0 - sol.density[p.elem(i, j)]);
                out.push_str(&format!("{} ", v.round() as u8));
            }
            out.push('\n');
        }
        std::fs::write(&path, out).expect("файл записан");
    }
}
