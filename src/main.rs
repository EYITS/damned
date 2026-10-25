#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod metrics;
mod state;

use metrics::Snapshot;
use slint::{ComponentHandle, Model, ModelRc, SharedPixelBuffer, VecModel};
use state::{AppState, Preferences};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

slint::include_modules!();

fn number(value: f64, digits: usize) -> String {
    format!("{value:.digits$}").replace('.', ",")
}
fn percent(value: Option<f64>) -> String {
    value
        .map(|v| format!("{}%", number(v, 1)))
        .unwrap_or("—".into())
}
fn memory(value: Option<f64>) -> String {
    value
        .map(|v| {
            if v >= 1_073_741_824.0 {
                format!("{} GB", number(v / 1_073_741_824.0, 2))
            } else {
                format!("{} MB", number(v / 1_048_576.0, 0))
            }
        })
        .unwrap_or("—".into())
}
fn rate(value: Option<f64>) -> String {
    value
        .map(|v| number(v / 1_000_000.0, 1))
        .unwrap_or("—".into())
}

fn update_rows(ui: &MonitorWindow, state: &AppState, model: &VecModel<ProcessRow>) {
    let mut rows = Vec::new();
    for group in state::groups(&state.snapshot.processes, &state.prefs) {
        let expanded = state.expanded.contains(&group.key);
        rows.push(ProcessRow {
            key: group.key.clone().into(),
            name: format!("{} ({})", group.name, group.children.len()).into(),
            cpu: percent(group.cpu).into(),
            ram: memory(group.ram).into(),
            gpu: percent(group.gpu).into(),
            network: "↓ —\n↑ —".into(),
            disk: "↓ —\n↑ —".into(),
            child: false,
            expandable: group.children.len() > 1,
            expanded,
        });
        if expanded && group.children.len() > 1 {
            for p in group.children {
                rows.push(ProcessRow {
                    key: format!("{}:{}", p.pid, p.identity).into(),
                    name: format!("{} · {}", p.name, p.pid).into(),
                    cpu: percent(p.cpu).into(),
                    ram: memory(p.ram).into(),
                    gpu: percent(p.gpu).into(),
                    network: "↓ —\n↑ —".into(),
                    disk: "↓ —\n↑ —".into(),
                    child: true,
                    expandable: false,
                    expanded: false,
                });
            }
        }
    }
    // Mutate a persistent model so list updates preserve the viewport.
    let common = model.row_count().min(rows.len());
    for (index, row) in rows.iter().take(common).enumerate() {
        model.set_row_data(index, row.clone());
    }
    while model.row_count() > rows.len() {
        model.remove(model.row_count() - 1);
    }
    for row in rows.into_iter().skip(common) {
        model.push(row);
    }
    ui.set_sort_column(state.prefs.sort);
    ui.set_descending(state.prefs.descending);
    ui.set_headings(ModelRc::new(VecModel::from(vec![
        "Uygulama".into(),
        format!("{}\nCPU", percent(state.snapshot.cpu)).into(),
        format!("{}\nRAM", percent(state.snapshot.ram_percent)).into(),
        format!("{}\nGPU", percent(state.snapshot.gpu)).into(),
        "Ağ\n↓/↑ MB/s".into(),
        "Disk\n↓/↑ MB/s".into(),
    ])));
}

type History = VecDeque<(Instant, [Option<f64>; 5])>;

fn graph(history: &History, column: usize, maximum: f64) -> slint::Image {
    let (width, height) = (180u32, 36u32);
    let mut buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let pixels = buffer.make_mut_slice();
    let now = Instant::now();
    let mut previous: Option<(i32, i32)> = None;
    for (time, values) in history {
        let Some(value) = values[column] else {
            previous = None;
            continue;
        };
        let x = ((1.0 - now.duration_since(*time).as_secs_f64() / 60.0).clamp(0.0, 1.0)
            * (width - 1) as f64) as i32;
        let y = ((1.0 - (value / maximum).clamp(0.0, 1.0)) * (height - 3) as f64) as i32 + 1;
        if let Some((px, py)) = previous {
            let steps = (x - px).abs().max((y - py).abs()).max(1);
            for step in 0..=steps {
                let xx = px + (x - px) * step / steps;
                let yy = py + (y - py) * step / steps;
                if xx >= 0 && xx < width as i32 && yy >= 0 && yy < height as i32 {
                    pixels[(yy as u32 * width + xx as u32) as usize] =
                        slint::Rgba8Pixel::new(205, 197, 212, 255);
                }
            }
        }
        previous = Some((x, y));
    }
    slint::Image::from_rgba8(buffer)
}

