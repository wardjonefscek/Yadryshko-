//! Окно программы: дерево слева, свойства справа, чертёж и графики в центре (NFR-05).

use crate::project::Project;
use crate::worker::{self, Job, Msg, Task};
use cadplus_aero::{Analysis, Distribution, Input, Section, Spar, Status, analyze, material};
use cadplus_rib::geom::{HoleShape, RibGeometry};
use cadplus_rib::setup::{self, Grid, RibSetup};
use cadplus_rib::{Manufacturing, RibResult, RibSettings, export, grid_params, prepare_all};
use cadplus_topopt::Cell;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Stroke, Vec2};
use egui_plot::{Legend, Line, Plot};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Что выбрано в дереве.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sel {
    Wing,
    Load,
    Spar(usize),
    Ribs,
    Rib(usize),
    Manufacturing,
    Materials,
}

/// Состояние одной нервюры в окне.
#[derive(Default)]
struct RibState {
    /// Подготовленная задача (для показа запретных зон до расчёта).
    preview: Option<(RibGeometry, RibSettings, Result<RibSetup, String>)>,
    /// Плотности (идёт расчёт или готово).
    density: Option<(Grid, Vec<f64>)>,
    /// Результат.
    result: Option<RibResult>,
    /// Для каких данных получен результат.
    computed_for: Option<(RibGeometry, RibSettings)>,
    /// История податливости.
    history: Vec<[f64; 2]>,
    /// Текстура плотностей.
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
    /// Ошибка расчёта.
    error: Option<String>,
}

/// Приложение.
pub struct App {
    project: Project,
    /// Снимок для отмены: последнее зафиксированное состояние.
    committed: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    /// Для какого проекта посчитаны производные данные.
    computed: Option<Project>,
    analysis: Result<Analysis, String>,
    ribs: Vec<(RibGeometry, Vec<String>)>,
    rib_states: Vec<RibState>,
    airfoil_error: Option<String>,
    sel: Sel,
    job: Option<Job>,
    path: String,
    dat_path: String,
    status: String,
    /// Запустить оптимизацию всех нервюр на первом кадре (ключ `--optimize`).
    autorun: bool,
}

const UNDO_DEPTH: usize = 200;

fn default_path() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    let docs = home.join("Documents");
    let base = if docs.is_dir() { docs } else { home };
    base.join("Cad+").join("проект.cadplus.json")
}

