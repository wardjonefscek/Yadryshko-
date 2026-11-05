//! Проект: всё, что вводит пользователь. Сохраняется в открытый JSON (FR-07).

use cadplus_aero::{LoadCase, Material, RibLayout, Spar, Wing, material, preset};
use cadplus_rib::RibSettings;
use cadplus_rib::airfoil::{Airfoil, CLARK_Y_DAT};
use serde::{Deserialize, Serialize};

/// Версия формата файла проекта.
pub const FORMAT_VERSION: u32 = 1;

/// Исходные данные.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// Версия формата.
    pub format: u32,
    /// Крыло.
    pub wing: Wing,
    /// Расчётный случай.
    pub load: LoadCase,
    /// Лонжероны одной консоли.
    pub spars: Vec<Spar>,
    /// Нервюры.
    pub ribs: RibLayout,
    /// Настройки оптимизации и изготовления нервюр.
    pub rib_settings: RibSettings,
    /// Библиотека материалов.
    pub materials: Vec<Material>,
    /// Координаты профиля (текст файла .dat).
    pub airfoil_dat: String,
}

impl Default for Project {
    /// Самолёт из сводки параметров (05.11.2026).
    fn default() -> Self {
        Self {
            format: FORMAT_VERSION,
            wing: preset::wing(),
            load: preset::load_case(),
            spars: preset::spars(),
            ribs: preset::ribs(),
            rib_settings: RibSettings::default(),
            materials: material::default_library(),
            airfoil_dat: CLARK_Y_DAT.to_string(),
        }
    }
}

impl Project {
    /// Профиль из текста проекта.
    pub fn airfoil(&self) -> Result<Airfoil, String> {
        Airfoil::parse_dat(&self.airfoil_dat)
    }

    /// Сохранить.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        for m in &self.materials {
            m.validate()?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text)
            .map_err(|e| format!("Не удалось записать {}: {e}", path.display()))
    }

    /// Открыть.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Не удалось прочитать {}: {e}", path.display()))?;
        let p: Self =
            serde_json::from_str(&text).map_err(|e| format!("Файл проекта повреждён: {e}"))?;
        if p.format > FORMAT_VERSION {
            return Err(format!(
                "Проект сохранён более новой версией программы (формат {})",
                p.format
            ));
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_everything() {
        let p = Project::default();
        let dir = std::env::temp_dir().join(format!("cadplus-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("проект.cadplus.json");
        p.save(&f).unwrap();
        let q = Project::load(&f).unwrap();
        assert_eq!(p, q);
        std::fs::remove_dir_all(&dir).ok();
    }
}
