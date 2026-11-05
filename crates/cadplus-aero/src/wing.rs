//! Геометрия трапециевидного крыла и описание лонжеронов.
//!
//! Координата `z` отсчитывается от корневой нервюры консоли к концу, в мм.

use crate::section::Section;
use serde::{Deserialize, Serialize};

/// Трапециевидное крыло с прямой передней кромкой.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wing {
    /// Полный размах (две консоли), мм.
    pub span_mm: f64,
    /// Корневая хорда, мм.
    pub root_chord_mm: f64,
    /// Концевая хорда, мм.
    pub tip_chord_mm: f64,
    /// Поперечное V каждой консоли, градусы.
    pub dihedral_deg: f64,
}

impl Wing {
    /// Проверка размеров.
    pub fn validate(&self) -> Result<(), String> {
        for (what, v) in [
            ("размах", self.span_mm),
            ("корневая хорда", self.root_chord_mm),
            ("концевая хорда", self.tip_chord_mm),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!("Крыло: {what} должен быть больше нуля"));
            }
        }
        if !(0.0..=15.0).contains(&self.dihedral_deg.abs()) {
            return Err("Крыло: поперечное V вне диапазона ±15°".into());
        }
        Ok(())
    }

    /// Полуразмах (длина консоли), мм.
    pub fn half_span(&self) -> f64 {
        self.span_mm / 2.0
    }

    /// Хорда на расстоянии `z` от корня, мм.
    pub fn chord_at(&self, z: f64) -> f64 {
        let t = (z / self.half_span()).clamp(0.0, 1.0);
        self.root_chord_mm + (self.tip_chord_mm - self.root_chord_mm) * t
    }

    /// Сужение λ = концевая хорда / корневая.
    pub fn taper(&self) -> f64 {
        self.tip_chord_mm / self.root_chord_mm
    }

    /// Площадь крыла (обе консоли), мм².
    pub fn area_mm2(&self) -> f64 {
        (self.root_chord_mm + self.tip_chord_mm) / 2.0 * self.span_mm
    }

    /// Средняя аэродинамическая хорда, мм.
    pub fn mac_mm(&self) -> f64 {
        let l = self.taper();
        2.0 / 3.0 * self.root_chord_mm * (1.0 + l + l * l) / (1.0 + l)
    }

    /// Положение САХ по размаху от корня, мм.
    pub fn mac_z_mm(&self) -> f64 {
        let l = self.taper();
        self.span_mm / 6.0 * (1.0 + 2.0 * l) / (1.0 + l)
    }

    /// Удлинение.
    pub fn aspect_ratio(&self) -> f64 {
        self.span_mm * self.span_mm / self.area_mm2()
    }
}

/// Статус элемента конструкции.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    /// Выбрано и подтверждено.
    Approved,
    /// Кандидат для расчёта, не утверждён.
    Candidate,
}

/// Лонжерон или соединительная трубка в пределах консоли.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spar {
    /// Название для отчёта.
    pub name: String,
    /// Сечение.
    pub section: Section,
    /// Название материала из библиотеки.
    pub material: String,
    /// Начало по размаху от корневой нервюры, мм.
    pub z_start: f64,
    /// Конец по размаху от корневой нервюры, мм.
    pub z_end: f64,
    /// Положение по хорде от передней кромки, доля 0…1.
    pub chord_pos: f64,
    /// Это соединитель через обе консоли (передаёт корневой момент в фюзеляж).
    pub joiner: bool,
    /// Статус.
    pub status: Status,
}

impl Spar {
    /// Проверка.
    pub fn validate(&self, wing: &Wing) -> Result<(), String> {
        self.section
            .validate()
            .map_err(|e| format!("{}: {e}", self.name))?;
        if !(self.z_start >= 0.0
            && self.z_end > self.z_start
            && self.z_end <= wing.half_span() + 1e-9)
        {
            return Err(format!(
                "{}: участок {}–{} мм выходит за консоль 0–{} мм",
                self.name,
                self.z_start,
                self.z_end,
                wing.half_span()
            ));
        }
        if !(0.02..=0.95).contains(&self.chord_pos) {
            return Err(format!(
                "{}: положение по хорде вне диапазона 2–95%",
                self.name
            ));
        }
        Ok(())
    }

    /// Проходит ли лонжерон через станцию `z`.
    pub fn covers(&self, z: f64) -> bool {
        z >= self.z_start - 1e-9 && z <= self.z_end + 1e-9
    }
}