impl App {
    /// Новое окно. Ключи запуска: путь к файлу проекта; `--rib N` — открыть нервюру N;
    /// `--ribs` — список нервюр; `--optimize` — сразу оптимизировать все нервюры.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut sel = Sel::Wing;
        let mut autorun = false;
        let mut path = default_path();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--rib" => {
                    if let Some(n) = it.next().and_then(|n| n.parse::<usize>().ok()) {
                        sel = Sel::Rib(n.saturating_sub(1));
                    }
                }
                "--ribs" => sel = Sel::Ribs,
                "--optimize" => autorun = true,
                other if !other.starts_with("--") => path = PathBuf::from(other),
                _ => {}
            }
        }
        let (project, status) = match Project::load(&path) {
            Ok(p) => (p, format!("Открыт {}", path.display())),
            Err(_) => (
                Project::default(),
                "Новый проект: самолёт из сводки параметров".to_string(),
            ),
        };
        Self {
            committed: project.clone(),
            project,
            undo: Vec::new(),
            redo: Vec::new(),
            computed: None,
            analysis: Err(String::new()),
            ribs: Vec::new(),
            rib_states: Vec::new(),
            airfoil_error: None,
            sel,
            job: None,
            path: path.display().to_string(),
            dat_path: String::new(),
            status,
            autorun,
        }
    }

    /// Пересчитать крыло и геометрию нервюр, если исходные данные изменились.
    fn refresh(&mut self) {
        if self.computed.as_ref() == Some(&self.project) {
            return;
        }
        let p = &self.project;
        let af = p.airfoil();
        self.airfoil_error = af.as_ref().err().cloned();
        let table = af.as_ref().map(|a| a.thickness_table()).unwrap_or_default();
        self.analysis = analyze(&Input {
            wing: &p.wing,
            load: &p.load,
            spars: &p.spars,
            materials: &p.materials,
            ribs: &p.ribs,
            thickness_table: &table,
        });
        let tol =
            material::find(&p.materials, &p.ribs.material).map_or(0.0, |m| m.print_tolerance_mm);
        self.ribs = match (&self.analysis, &af) {
            (Ok(a), Ok(af)) => prepare_all(af, a, &p.spars, &p.rib_settings, tol),
            _ => Vec::new(),
        };
        self.rib_states
            .resize_with(self.ribs.len(), RibState::default);
        if let Sel::Rib(i) = self.sel
            && i >= self.ribs.len()
        {
            self.sel = Sel::Ribs;
        }
        self.computed = Some(self.project.clone());
    }

    fn rib_material(&self) -> Option<&cadplus_aero::Material> {
        material::find(&self.project.materials, &self.project.ribs.material)
    }

    /// Подготовить задачу нервюры (или взять готовую).
    fn rib_setup(&mut self, i: usize) -> Result<RibSetup, String> {
        let g = self.ribs[i].0.clone();
        let s = self.project.rib_settings.clone();
        if let Some((pg, ps, r)) = &self.rib_states[i].preview
            && *pg == g
            && *ps == s
        {
            return r.clone();
        }
        let m = self
            .rib_material()
            .ok_or("Материал нервюр не найден в библиотеке")?;
        let gp = grid_params(&s, m.e_mpa, m.poisson, self.project.ribs.thickness_mm);
        let r = s.validate().and_then(|_| setup::build(&g, &gp));
        self.rib_states[i].preview = Some((g, s, r.clone()));
        r
    }

    fn start(&mut self, ctx: &egui::Context, which: Vec<usize>) {
        if self.job.is_some() {
            return;
        }
        let Some(m) = self.rib_material().cloned() else {
            self.status = "Материал нервюр не найден в библиотеке".into();
            return;
        };
        let mut tasks = Vec::new();
        for i in which {
            match self.rib_setup(i) {
                Ok(s) => {
                    let st = &mut self.rib_states[i];
                    st.history.clear();
                    st.error = None;
                    st.result = None;
                    st.density = Some((
                        s.grid,
                        vec![self.project.rib_settings.volfrac; s.problem.cells.len()],
                    ));
                    st.texture_dirty = true;
                    st.computed_for =
                        Some((self.ribs[i].0.clone(), self.project.rib_settings.clone()));
                    tasks.push(Task { rib: i, setup: s });
                }
                Err(e) => self.rib_states[i].error = Some(e),
            }
        }
        if tasks.is_empty() {
            self.status = "Нечего считать: проверьте ошибки нервюр".into();
            return;
        }
        let c = ctx.clone();
        self.job = Some(worker::spawn(
            tasks,
            self.project.rib_settings.clone(),
            self.project.ribs.thickness_mm,
            m.density_g_cm3,
            move || c.request_repaint(),
        ));
        self.status = "Идёт оптимизация нервюр…".into();
    }

    fn poll(&mut self) {
        let Some(job) = &mut self.job else { return };
        let mut finished = false;
        while let Ok(msg) = job.rx.try_recv() {
            match msg {
                Msg::Started { rib } => job.current = Some((rib, 0)),
                Msg::Progress {
                    rib,
                    iter,
                    compliance,
                    density,
                } => {
                    job.current = Some((rib, iter));
                    if let Some(st) = self.rib_states.get_mut(rib) {
                        st.history.push([iter as f64, compliance]);
                        if let Some((g, _)) = st.density {
                            st.density = Some((g, density));
                            st.texture_dirty = true;
                        }
                    }
                }
                Msg::Done {
                    rib,
                    result,
                    density,
                } => {
                    job.done += 1;
                    if let Some(st) = self.rib_states.get_mut(rib) {
                        if let Some((g, _)) = st.density {
                            st.density = Some((g, density));
                        }
                        st.texture_dirty = true;
                        st.result = Some(*result);
                    }
                }
                Msg::Failed { rib, error } => {
                    job.done += 1;
                    if let Some(st) = self.rib_states.get_mut(rib) {
                        st.error = Some(error);
                    }
                }
                Msg::Finished => finished = true,
            }
        }
        if finished {
            let cancelled = job.cancel.load(Ordering::Relaxed);
            self.status = if cancelled {
                "Оптимизация остановлена".into()
            } else {
                "Оптимизация нервюр завершена".into()
            };
            self.job = None;
        }
    }

    fn is_fresh(&self, i: usize) -> bool {
        let st = &self.rib_states[i];
        st.computed_for.as_ref()
            == Some(&(self.ribs[i].0.clone(), self.project.rib_settings.clone()))
    }

    fn project_dir(&self) -> PathBuf {
        Path::new(&self.path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    fn save(&mut self) {
        let path = PathBuf::from(&self.path);
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        self.status = match self.project.save(&path) {
            Ok(()) => format!("Сохранено: {}", path.display()),
            Err(e) => e,
        };
    }

    fn open(&mut self) {
        match Project::load(Path::new(&self.path)) {
            Ok(p) => {
                self.undo.push(self.project.clone());
                self.project = p.clone();
                self.committed = p;
                self.rib_states.clear();
                self.status = format!("Открыт {}", self.path);
            }
            Err(e) => self.status = e,
        }
    }

    fn export(&mut self) {
        if self.ribs.is_empty() {
            self.status = "Нет нервюр для экспорта — проверьте исходные данные".into();
            return;
        }
        let fresh: Vec<bool> = (0..self.ribs.len())
            .map(|i| self.is_fresh(i) && self.rib_states[i].result.is_some())
            .collect();
        let parts: Vec<export::Part> = self
            .ribs
            .iter()
            .enumerate()
            .map(|(i, (g, _))| export::Part {
                geometry: g,
                pockets: if fresh[i] {
                    &self.rib_states[i]
                        .result
                        .as_ref()
                        .expect("есть результат")
                        .pockets
                } else {
                    &[]
                },
            })
            .collect();
        let field = self.project.rib_settings.manufacturing.field_mm();
        let (dxf, warns) = export::to_dxf(&parts, field);
        let svg = export::to_svg(&parts, field);
        let dir = self.project_dir().join("export");
        let res = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join("нервюры.dxf"), dxf))
            .and_then(|_| std::fs::write(dir.join("нервюры.svg"), svg));
        let solid = fresh.iter().filter(|f| !**f).count();
        self.status = match res {
            Ok(()) => {
                let mut s = format!("Экспорт: {}", dir.display());
                if solid > 0 {
                    s.push_str(&format!(" — {solid} нерв. без оптимизации (сплошные)"));
                }
                for w in warns {
                    s.push_str(" — ");
                    s.push_str(&w);
                }
                s
            }
            Err(e) => format!("Ошибка экспорта: {e}"),
        };
    }

    /// Отмена (Ctrl+Z) — 200 шагов (FR-02).
    fn commit_undo(&mut self, ctx: &egui::Context) {
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        if self.project != self.committed && !pointer_down {
            self.undo
                .push(std::mem::replace(&mut self.committed, self.project.clone()));
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn do_undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.project, p.clone()));
            self.committed = p;
        }
    }

    fn do_redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.project, p.clone()));
            self.committed = p;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        use egui::{Key, KeyboardShortcut, Modifiers};
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)))
        {
            self.do_undo();
        }
        if ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
                || i.consume_shortcut(&KeyboardShortcut::new(
                    Modifiers::COMMAND | Modifiers::SHIFT,
                    Key::Z,
                ))
        }) {
            self.do_redo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)))
        {
            self.save();
        }
        self.refresh();
        if std::mem::take(&mut self.autorun) {
            let all = (0..self.ribs.len()).collect();
            self.start(&ctx, all);
        }

        egui::Panel::top("menu").show(ui, |ui| self.top_bar(ui, &ctx));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if let Some(job) = &self.job {
                    let (rib, it) = job.current.unwrap_or((0, 0));
                    let max = self.project.rib_settings.max_iter.max(1);
                    let frac = (job.done as f32 + it as f32 / max as f32) / job.total.max(1) as f32;
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(260.0)
                            .text(format!("нервюра {} · итерация {it}", rib + 1)),
                    );
                }
            });
        });
        egui::Panel::left("tree")
            .resizable(true)
            .default_size(230.0)
            .show(ui, |ui| self.tree(ui));
        egui::Panel::right("props")
            .resizable(true)
            .default_size(320.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.properties(ui));
            });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.sel {
                Sel::Rib(i) => self.rib_view(ui, &ctx, i),
                Sel::Ribs | Sel::Manufacturing => self.ribs_overview(ui, &ctx),
                _ => self.wing_view(ui),
            });
        });
        self.commit_undo(&ctx);
    }
}