fn update_cards(ui: &MonitorWindow, sample: &Snapshot, history: &History) {
    let draw = matches!(ui.get_mode(), 0 | 3);
    let make_graph = |index| {
        if !draw {
            return slint::Image::default();
        }
        let max = if index < 3 {
            100.0
        } else {
            history
                .iter()
                .filter_map(|(_, v)| v[index])
                .fold(1_000_000.0, f64::max)
                * 1.1
        };
        graph(history, index, max)
    };
    let cards = vec![
        ResourceCard { label: "CPU".into(), value: percent(sample.cpu).into(), detail: "".into(), explanation: "GetSystemTimes: toplam işlemci kapasitesi; ilk örnekte hız hesaplanamaz.".into(), graph: make_graph(0) },
        ResourceCard { label: "RAM".into(), value: percent(sample.ram_percent).into(), detail: format!("{} / {}", memory(sample.ram_used), memory(sample.ram_total)).into(), explanation: "GlobalMemoryStatusEx: kullanılan fiziksel bellek. İşlem çalışma kümelerinin toplamıyla eşit olması beklenmez.".into(), graph: make_graph(1) },
        ResourceCard { label: "GPU".into(), value: percent(sample.gpu).into(), detail: format!("{} / — · — MHz", memory(sample.vram_used)).into(), explanation: "PDH GPU Engine: tüm GPU'ların en yoğun motoru. VRAM kullanımı adaptörlerin toplamıdır. Toplam VRAM ve saat hızı uygulanmadı; sürücü sayaçları yoksa —.".into(), graph: make_graph(2) },
        ResourceCard { label: "AĞ".into(), value: format!("↓ {} MB/s", rate(sample.download)).into(), detail: format!("↑ {} MB/s", rate(sample.upload)).into(), explanation: "GetIfTable2: bağlı donanım arayüzlerinin toplamı; VPN ve loopback hariç. MB/s = 1.000.000 bayt/s.".into(), graph: make_graph(3) },
        ResourceCard { label: "DİSK".into(), value: format!("↓ {} MB/s", rate(sample.read)).into(), detail: format!("↑ {} MB/s", rate(sample.write)).into(), explanation: "PDH PhysicalDisk(_Total): tüm fiziksel disklerin okuma/yazma toplamı. Sayaç erişilemiyorsa —; yönetici izni otomatik istenmez.".into(), graph: make_graph(4) },
    ];
    ui.set_cards(ModelRc::new(VecModel::from(cards)));
}

fn own_window() -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe extern "system" fn visit(
        hwnd: windows_sys::Win32::Foundation::HWND,
        data: isize,
    ) -> i32 {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut title = [0u16; 64];
        let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        if pid == std::process::id()
            && GetWindow(hwnd, GW_OWNER).is_null()
            && len > 0
            && String::from_utf16_lossy(&title[..len as usize]) == "damned.exe"
        {
            *(data as *mut windows_sys::Win32::Foundation::HWND) = hwnd;
            return 0;
        }
        1
    }
    unsafe {
        let mut hwnd = std::ptr::null_mut();
        EnumWindows(Some(visit), &mut hwnd as *mut _ as isize);
        hwnd
    }
}

fn sampling_slow() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let hwnd = own_window();
    !hwnd.is_null() && unsafe { IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0 }
}

fn fit_window(ui: &MonitorWindow, state: &AppState) {
    use windows_sys::Win32::Graphics::Gdi::*;
    let scale = state.prefs.scale as f32 / 100.0;
    let hwnd = own_window();
    let mut work_height = 850.0;
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            work_height =
                (info.rcWork.bottom - info.rcWork.top) as f32 / ui.window().scale_factor() - 60.0;
        }
    }
    let summary = match state.prefs.mode {
        0 | 3 => 156.0,
        2 => 104.0,
        _ => 0.0,
    };
    let rows = if state.prefs.mode == 0 {
        0.0
    } else {
        state::groups(&state.snapshot.processes, &state.prefs)
            .iter()
            .map(|g| {
                1 + if state.expanded.contains(&g.key) && g.children.len() > 1 {
                    g.children.len()
                } else {
                    0
                }
            })
            .sum::<usize>()
            .clamp(1, 12) as f32
            * 48.0
            + 52.0
    };
    let height = ((46.0 + summary + rows) * scale)
        .min(work_height)
        .max(100.0);
    ui.set_content_height(height);
    ui.window()
        .set_size(slint::LogicalSize::new(900.0 * scale, height));
}

