//! Все нервюры стартового проекта: `cargo run --release -p cadplus-rib --example ribs [папка]`.
//! Пишет ribs.dxf и ribs.svg, печатает массы до и после.

use cadplus_aero::{Input, analyze, material, preset};
use cadplus_rib::{
    RibSettings, airfoil::Airfoil, export, finalize, grid_params, optimize, prepare_all, setup,
};
use cadplus_topopt::Control;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "export".into());
    std::fs::create_dir_all(&out).expect("папка для экспорта");
    let (wing, load, spars, ribs) = (
        preset::wing(),
        preset::load_case(),
        preset::spars(),
        preset::ribs(),
    );
    let lib = material::default_library();
    let af = Airfoil::clark_y();
    let table = af.thickness_table();
    let a = analyze(&Input {
        wing: &wing,
        load: &load,
        spars: &spars,
        materials: &lib,
        ribs: &ribs,
        thickness_table: &table,
    })
    .expect("расчёт крыла");
    let mat = material::find(&lib, &ribs.material).expect("материал нервюр");
    let settings = RibSettings::default();
    let gp = grid_params(&settings, mat.e_mpa, mat.poisson, ribs.thickness_mm);
    let mut results = Vec::new();
    let (mut before, mut after) = (0.0, 0.0);
    for (g, warns) in prepare_all(&af, &a, &spars, &settings, mat.print_tolerance_mm) {
        for w in warns {
            println!("! {w}");
        }
        let t = std::time::Instant::now();
        let s = setup::build(&g, &gp).expect("задача нервюры");
        let sol = optimize(&s, &settings, |_, _| Control::Continue).expect("оптимизация");
        let r = finalize(
            &s,
            &sol.density,
            Some(&sol),
            &settings,
            ribs.thickness_mm,
            mat.density_g_cm3,
        );
        println!(
            "Нервюра {} (z = {:.0}, хорда {:.0} мм, {:.2} Н): сетка {}×{}, {} итераций за {:.1} с; масса {:.2} → {:.2} г, вырезов {}",
            g.index + 1,
            g.z,
            g.chord,
            g.force_n,
            s.grid.nelx,
            s.grid.nely,
            r.iterations,
            t.elapsed().as_secs_f64(),
            r.mass_before_g,
            r.mass_after_g,
            r.pockets.len()
        );
        for w in &r.warnings {
            println!("  ! {w}");
        }
        before += r.mass_before_g * g.quantity as f64;
        after += r.mass_after_g * g.quantity as f64;
        results.push(r);
    }
    println!("Все нервюры самолёта: {before:.1} → {after:.1} г");
    let parts: Vec<export::Part> = results
        .iter()
        .map(|r| export::Part {
            geometry: &r.geometry,
            pockets: &r.pockets,
        })
        .collect();
    let (dxf, warns) = export::to_dxf(&parts, settings.manufacturing.field_mm());
    for w in warns {
        println!("! {w}");
    }
    std::fs::write(format!("{out}/ribs.dxf"), dxf).expect("запись DXF");
    std::fs::write(
        format!("{out}/ribs.svg"),
        export::to_svg(&parts, settings.manufacturing.field_mm()),
    )
    .expect("запись SVG");
    println!("Записано: {out}/ribs.dxf, {out}/ribs.svg");
}