// ───────────────────────────── Панели ─────────────────────────────

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.strong("Cad+");
            ui.separator();
            ui.label("Проект:");
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(360.0));
            if ui.button("Открыть").clicked() {
                self.open();
            }
            if ui.button("Сохранить").on_hover_text("Ctrl+S").clicked() {
                self.save();
            }
            if ui
                .button("Новый")
                .on_hover_text("Самолёт из сводки параметров")
                .clicked()
            {
                self.undo.push(self.project.clone());
                self.project = Project::default();
                self.rib_states.clear();
            }
            ui.separator();
            if ui
                .add_enabled(!self.undo.is_empty(), egui::Button::new("↶ Отменить"))
                .on_hover_text("Ctrl+Z")
                .clicked()
            {
                self.do_undo();
            }
            if ui
                .add_enabled(!self.redo.is_empty(), egui::Button::new("↷ Повторить"))
                .on_hover_text("Ctrl+Y")
                .clicked()
            {
                self.do_redo();
            }
            ui.separator();
            let busy = self.job.is_some();
            if ui
                .add_enabled(
                    !busy && !self.ribs.is_empty(),
                    egui::Button::new("Оптимизировать все нервюры"),
                )
                .clicked()
            {
                let all: Vec<usize> = (0..self.ribs.len()).collect();
                self.start(ctx, all);
            }
            if ui
                .add_enabled(busy, egui::Button::new("Остановить"))
                .clicked()
                && let Some(j) = &self.job
            {
                j.cancel.store(true, Ordering::Relaxed);
            }
            if ui.button("Экспорт DXF + SVG").clicked() {
                self.export();
            }
        });
    }

    fn tree(&mut self, ui: &mut egui::Ui) {
        ui.heading("Самолёт");
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.selectable_value(&mut self.sel, Sel::Wing, "Крыло");
            ui.selectable_value(&mut self.sel, Sel::Load, "Расчётный случай");
            ui.collapsing("Лонжероны", |ui| {
                for (k, s) in self.project.spars.iter().enumerate() {
                    let tag = if s.status == Status::Candidate {
                        " (кандидат)"
                    } else {
                        ""
                    };
                    let name = s.name.trim_end_matches(" (кандидат)");
                    ui.selectable_value(&mut self.sel, Sel::Spar(k), format!("{name}{tag}"));
                }
                if ui.small_button("+ добавить лонжерон").clicked() {
                    let s = self.project.wing.half_span();
                    self.project.spars.push(Spar {
                        name: format!("Лонжерон {}", self.project.spars.len() + 1),
                        section: Section::Tube {
                            d_out: 6.0,
                            wall: 1.0,
                        },
                        material: material::CARBON_TUBE.into(),
                        z_start: 0.0,
                        z_end: s,
                        chord_pos: 0.65,
                        joiner: false,
                        status: Status::Candidate,
                    });
                    self.sel = Sel::Spar(self.project.spars.len() - 1);
                }
            });
            ui.collapsing("Нервюры", |ui| {
                ui.selectable_value(&mut self.sel, Sel::Ribs, "Все нервюры");
                for (i, (g, w)) in self.ribs.iter().enumerate() {
                    let mark = if !w.is_empty()
                        || self.rib_states.get(i).is_some_and(|s| s.error.is_some())
                    {
                        " !"
                    } else {
                        ""
                    };
                    ui.selectable_value(
                        &mut self.sel,
                        Sel::Rib(i),
                        format!("Нервюра {} · z = {:.0}{mark}", g.index + 1, g.z),
                    );
                }
            })
            .header_response
            .on_hover_text("Нервюры одной консоли; в самолёте каждая — в двух экземплярах");
            ui.selectable_value(
                &mut self.sel,
                Sel::Manufacturing,
                "Изготовление и оптимизация",
            );
            ui.selectable_value(&mut self.sel, Sel::Materials, "Материалы");
        });
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        let half = self.project.wing.half_span();
        match self.sel {
            Sel::Wing => {
                ui.heading("Крыло");
                let w = &mut self.project.wing;
                grid(ui, "wing", |ui| {
                    num(ui, "Размах", &mut w.span_mm, 100.0..=5000.0, 1.0, " мм");
                    num(
                        ui,
                        "Корневая хорда",
                        &mut w.root_chord_mm,
                        20.0..=1000.0,
                        1.0,
                        " мм",
                    );
                    num(
                        ui,
                        "Концевая хорда",
                        &mut w.tip_chord_mm,
                        10.0..=1000.0,
                        1.0,
                        " мм",
                    );
                    num(
                        ui,
                        "Поперечное V",
                        &mut w.dihedral_deg,
                        -15.0..=15.0,
                        0.1,
                        "°",
                    );
                });
                ui.separator();
                let w = &self.project.wing;
                ui.label(format!("Площадь: {:.1} дм²", w.area_mm2() / 1e4));
                ui.label(format!(
                    "САХ: {:.1} мм на z = {:.0} мм",
                    w.mac_mm(),
                    w.mac_z_mm()
                ));
                ui.label(format!("Удлинение: {:.2}", w.aspect_ratio()));
                ui.separator();
                ui.label("Профиль");
                match self.project.airfoil() {
                    Ok(af) => {
                        let (t, x) = af.max_thickness();
                        ui.label(format!(
                            "{}: толщина {:.1}% на {:.0}% хорды",
                            af.name,
                            t * 100.0,
                            x * 100.0
                        ));
                    }
                    Err(e) => {
                        ui.colored_label(Color32::from_rgb(200, 40, 40), e);
                    }
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dat_path)
                            .hint_text("путь к файлу .dat")
                            .desired_width(190.0),
                    );
                    if ui.button("Загрузить").clicked() {
                        match std::fs::read_to_string(&self.dat_path) {
                            Ok(t) => match cadplus_rib::airfoil::Airfoil::parse_dat(&t) {
                                Ok(_) => {
                                    self.project.airfoil_dat = t;
                                    self.status = "Профиль загружен".into();
                                }
                                Err(e) => self.status = e,
                            },
                            Err(e) => self.status = format!("Не удалось прочитать файл: {e}"),
                        }
                    }
                });
                if ui.small_button("Вернуть Clark Y (UIUC)").clicked() {
                    self.project.airfoil_dat = cadplus_rib::airfoil::CLARK_Y_DAT.to_string();
                }
            }
            Sel::Load => {
                ui.heading("Расчётный случай");
                let l = &mut self.project.load;
                grid(ui, "load", |ui| {
                    num(
                        ui,
                        "Полётная масса",
                        &mut l.mass_kg,
                        0.05..=50.0,
                        0.01,
                        " кг",
                    );
                    num(ui, "Перегрузка n", &mut l.load_factor, 1.0..=20.0, 0.1, "");
                    num(
                        ui,
                        "Коэфф. безопасности f",
                        &mut l.safety_factor,
                        1.0..=5.0,
                        0.05,
                        "",
                    );
                    ui.label("Распределение");
                    egui::ComboBox::from_id_salt("dist")
                        .selected_text(dist_name(l.distribution))
                        .show_ui(ui, |ui| {
                            for d in [
                                Distribution::Schrenk,
                                Distribution::Elliptic,
                                Distribution::Trapezoidal,
                                Distribution::Uniform,
                            ] {
                                ui.selectable_value(&mut l.distribution, d, dist_name(d));
                            }
                        });
                    ui.end_row();
                });
                ui.label(format!(
                    "Расчётная подъёмная сила: {:.1} Н",
                    self.project.load.design_lift_n()
                ));
                ui.small("Масса неизвестна точно: известно только оборудование (224,7 г). Уточните после взвешивания.");
            }
            Sel::Spar(k) if k < self.project.spars.len() => {
                let names: Vec<String> = self
                    .project
                    .materials
                    .iter()
                    .map(|m| m.name.clone())
                    .collect();
                let mut delete = false;
                {
                    let s = &mut self.project.spars[k];
                    ui.heading("Лонжерон");
                    ui.text_edit_singleline(&mut s.name);
                    grid(ui, "spar", |ui| {
                        ui.label("Сечение");
                        let kind = match s.section {
                            Section::Tube { .. } => 0,
                            Section::Rect { .. } => 1,
                            Section::RectTube { .. } => 2,
                        };
                        let mut nk = kind;
                        egui::ComboBox::from_id_salt("sec")
                            .selected_text(["Трубка", "Рейка", "Коробка"][kind])
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut nk, 0, "Трубка");
                                ui.selectable_value(&mut nk, 1, "Рейка");
                                ui.selectable_value(&mut nk, 2, "Коробка");
                            });
                        ui.end_row();
                        if nk != kind {
                            let (h, b) = (s.section.height(), s.section.width());
                            s.section = match nk {
                                0 => Section::Tube {
                                    d_out: h,
                                    wall: (h / 10.0).max(0.5),
                                },
                                1 => Section::Rect { b, h },
                                _ => Section::RectTube {
                                    b: b.max(4.0),
                                    h: h.max(4.0),
                                    wall: 1.0,
                                },
                            };
                        }
                        match &mut s.section {
                            Section::Tube { d_out, wall } => {
                                num(ui, "Наружный диаметр", d_out, 1.0..=60.0, 0.1, " мм");
                                num(ui, "Стенка", wall, 0.1..=30.0, 0.05, " мм");
                            }
                            Section::Rect { b, h } => {
                                num(ui, "Ширина (по хорде)", b, 0.5..=100.0, 0.1, " мм");
                                num(ui, "Высота", h, 0.5..=100.0, 0.1, " мм");
                            }
                            Section::RectTube { b, h, wall } => {
                                num(ui, "Ширина (по хорде)", b, 1.0..=100.0, 0.1, " мм");
                                num(ui, "Высота", h, 1.0..=100.0, 0.1, " мм");
                                num(ui, "Стенка", wall, 0.1..=20.0, 0.05, " мм");
                            }
                        }
                        ui.label("Материал");
                        material_combo(ui, "spar_mat", &mut s.material, &names);
                        ui.end_row();
                        num(
                            ui,
                            "Начало, от корня",
                            &mut s.z_start,
                            0.0..=half,
                            1.0,
                            " мм",
                        );
                        num(ui, "Конец, от корня", &mut s.z_end, 0.0..=half, 1.0, " мм");
                        let mut pct = s.chord_pos * 100.0;
                        num(ui, "Положение по хорде", &mut pct, 2.0..=95.0, 0.5, " %");
                        s.chord_pos = pct / 100.0;
                        ui.label("Соединитель консолей");
                        ui.checkbox(&mut s.joiner, "");
                        ui.end_row();
                        ui.label("Статус");
                        egui::ComboBox::from_id_salt("st")
                            .selected_text(if s.status == Status::Approved {
                                "Утверждён"
                            } else {
                                "Кандидат"
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut s.status, Status::Approved, "Утверждён");
                                ui.selectable_value(&mut s.status, Status::Candidate, "Кандидат");
                            });
                        ui.end_row();
                    });
                    ui.label(format!(
                        "{}: I = {:.1} мм⁴, площадь {:.1} мм²",
                        s.section.label(),
                        s.section.inertia(),
                        s.section.area()
                    ));
                    if ui.button("Удалить лонжерон").clicked() {
                        delete = true;
                    }
                }
                if let Ok(a) = &self.analysis
                    && let Some(sum) = a.spars.get(k)
                {
                    ui.separator();
                    ui.label(format!(
                        "σmax = {:.0} МПа на z = {:.0} мм",
                        sum.max_stress_mpa, sum.z_at_max
                    ));
                    ui.label(format!("Запас прочности: {}", margin_text(sum.margin)));
                    ui.label(format!("Масса на консоль: {:.1} г", sum.mass_g));
                    ui.label(format!(
                        "Высота {:.1} мм, в профиле доступно {:.1} мм",
                        sum.fit.0, sum.fit.1
                    ));
                }
                if delete {
                    self.project.spars.remove(k);
                    self.sel = Sel::Wing;
                }
            }
            Sel::Spar(_) => self.sel = Sel::Wing,
            Sel::Ribs | Sel::Rib(_) => {
                ui.heading("Нервюры");
                let names: Vec<String> = self
                    .project
                    .materials
                    .iter()
                    .map(|m| m.name.clone())
                    .collect();
                let r = &mut self.project.ribs;
                grid(ui, "ribs", |ui| {
                    num(
                        ui,
                        "Шаг",
                        &mut r.pitch_mm,
                        10.0..=half.max(10.0),
                        1.0,
                        " мм",
                    );
                    num(ui, "Толщина", &mut r.thickness_mm, 0.3..=20.0, 0.1, " мм");
                    ui.label("Материал");
                    material_combo(ui, "rib_mat", &mut r.material, &names);
                    ui.end_row();
                    num(
                        ui,
                        "Мин. стенка у лонжерона",
                        &mut r.min_wall_mm,
                        0.0..=20.0,
                        0.1,
                        " мм",
                    );
                });
                ui.separator();
                self.rib_settings_ui(ui);
            }
            Sel::Manufacturing => self.rib_settings_ui(ui),
            Sel::Materials => self.materials_ui(ui),
        }
    }

    fn rib_settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Изготовление и оптимизация");
        let s = &mut self.project.rib_settings;
        let cnc = s.manufacturing.is_cnc();
        ui.horizontal(|ui| {
            if ui.selectable_label(cnc, "Фрезерование ЧПУ").clicked() && !cnc {
                s.manufacturing = Manufacturing::Cnc {
                    cutter_d_mm: 2.0,
                    sheet_mm: (600.0, 600.0),
                };
            }
            if ui.selectable_label(!cnc, "FDM-печать").clicked() && cnc {
                s.manufacturing = Manufacturing::Fdm {
                    nozzle_mm: 0.4,
                    bed_mm: (256.0, 256.0),
                };
            }
        });
        grid(ui, "mfg", |ui| {
            match &mut s.manufacturing {
                Manufacturing::Cnc {
                    cutter_d_mm,
                    sheet_mm,
                } => {
                    num(ui, "Диаметр фрезы", cutter_d_mm, 0.3..=20.0, 0.1, " мм");
                    num(
                        ui,
                        "Поле станка X",
                        &mut sheet_mm.0,
                        50.0..=3000.0,
                        1.0,
                        " мм",
                    );
                    num(
                        ui,
                        "Поле станка Y",
                        &mut sheet_mm.1,
                        50.0..=3000.0,
                        1.0,
                        " мм",
                    );
                }
                Manufacturing::Fdm { nozzle_mm, bed_mm } => {
                    num(ui, "Сопло", nozzle_mm, 0.1..=2.0, 0.05, " мм");
                    num(ui, "Стол X", &mut bed_mm.0, 50.0..=2000.0, 1.0, " мм");
                    num(ui, "Стол Y", &mut bed_mm.1, 50.0..=2000.0, 1.0, " мм");
                }
            }
            num(
                ui,
                "Посадка отверстий",
                &mut s.fit_clearance_mm,
                0.0..=2.0,
                0.01,
                " мм",
            );
            num(
                ui,
                "Кромка по контуру",
                &mut s.edge_band_mm,
                0.5..=20.0,
                0.1,
                " мм",
            );
            num(
                ui,
                "Кольцо у отверстий",
                &mut s.ring_mm,
                0.5..=20.0,
                0.1,
                " мм",
            );
            let mut pct = s.volfrac * 100.0;
            num(ui, "Остаётся материала", &mut pct, 5.0..=95.0, 1.0, " %");
            s.volfrac = pct / 100.0;
            num(
                ui,
                "Мин. перемычка (фильтр)",
                &mut s.filter_mm,
                0.2..=20.0,
                0.1,
                " мм",
            );
            num(
                ui,
                "Размер элемента",
                &mut s.element_mm,
                0.2..=3.0,
                0.05,
                " мм",
            );
            let mut it = s.max_iter as f64;
            num(ui, "Предел итераций", &mut it, 10.0..=2000.0, 1.0, "");
            s.max_iter = it.round() as usize;
        });
        if !cnc {
            ui.small(
                "Для печати к отверстиям добавляется допуск расширения из карточки материала.",
            );
        }
    }

    fn materials_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Материалы");
        ui.small("Справочные значения. Отметьте «проверено», когда свойства подтверждены паспортом или испытанием.");
        let mut remove = None;
        let used: Vec<String> = self
            .project
            .spars
            .iter()
            .map(|s| s.material.clone())
            .chain([self.project.ribs.material.clone()])
            .collect();
        for (k, m) in self.project.materials.iter_mut().enumerate() {
            let title = if m.verified {
                m.name.clone()
            } else {
                format!("{} — не проверено", m.name)
            };
            egui::CollapsingHeader::new(title)
                .id_salt(("mat", k))
                .show(ui, |ui| {
                    grid(ui, ("matg", k), |ui| {
                        num(ui, "E", &mut m.e_mpa, 100.0..=500_000.0, 100.0, " МПа");
                        num(
                            ui,
                            "Предел прочности",
                            &mut m.strength_mpa,
                            1.0..=5000.0,
                            1.0,
                            " МПа",
                        );
                        num(
                            ui,
                            "Прочность на смятие",
                            &mut m.bearing_mpa,
                            1.0..=5000.0,
                            1.0,
                            " МПа",
                        );
                        num(ui, "Коэфф. Пуассона", &mut m.poisson, 0.0..=0.49, 0.01, "");
                        num(
                            ui,
                            "Плотность",
                            &mut m.density_g_cm3,
                            0.05..=25.0,
                            0.01,
                            " г/см³",
                        );
                        num(
                            ui,
                            "Допуск печати",
                            &mut m.print_tolerance_mm,
                            0.0..=1.0,
                            0.01,
                            " мм",
                        );
                        ui.label("Проверено");
                        ui.checkbox(&mut m.verified, "");
                        ui.end_row();
                    });
                    ui.add(egui::TextEdit::multiline(&mut m.note).desired_rows(2));
                    if used.contains(&m.name) {
                        ui.small("Используется — удалить нельзя.");
                    } else if ui.small_button("Удалить").clicked() {
                        remove = Some(k);
                    }
                });
        }
        if let Some(k) = remove {
            self.project.materials.remove(k);
        }
        if ui.button("+ Новый материал").clicked() {
            let mut m = material::default_library().remove(1);
            m.name = format!("Материал {}", self.project.materials.len() + 1);
            self.project.materials.push(m);
        }
    }

    // ───────────────────────────── Крыло ─────────────────────────────

    fn wing_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Консоль крыла");
        if let Some(e) = &self.airfoil_error {
            ui.colored_label(Color32::from_rgb(200, 40, 40), format!("Профиль: {e}"));
        }
        self.draw_planform(ui);
        let a = match &self.analysis {
            Ok(a) => a,
            Err(e) => {
                ui.colored_label(Color32::from_rgb(200, 40, 40), e);
                return;
            }
        };
        ui.horizontal_wrapped(|ui| {
            stat(
                ui,
                "Подъёмная сила (расч.)",
                format!("{:.1} Н", a.design_lift_n),
            );
            stat(ui, "На консоль", format!("{:.1} Н", a.half_lift_n));
            stat(
                ui,
                "M у корня",
                format!("{:.2} Н·м", a.root_moment_nmm / 1000.0),
            );
            stat(
                ui,
                "Нагрузка на крыло",
                format!("{:.1} г/дм²", a.wing_loading_g_dm2),
            );
            stat(
                ui,
                "Прогиб конца",
                a.tip_deflection_mm
                    .map_or("не определён".into(), |v| format!("{v:.1} мм")),
            );
        });
        let z: Vec<f64> = a.stations.iter().map(|s| s.z).collect();
        let series = |f: &dyn Fn(&cadplus_aero::analysis::Station) -> f64| -> Vec<[f64; 2]> {
            a.stations.iter().zip(&z).map(|(s, &z)| [z, f(s)]).collect()
        };
        let m = series(&|s| s.moment / 1000.0);
        let q = series(&|s| s.shear);
        let v: Vec<[f64; 2]> = a
            .stations
            .iter()
            .filter_map(|s| s.deflection.map(|d| [s.z, d]))
            .collect();
        ui.columns(2, |cols| {
            plot(
                &mut cols[0],
                "m",
                "Изгибающий момент, Н·м",
                vec![("M", m, Color32::from_rgb(31, 95, 191))],
            );
            plot(
                &mut cols[1],
                "q",
                "Перерезывающая сила, Н",
                vec![("Q", q, Color32::from_rgb(192, 57, 43))],
            );
        });
        let colors = [
            Color32::from_rgb(192, 57, 43),
            Color32::from_rgb(39, 120, 70),
            Color32::from_rgb(130, 60, 160),
            Color32::from_rgb(200, 120, 20),
        ];
        let stress: Vec<(&str, Vec<[f64; 2]>, Color32)> = self
            .project
            .spars
            .iter()
            .enumerate()
            .map(|(j, sp)| {
                let pts = a
                    .stations
                    .iter()
                    .filter_map(|s| s.spar_stress.get(j).copied().flatten().map(|v| [s.z, v]))
                    .collect();
                (sp.name.as_str(), pts, colors[j % colors.len()])
            })
            .collect();
        ui.columns(2, |cols| {
            plot(
                &mut cols[0],
                "v",
                "Прогиб, мм",
                vec![("прогиб", v, Color32::from_rgb(60, 60, 60))],
            );
            plot(&mut cols[1], "s", "Напряжение в лонжеронах, МПа", stress);
        });
        ui.separator();
        ui.strong("Лонжероны");
        egui::Grid::new("spars_tbl").striped(true).show(ui, |ui| {
            for h in ["Название", "Сечение", "σmax, МПа", "Запас", "Масса, г"]
            {
                ui.strong(h);
            }
            ui.end_row();
            for (sp, s) in self.project.spars.iter().zip(&a.spars) {
                ui.label(&sp.name);
                ui.label(sp.section.label());
                ui.label(format!("{:.0}", s.max_stress_mpa));
                margin_label(ui, s.margin);
                ui.label(format!("{:.1}", s.mass_g));
                ui.end_row();
            }
        });
        for j in &a.joiners {
            ui.label(format!(
                "{}: держат нервюры {:?} мм, сила на отверстие {:.1} Н, смятие {:.2} МПа (запас {}), момент в конце трубки {:.2} Н·м.",
                j.name,
                j.ribs_on_tube.iter().map(|z| *z as i64).collect::<Vec<_>>(),
                j.bearing_force_n,
                j.bearing_stress_mpa,
                margin_text(j.bearing_margin),
                j.moment_at_end / 1000.0
            ));
        }
        warnings(ui, &a.warnings);
        ui.small("Результат — расчётная оценка. Он не заменяет статические испытания крыла и соединителя.");
    }

    fn draw_planform(&self, ui: &mut egui::Ui) {
        let w = &self.project.wing;
        let s = w.half_span();
        let cr = w.root_chord_mm.max(w.tip_chord_mm);
        let width = ui.available_width().max(300.0);
        let (resp, painter) = ui.allocate_painter(Vec2::new(width, 210.0), egui::Sense::hover());
        let rect = resp.rect;
        painter.rect_filled(rect, 4.0, Color32::WHITE);
        let m = 28.0;
        let k = ((rect.width() - 2.0 * m) as f64 / s).min((rect.height() - 2.0 * m) as f64 / cr);
        let o = rect.min + Vec2::new(m, m);
        let p = |z: f64, y: f64| Pos2::new(o.x + (z * k) as f32, o.y + (y * k) as f32);
        let ink = Stroke::new(1.5, Color32::from_gray(30));
        painter.add(egui::Shape::closed_line(
            vec![
                p(0.0, 0.0),
                p(s, 0.0),
                p(s, w.tip_chord_mm),
                p(0.0, w.root_chord_mm),
            ],
            ink,
        ));
        let font = FontId::proportional(11.0);
        for g in &self.ribs {
            let z = g.0.z;
            painter.line_segment(
                [p(z, 0.0), p(z, w.chord_at(z))],
                Stroke::new(1.0, Color32::from_gray(150)),
            );
            painter.text(
                p(z, 0.0) - Vec2::new(0.0, 4.0),
                Align2::CENTER_BOTTOM,
                format!("Н{}", g.0.index + 1),
                font.clone(),
                Color32::from_gray(90),
            );
        }
        let colors = [
            Color32::from_rgb(192, 57, 43),
            Color32::from_rgb(39, 120, 70),
            Color32::from_rgb(130, 60, 160),
            Color32::from_rgb(200, 120, 20),
        ];
        for (j, sp) in self.project.spars.iter().enumerate() {
            let c = colors[j % colors.len()];
            let a = p(sp.z_start, sp.chord_pos * w.chord_at(sp.z_start));
            let b = p(sp.z_end, sp.chord_pos * w.chord_at(sp.z_end));
            let sel = self.sel == Sel::Spar(j);
            let width = if sp.joiner { 4.0 } else { 2.5 } + if sel { 2.0 } else { 0.0 };
            if sp.status == Status::Candidate {
                for sh in egui::Shape::dashed_line(&[a, b], Stroke::new(width, c), 8.0, 5.0) {
                    painter.add(sh);
                }
            } else {
                painter.line_segment([a, b], Stroke::new(width, c));
            }
        }
        painter.text(
            p(0.0, w.root_chord_mm) + Vec2::new(0.0, 6.0),
            Align2::LEFT_TOP,
            "корень",
            font.clone(),
            Color32::from_gray(90),
        );
        painter.text(
            p(s, w.tip_chord_mm) + Vec2::new(0.0, 6.0),
            Align2::RIGHT_TOP,
            "конец",
            font.clone(),
            Color32::from_gray(90),
        );
        painter.text(
            rect.right_top() + Vec2::new(-8.0, 8.0),
            Align2::RIGHT_TOP,
            "передняя кромка сверху · пунктир — кандидат",
            font,
            Color32::from_gray(120),
        );
    }

    // ───────────────────────────── Нервюры ─────────────────────────────

    fn ribs_overview(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Нервюры консоли");
        if self.ribs.is_empty() {
            ui.label("Нет нервюр: исправьте ошибки в исходных данных.");
            return;
        }
        let mat = self.rib_material().cloned();
        egui::Grid::new("ribs_tbl").striped(true).show(ui, |ui| {
            for h in [
                "№",
                "z, мм",
                "Хорда, мм",
                "Сила, Н",
                "Отверстия",
                "Масса до, г",
                "после, г",
                "Состояние",
            ] {
                ui.strong(h);
            }
            ui.end_row();
            for i in 0..self.ribs.len() {
                let (g, _) = &self.ribs[i];
                ui.label(format!("{}", g.index + 1));
                ui.label(format!("{:.0}", g.z));
                ui.label(format!("{:.0}", g.chord));
                ui.label(format!("{:.2}", g.force_n));
                ui.label(
                    g.holes
                        .iter()
                        .map(hole_label)
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                let st = &self.rib_states[i];
                let fresh = self.is_fresh(i);
                match (&st.result, fresh) {
                    (Some(r), true) => {
                        ui.label(format!("{:.2}", r.mass_before_g));
                        ui.label(format!("{:.2}", r.mass_after_g));
                        ui.label(if r.converged {
                            "готово"
                        } else {
                            "промежуточный"
                        });
                    }
                    (Some(_), false) => {
                        ui.label("—");
                        ui.label("—");
                        ui.label("устарело");
                    }
                    _ => {
                        let before = mat.as_ref().map(|m| {
                            (g.outline_area() - g.holes.iter().map(|h| h.area()).sum::<f64>())
                                * self.project.ribs.thickness_mm
                                * m.density_g_cm3
                                / 1000.0
                        });
                        ui.label(before.map_or("—".into(), |b| format!("{b:.2}")));
                        ui.label("—");
                        ui.label(if st.error.is_some() {
                            "ошибка"
                        } else {
                            "не считалось"
                        });
                    }
                }
                ui.end_row();
            }
        });
        let total: Option<(f64, f64)> = (0..self.ribs.len())
            .map(|i| {
                let q = self.ribs[i].0.quantity as f64;
                self.rib_states[i]
                    .result
                    .as_ref()
                    .filter(|_| self.is_fresh(i))
                    .map(|r| (r.mass_before_g * q, r.mass_after_g * q))
            })
            .try_fold((0.0, 0.0), |acc, x| x.map(|(a, b)| (acc.0 + a, acc.1 + b)));
        if let Some((b, a)) = total {
            ui.strong(format!("Все нервюры самолёта: {b:.1} → {a:.1} г"));
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new("Оптимизировать все"))
                .clicked()
            {
                let all = (0..self.ribs.len()).collect();
                self.start(ctx, all);
            }
            if ui.button("Экспорт DXF + SVG").clicked() {
                self.export();
            }
        });
        let all: Vec<String> = self.ribs.iter().flat_map(|r| r.1.clone()).collect();
        warnings(ui, &all);
    }

    fn rib_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, i: usize) {
        if i >= self.ribs.len() {
            return;
        }
        let (g, warns) = self.ribs[i].clone();
        ui.horizontal(|ui| {
            for k in 0..self.ribs.len() {
                if ui.selectable_label(k == i, format!("Н{}", k + 1)).clicked() {
                    self.sel = Sel::Rib(k);
                }
            }
        });
        ui.heading(format!(
            "Нервюра {} · z = {:.0} мм · хорда {:.0} мм",
            g.index + 1,
            g.z,
            g.chord
        ));
        ui.label(format!(
            "Расчётная сила {:.2} Н · {} · {} шт. в самолёте",
            g.force_n,
            g.holes
                .iter()
                .map(hole_label)
                .collect::<Vec<_>>()
                .join(", "),
            g.quantity
        ));
        let busy = self.job.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("Оптимизировать нервюру"))
                .clicked()
            {
                self.start(ctx, vec![i]);
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Оптимизировать все"))
                .clicked()
            {
                let all = (0..self.ribs.len()).collect();
                self.start(ctx, all);
            }
            if ui
                .add_enabled(busy, egui::Button::new("Остановить"))
                .clicked()
                && let Some(j) = &self.job
            {
                j.cancel.store(true, Ordering::Relaxed);
            }
        });
        let setup = self.rib_setup(i);
        let fresh = self.is_fresh(i);
        if let Err(e) = &setup {
            ui.colored_label(Color32::from_rgb(200, 40, 40), e);
        }
        // Холст.
        let width = ui.available_width().max(300.0);
        let (ymin, ymax) = g
            .outline
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q.1), b.max(q.1)));
        let m = 16.0_f64;
        let k = (width as f64 - 2.0 * m) / g.chord;
        let height = ((ymax - ymin) * k + 2.0 * m) as f32;
        let (resp, painter) = ui.allocate_painter(Vec2::new(width, height), egui::Sense::hover());
        let rect = resp.rect;
        painter.rect_filled(rect, 4.0, Color32::WHITE);
        let p = |x: f64, y: f64| {
            Pos2::new(
                rect.min.x + (m + x * k) as f32,
                rect.min.y + (m + (ymax - y) * k) as f32,
            )
        };
        // Поле плотностей или запретные зоны.
        let st = &mut self.rib_states[i];
        let img = match (&st.density, &setup) {
            (Some((grid, d)), Ok(s)) if fresh && d.len() == s.problem.cells.len() => {
                Some((*grid, density_image(grid, d, &s.problem.cells)))
            }
            (_, Ok(s)) => Some((s.grid, zones_image(&s.grid, &s.problem.cells))),
            _ => None,
        };
        if let Some((grid, image)) = img {
            if st.texture.is_none() || st.texture_dirty || !fresh {
                match &mut st.texture {
                    Some(t) => t.set(image, egui::TextureOptions::NEAREST),
                    None => {
                        st.texture = Some(ctx.load_texture(
                            format!("rib{i}"),
                            image,
                            egui::TextureOptions::NEAREST,
                        ))
                    }
                }
                st.texture_dirty = false;
            }
            if let Some(t) = &st.texture {
                let r = Rect::from_min_max(
                    p(grid.x0, grid.ytop),
                    p(
                        grid.x0 + grid.nelx as f64 * grid.h,
                        grid.ytop - grid.nely as f64 * grid.h,
                    ),
                );
                painter.image(
                    t.id(),
                    r,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        painter.add(egui::Shape::closed_line(
            g.outline.iter().map(|&(x, y)| p(x, y)).collect(),
            Stroke::new(1.5, Color32::from_gray(20)),
        ));
        for h in &g.holes {
            match h.shape {
                HoleShape::Circle { d } => {
                    painter.circle_stroke(
                        p(h.center.0, h.center.1),
                        (d / 2.0 * k) as f32,
                        Stroke::new(1.5, Color32::from_rgb(192, 57, 43)),
                    );
                }
                HoleShape::Rect { w, h: hh } => {
                    let r = Rect::from_two_pos(
                        p(h.center.0 - w / 2.0, h.center.1 - hh / 2.0),
                        p(h.center.0 + w / 2.0, h.center.1 + hh / 2.0),
                    );
                    painter.rect_stroke(
                        r,
                        0.0,
                        Stroke::new(1.5, Color32::from_rgb(192, 57, 43)),
                        egui::StrokeKind::Middle,
                    );
                }
            }
        }
        if fresh && let Some(r) = &st.result {
            for pk in &r.pockets {
                painter.add(egui::Shape::closed_line(
                    pk.iter().map(|&(x, y)| p(x, y)).collect(),
                    Stroke::new(1.6, Color32::from_rgb(31, 95, 191)),
                ));
            }
        }
        // Итоги.
        if fresh && let Some(r) = &st.result {
            ui.horizontal_wrapped(|ui| {
                stat(ui, "Масса до", format!("{:.2} г", r.mass_before_g));
                stat(ui, "после", format!("{:.2} г", r.mass_after_g));
                stat(
                    ui,
                    "Облегчение",
                    format!("{:.0} %", 100.0 * (1.0 - r.mass_after_g / r.mass_before_g)),
                );
                stat(ui, "Вырезов", format!("{}", r.pockets.len()));
                stat(
                    ui,
                    "Итераций",
                    format!(
                        "{}{}",
                        r.iterations,
                        if r.converged {
                            ""
                        } else {
                            " (не сошлось)"
                        }
                    ),
                );
            });
            warnings(ui, &r.warnings);
        } else if st.result.is_some() {
            ui.colored_label(
                Color32::from_rgb(200, 120, 20),
                "Исходные данные изменились — результат устарел, запустите оптимизацию снова.",
            );
        } else if st.density.is_none() {
            ui.small("Тёмное — сплошные зоны (кромка, кольца), голубое — область оптимизации.");
        }
        if let Some(e) = &st.error {
            ui.colored_label(Color32::from_rgb(200, 40, 40), e);
        }
        if !st.history.is_empty() {
            let h = st.history.clone();
            Plot::new(("hist", i))
                .height(140.0)
                .allow_scroll(false)
                .x_axis_label("итерация")
                .y_axis_label("податливость, Н·мм")
                .show(ui, |pu| {
                    pu.line(Line::new("податливость", h).color(Color32::from_rgb(31, 95, 191)))
                });
        }
        warnings(ui, &warns);
    }
}

// ───────────────────────────── Мелочи ─────────────────────────────

fn grid(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut egui::Ui),
) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, add);
}

