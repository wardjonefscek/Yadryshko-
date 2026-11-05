//! Краткий отчёт по стартовому проекту: `cargo run -p cadplus-aero --example report`.

use cadplus_aero::{Input, analyze, material::default_library, preset};

fn main() {
    let (wing, load, spars, ribs) = (
        preset::wing(),
        preset::load_case(),
        preset::spars(),
        preset::ribs(),
    );
    let lib = default_library();
    let a = analyze(&Input {
        wing: &wing,
        load: &load,
        spars: &spars,
        materials: &lib,
        ribs: &ribs,
        thickness_table: &[],
    })
    .expect("исходные данные корректны");
    println!(
        "Площадь {:.1} дм², САХ {:.1} мм, нагрузка на крыло {:.1} г/дм²",
        a.area_dm2, a.mac_mm, a.wing_loading_g_dm2
    );
    println!(
        "Расчётная подъёмная сила {:.1} Н (n = {}, f = {}), на консоль {:.1} Н",
        a.design_lift_n, load.load_factor, load.safety_factor, a.half_lift_n
    );
    println!(
        "У корня: Q = {:.1} Н, M = {:.2} Н·м",
        a.root_shear_n,
        a.root_moment_nmm / 1000.0
    );
    for s in &a.spars {
        println!(
            "{}: σmax = {:.0} МПа на z = {:.0} мм, запас {:.1}, масса {:.1} г",
            s.name, s.max_stress_mpa, s.z_at_max, s.margin, s.mass_g
        );
    }
    match a.tip_deflection_mm {
        Some(v) => println!("Прогиб конца консоли {v:.1} мм"),
        None => println!("Прогиб конца не определён: есть участок без лонжерона"),
    }
    for j in &a.joiners {
        println!(
            "{}: нервюры на трубке {:?}, плечо {:.0} мм, сила на отверстие {:.1} Н, смятие {:.2} МПа, запас {:.0}; момент в конце трубки {:.2} Н·м, перекрытие {:.0} мм",
            j.name,
            j.ribs_on_tube,
            j.arm_mm,
            j.bearing_force_n,
            j.bearing_stress_mpa,
            j.bearing_margin,
            j.moment_at_end / 1000.0,
            j.overlap_mm
        );
    }
    for r in &a.ribs {
        println!(
            "Нервюра {} (z = {:.0} мм, хорда {:.0} мм): {:.2} Н",
            r.index + 1,
            r.z,
            r.chord,
            r.force_n
        );
    }
    for w in &a.warnings {
        println!("! {w}");
    }
}
