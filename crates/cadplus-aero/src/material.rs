//! Библиотека материалов (FR-12).
//!
//! Значения по умолчанию — справочные. Поле `verified = false` означает:
//! «проверить по паспорту материала, данным продавца или испытанием».

use serde::{Deserialize, Serialize};

/// Материал с механическими свойствами.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// Название, по нему материал выбирают в лонжеронах и нервюрах.
    pub name: String,
    /// Модуль упругости, МПа.
    pub e_mpa: f64,
    /// Предел прочности при изгибе или растяжении, МПа. Сравнивается с расчётным
    /// напряжением, в которое уже входит коэффициент безопасности.
    pub strength_mpa: f64,
    /// Предел прочности на смятие кромки отверстия, МПа.
    pub bearing_mpa: f64,
    /// Коэффициент Пуассона.
    pub poisson: f64,
    /// Плотность, г/см³.
    pub density_g_cm3: f64,
    /// Допуск на расширение при FDM-печати, мм (для листовых материалов — 0).
    pub print_tolerance_mm: f64,
    /// Свойства подтверждены паспортом или испытанием.
    pub verified: bool,
    /// Откуда значения и что проверить.
    pub note: String,
}

impl Material {
    /// Проверка диапазонов (FR-12: значения вне диапазона не сохраняются).
    pub fn validate(&self) -> Result<(), String> {
        let n = &self.name;
        if n.trim().is_empty() {
            return Err("У материала нет названия".into());
        }
        check_range(n, "модуль упругости, МПа", self.e_mpa, 100.0, 500_000.0)?;
        check_range(n, "предел прочности, МПа", self.strength_mpa, 1.0, 5_000.0)?;
        check_range(
            n,
            "прочность на смятие, МПа",
            self.bearing_mpa,
            1.0,
            5_000.0,
        )?;
        check_range(n, "коэффициент Пуассона", self.poisson, 0.0, 0.49)?;
        check_range(n, "плотность, г/см³", self.density_g_cm3, 0.05, 25.0)?;
        check_range(n, "допуск печати, мм", self.print_tolerance_mm, 0.0, 1.0)?;
        Ok(())
    }
}

fn check_range(name: &str, what: &str, v: f64, lo: f64, hi: f64) -> Result<(), String> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(())
    } else {
        Err(format!("{name}: {what} = {v} вне диапазона {lo}…{hi}"))
    }
}

/// Найти материал по названию.
pub fn find<'a>(lib: &'a [Material], name: &str) -> Option<&'a Material> {
    lib.iter().find(|m| m.name == name)
}

#[allow(clippy::too_many_arguments)]
fn mat(
    name: &str,
    e_mpa: f64,
    strength_mpa: f64,
    bearing_mpa: f64,
    poisson: f64,
    density_g_cm3: f64,
    print_tolerance_mm: f64,
    note: &str,
) -> Material {
    Material {
        name: name.into(),
        e_mpa,
        strength_mpa,
        bearing_mpa,
        poisson,
        density_g_cm3,
        print_tolerance_mm,
        verified: false,
        note: note.into(),
    }
}

/// Название углепластика для трубок по умолчанию.
pub const CARBON_TUBE: &str = "Углепластик (трубка)";
/// Название стеклотекстолита по умолчанию.
pub const STEF: &str = "СТЭФ (стеклотекстолит)";

/// Справочная библиотека. Все значения помечены как непроверенные.
pub fn default_library() -> Vec<Material> {
    vec![
        mat(
            CARBON_TUBE,
            70_000.0,
            600.0,
            300.0,
            0.30,
            1.55,
            0.0,
            "Осторожная оценка. У намоточных и пултрузионных трубок E от 70 до 130 ГПа — уточнить у продавца.",
        ),
        mat(
            STEF,
            20_000.0,
            200.0,
            150.0,
            0.15,
            1.80,
            0.0,
            "Оценка для листового стеклотекстолита. Плотность проверить взвешиванием образца, прочность — по марке и ГОСТ 12652.",
        ),
        mat(
            "Сосна (рейка)",
            10_000.0,
            60.0,
            30.0,
            0.35,
            0.50,
            0.0,
            "Вдоль волокон, без сучков. Разброс у древесины большой.",
        ),
        mat(
            "Алюминий Д16Т",
            72_000.0,
            400.0,
            600.0,
            0.33,
            2.78,
            0.0,
            "Справочные значения для прутка и трубы.",
        ),
        mat(
            "PETG (FDM)",
            2_000.0,
            45.0,
            40.0,
            0.38,
            1.27,
            0.20,
            "Свойства печатной детали зависят от заполнения и направления слоёв.",
        ),
        mat(
            "PLA (FDM)",
            3_300.0,
            50.0,
            45.0,
            0.36,
            1.24,
            0.15,
            "Свойства печатной детали зависят от заполнения и направления слоёв.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_valid_and_unverified() {
        for m in default_library() {
            m.validate().unwrap();
            assert!(
                !m.verified,
                "{} не должен быть помечен как проверенный",
                m.name
            );
        }
    }

    #[test]
    fn out_of_range_is_rejected() {
        let mut m = default_library().remove(0);
        m.poisson = 0.7;
        assert!(m.validate().is_err());
        m.poisson = 0.3;
        m.print_tolerance_mm = 1.5;
        assert!(m.validate().is_err());
    }
}