fn num(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    suffix: &str,
) {
    ui.label(label);
    ui.add(
        egui::DragValue::new(v)
            .range(range)
            .speed(speed)
            .suffix(suffix.to_string())
            .max_decimals(3),
    );
    ui.end_row();
}

fn material_combo(ui: &mut egui::Ui, id: &str, value: &mut String, names: &[String]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.clone())
        .width(190.0)
        .show_ui(ui, |ui| {
            for n in names {
                ui.selectable_value(value, n.clone(), n);
            }
        });
}

fn dist_name(d: Distribution) -> &'static str {
    match d {
        Distribution::Schrenk => "Шренк",
        Distribution::Elliptic => "Эллиптическое",
        Distribution::Trapezoidal => "По хорде",
        Distribution::Uniform => "Равномерное",
    }
}

fn hole_label(h: &cadplus_rib::geom::Hole) -> String {
    match h.shape {
        HoleShape::Circle { d } => format!("Ø{d:.1}"),
        HoleShape::Rect { w, h } => format!("{w:.1}×{h:.1}"),
    }
}

fn margin_text(m: f64) -> String {
    if m.is_finite() {
        format!("{m:.1}")
    } else {
        "∞".into()
    }
}

fn margin_label(ui: &mut egui::Ui, m: f64) {
    let c = if m < 1.0 {
        Color32::from_rgb(200, 40, 40)
    } else if m < 1.5 {
        Color32::from_rgb(200, 120, 20)
    } else {
        Color32::from_rgb(39, 120, 70)
    };
    ui.colored_label(c, margin_text(m));
}

