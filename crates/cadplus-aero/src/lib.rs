//! Cad+: расчёт консоли крыла.
//!
//! Нагрузка по размаху (Шренк), эпюры перерезывающей силы и изгибающего момента,
//! напряжения и прогиб лонжеронов, нагрузки на нервюры и проверка соединительной трубки.
//! Результат — расчётная оценка: он не заменяет статические испытания.

pub mod analysis;
pub mod material;
pub mod preset;
pub mod section;
pub mod wing;

pub use analysis::{Analysis, Distribution, Input, LoadCase, RibLayout, analyze};
pub use material::Material;
pub use section::Section;
pub use wing::{Spar, Status, Wing};