fn diagnose(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut collector = metrics::Collector::new();
    let mut samples = Vec::new();
    for _ in 0..6 {
        let s = collector.sample();
        samples.push(serde_json::json!({
            "cpu_percent": s.cpu, "ram_percent": s.ram_percent,
            "ram_used_bytes": s.ram_used, "ram_total_bytes": s.ram_total,
            "gpu_busiest_engine_percent": s.gpu, "vram_used_bytes": s.vram_used,
            "download_bytes_per_second": s.download, "upload_bytes_per_second": s.upload,
            "disk_read_bytes_per_second": s.read, "disk_write_bytes_per_second": s.write,
            "process_count": s.processes.len(),
            "self": s.processes.iter().find(|p| p.pid == std::process::id()).map(|p|
                serde_json::json!({"cpu_percent": p.cpu, "working_set_bytes": p.ram})),
        }));
        std::thread::sleep(Duration::from_secs(1));
    }
    std::fs::write(path, serde_json::to_vec_pretty(&samples)?)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--diagnose") {
        return diagnose(
            args.get(2)
                .map(String::as_str)
                .unwrap_or("damned-metrics.json"),
        );
    }
    let ui = MonitorWindow::new()?;
    let state = Rc::new(RefCell::new(AppState {
        prefs: Preferences::load(),
        expanded: HashSet::new(),
        snapshot: Snapshot::default(),
    }));
    let rows = Rc::new(VecModel::default());
    ui.set_rows(ModelRc::from(rows.clone()));
    ui.set_mode(state.borrow().prefs.mode);
    ui.set_scale(state.borrow().prefs.scale as f32 / 100.0);
    let history = Rc::new(RefCell::new(History::new()));
    update_rows(&ui, &state.borrow(), &rows);
    update_cards(&ui, &state.borrow().snapshot, &history.borrow());

    {
        let (weak, state, history) = (ui.as_weak(), state.clone(), history.clone());
        ui.on_choose_mode(move |mode| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.prefs.mode = mode.clamp(0, 3);
            ui.set_mode(state.prefs.mode);
            update_cards(&ui, &state.snapshot, &history.borrow());
            fit_window(&ui, &state);
            if let Err(e) = state.prefs.save() {
                ui.set_hint(format!("Ayar kaydedilemedi: {e}").into());
            }
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_change_scale(move |delta| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.prefs.scale = if delta == 0 {
                90
            } else {
                (state.prefs.scale + delta).clamp(70, 130)
            };
            ui.set_scale(state.prefs.scale as f32 / 100.0);
            fit_window(&ui, &state);
            if let Err(e) = state.prefs.save() {
                ui.set_hint(format!("Ayar kaydedilemedi: {e}").into());
            }
        });
    }
    {
        let (weak, state, rows) = (ui.as_weak(), state.clone(), rows.clone());
        ui.on_sort(move |column| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            if state.prefs.sort == column {
                state.prefs.descending = !state.prefs.descending;
            } else {
                state.prefs.sort = column;
                state.prefs.descending = true;
            }
            update_rows(&ui, &state, &rows);
            if let Err(e) = state.prefs.save() {
                ui.set_hint(format!("Ayar kaydedilemedi: {e}").into());
            }
        });
    }
    {
        let (weak, state, rows) = (ui.as_weak(), state.clone(), rows.clone());
        ui.on_toggle(move |key| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            if !state.expanded.remove(key.as_str()) {
                state.expanded.insert(key.to_string());
            }
            update_rows(&ui, &state, &rows);
            fit_window(&ui, &state);
        });
    }

    let latest = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let (latest, stop) = (latest.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut collector = metrics::Collector::new();
            let mut last = Instant::now() - Duration::from_secs(10);
            while !stop.load(Ordering::Relaxed) {
                let period = if sampling_slow() { 5 } else { 1 };
                if last.elapsed() >= Duration::from_secs(period) {
                    let snapshot = collector.sample();
                    *latest.lock().unwrap() = Some(snapshot);
                    last = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    };
    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    let mut last_row_count = None;
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(200),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if sampling_slow() {
                return;
            }
            let Some(snapshot) = latest.lock().unwrap().take() else {
                return;
            };
            let now = Instant::now();
            let mut history = history.borrow_mut();
            history.push_back((
                now,
                [
                    snapshot.cpu,
                    snapshot.ram_percent,
                    snapshot.gpu,
                    snapshot.download.zip(snapshot.upload).map(|(a, b)| a + b),
                    snapshot.read.zip(snapshot.write).map(|(a, b)| a + b),
                ],
            ));
            while history
                .front()
                .is_some_and(|(time, _)| now.duration_since(*time).as_secs() >= 60)
                || history.len() > 61
            {
                history.pop_front();
            }
            let mut state = state.borrow_mut();
            state.snapshot = snapshot;
            update_rows(&ui, &state, &rows);
            update_cards(&ui, &state.snapshot, &history);
            let row_count = rows.row_count().min(12);
            if last_row_count != Some(row_count) {
                fit_window(&ui, &state);
                last_row_count = Some(row_count);
            }
        },
    );
    let result = ui.run();
    stop.store(true, Ordering::Relaxed);
    let _ = worker.join();
    result?;
    Ok(())
}