fn stat(ui: &mut egui::Ui, label: &str, value: String) {
    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.small(label);
            ui.strong(value);
        });
    });
}

fn warnings(ui: &mut egui::Ui, list: &[String]) {
    for w in list {
        ui.label(RichText::new(format!("! {w}")).color(Color32::from_rgb(190, 110, 0)));
    }
}

fn plot(ui: &mut egui::Ui, id: &str, title: &str, lines: Vec<(&str, Vec<[f64; 2]>, Color32)>) {
    ui.small(title);
    Plot::new(id)
        .height(170.0)
        .allow_scroll(false)
        .x_axis_label("z от корня, мм")
        .legend(Legend::default())
        .show(ui, |pu| {
            for (name, pts, c) in lines {
                pu.line(Line::new(name.to_string(), pts).color(c).width(2.0));
            }
        });
}

fn density_image(g: &Grid, d: &[f64], cells: &[Cell]) -> egui::ColorImage {
    let mut px = Vec::with_capacity(g.nelx * g.nely);
    for j in 0..g.nely {
        for i in 0..g.nelx {
            let e = i * g.nely + j;
            px.push(match cells[e] {
                Cell::Void => Color32::WHITE,
                Cell::Solid => Color32::from_gray(70),
                Cell::Active => {
                    let v = (255.0 * (1.0 - d[e].clamp(0.0, 1.0) * 0.75)) as u8;
                    Color32::from_gray(v)
                }
            });
        }
    }
    egui::ColorImage::new([g.nelx, g.nely], px)
}

fn zones_image(g: &Grid, cells: &[Cell]) -> egui::ColorImage {
    let mut px = Vec::with_capacity(g.nelx * g.nely);
    for j in 0..g.nely {
        for i in 0..g.nelx {
            px.push(match cells[i * g.nely + j] {
                Cell::Void => Color32::WHITE,
                Cell::Solid => Color32::from_gray(90),
                Cell::Active => Color32::from_rgb(200, 220, 245),
            });
        }
    }
    egui::ColorImage::new([g.nelx, g.nely], px)
}
