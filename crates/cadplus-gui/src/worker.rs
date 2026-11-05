//! Оптимизация нервюр в фоновом потоке: окно не зависает (NFR-01), отмена — флагом (FR-10).
//!
//! Это временное решение этапов 1–3. По спецификации (AD-01, AD-04) решатель переедет
//! в отдельный процесс с gRPC — на этапе 4.

use cadplus_rib::setup::RibSetup;
use cadplus_rib::{RibResult, RibSettings, finalize, optimize};
use cadplus_topopt::Control;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Сообщение из фонового потока.
pub enum Msg {
    /// Началась нервюра.
    Started {
        /// Номер нервюры.
        rib: usize,
    },
    /// Итерация.
    Progress {
        /// Номер нервюры.
        rib: usize,
        /// Номер итерации.
        iter: usize,
        /// Податливость.
        compliance: f64,
        /// Плотности.
        density: Vec<f64>,
    },
    /// Готово.
    Done {
        /// Номер нервюры.
        rib: usize,
        /// Результат.
        result: Box<RibResult>,
        /// Итоговые плотности.
        density: Vec<f64>,
    },
    /// Ошибка.
    Failed {
        /// Номер нервюры.
        rib: usize,
        /// Текст.
        error: String,
    },
    /// Очередь закончилась.
    Finished,
}

/// Задание для потока.
pub struct Task {
    /// Номер нервюры.
    pub rib: usize,
    /// Подготовленная задача.
    pub setup: RibSetup,
}

/// Работающий поток.
pub struct Job {
    /// Флаг отмены.
    pub cancel: Arc<AtomicBool>,
    /// Сообщения.
    pub rx: Receiver<Msg>,
    /// Сколько нервюр в очереди всего.
    pub total: usize,
    /// Сколько закончено.
    pub done: usize,
    /// Текущая нервюра и итерация.
    pub current: Option<(usize, usize)>,
}

/// Запустить очередь нервюр.
pub fn spawn(
    tasks: Vec<Task>,
    settings: RibSettings,
    thickness_mm: f64,
    density_g_cm3: f64,
    repaint: impl Fn() + Send + 'static,
) -> Job {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = channel();
    let total = tasks.len();
    let stop = cancel.clone();
    std::thread::Builder::new()
        .name("cadplus-solver".into())
        .spawn(move || {
            run(
                tasks,
                settings,
                thickness_mm,
                density_g_cm3,
                stop,
                tx,
                repaint,
            )
        })
        .expect("поток решателя запускается");
    Job {
        cancel,
        rx,
        total,
        done: 0,
        current: None,
    }
}

fn run(
    tasks: Vec<Task>,
    settings: RibSettings,
    thickness_mm: f64,
    density_g_cm3: f64,
    stop: Arc<AtomicBool>,
    tx: Sender<Msg>,
    repaint: impl Fn(),
) {
    for t in tasks {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let _ = tx.send(Msg::Started { rib: t.rib });
        repaint();
        let mut last_sent = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let res = optimize(&t.setup, &settings, |it, dens| {
            // Не чаще 20 раз в секунду: окну больше не нужно.
            if last_sent.elapsed().as_millis() >= 50 {
                last_sent = std::time::Instant::now();
                let _ = tx.send(Msg::Progress {
                    rib: t.rib,
                    iter: it.iter,
                    compliance: it.compliance,
                    density: dens.to_vec(),
                });
                repaint();
            }
            if stop.load(Ordering::Relaxed) {
                Control::Stop
            } else {
                Control::Continue
            }
        });
        match res {
            Ok(sol) => {
                let r = finalize(
                    &t.setup,
                    &sol.density,
                    Some(&sol),
                    &settings,
                    thickness_mm,
                    density_g_cm3,
                );
                let _ = tx.send(Msg::Done {
                    rib: t.rib,
                    result: Box::new(r),
                    density: sol.density,
                });
            }
            Err(e) => {
                let _ = tx.send(Msg::Failed {
                    rib: t.rib,
                    error: e,
                });
            }
        }
        repaint();
    }
    let _ = tx.send(Msg::Finished);
    repaint();
}
