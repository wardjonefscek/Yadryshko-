//! Исходные данные самолёта из сводки «Параметры радиоуправляемого самолёта» (05.11.2026).
//! Это стартовый проект, а не зашитые в код константы: всё меняется в окне программы.

use crate::analysis::{Distribution, LoadCase, RibLayout};
use crate::material::{CARBON_TUBE, STEF};
use crate::section::Section;
use crate::wing::{Spar, Status, Wing};

/// Крыло: трапеция 1000 мм, хорды 220/160, Clark Y, V 2° на консоль.
pub fn wing() -> Wing {
    Wing {
        span_mm: 1000.0,
        root_chord_mm: 220.0,
        tip_chord_mm: 160.0,
        dihedral_deg: 2.0,
    }
}

/// Расчётный случай. Масса 0,7 кг — оценка с запасом: известно только оборудование (224,7 г).
pub fn load_case() -> LoadCase {
    LoadCase {
        mass_kg: 0.7,
        load_factor: 4.0,
        safety_factor: 1.5,
        distribution: Distribution::Schrenk,
    }
}

/// Лонжероны одной консоли.
///
/// Трубка Ø10×1 длиной 500 мм проходит через обе консоли — по 250 мм в каждую.
/// Внешний лонжерон — кандидат для расчёта: в сводке его схема и сечение не утверждены.
pub fn spars() -> Vec<Spar> {
    vec![
        Spar {
            name: "Соединительная трубка".into(),
            section: Section::Tube {
                d_out: 10.0,
                wall: 1.0,
            },
            material: CARBON_TUBE.into(),
            z_start: 0.0,
            z_end: 250.0,
            chord_pos: 0.30,
            joiner: true,
            status: Status::Approved,
        },
        Spar {
            name: "Внешний лонжерон (кандидат)".into(),
            section: Section::Tube {
                d_out: 6.0,
                wall: 1.0,
            },
            material: CARBON_TUBE.into(),
            z_start: 250.0,
            z_end: 500.0,
            chord_pos: 0.30,
            joiner: false,
            status: Status::Candidate,
        },
    ]
}

/// Нервюры: шаг 100 мм (6 станций на консоль), СТЭФ 2 мм.
pub fn ribs() -> RibLayout {
    RibLayout {
        pitch_mm: 100.0,
        thickness_mm: 2.0,
        material: STEF.into(),
        min_wall_mm: 2.0,
    }
}
