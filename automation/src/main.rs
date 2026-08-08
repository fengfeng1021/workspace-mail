// Workspace Mail 自動化 — 主程式（egui 介面）
mod browser;
mod fingerprint;
mod firestore;

use browser::{FieldAction, TaskConfig};
use fingerprint::FingerprintStore;
use firestore::{Account, FirebaseConfig, FirestoreClient};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const CONFIG_FILE: &str = "wm-automation-config.json";
const FINGERPRINT_FILE: &str = "wm-fingerprints.json";
/// 持久 Chrome profile 儲存位置：Workspace-Mail 專案目錄下（隨專案走，統一管理）
const PROFILE_ROOT: &str = r"D:\Desktop\App\Workspace-Mail\wm-profiles";

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct AppConfig {
    firebase: FirebaseConfig,
    chrome_path: String,
    domain: String,
    url: String,
    fields: Vec<FieldAction>,
    submit_selector: String,
    wait_after_ms: u64,
    step_delay_ms: u64,
    parallel: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            firebase: FirebaseConfig {
                api_key: "AIzaSyBJ_TWOMc7IHhI-5at21Aru0Bf1U-qpFBM".into(),
                project_id: "mailhub-d64e2".into(),
                email: "wangjunfeng350@gmail.com".into(),
                password: "WJF7374520wjf".into(),
            },
            chrome_path: r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
            domain: "".into(),
            url: "".into(),
            fields: vec![
                FieldAction { selector: "#email".into(), value: "{email}".into() },
                FieldAction { selector: "#password".into(), value: "{password}".into() },
            ],
            submit_selector: "button[type=submit]".into(),
            wait_after_ms: 3000,
            step_delay_ms: 500,
            parallel: 5,
        }
    }
}

fn config_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    dir.join(CONFIG_FILE)
}

fn fingerprint_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    dir.join(FINGERPRINT_FILE)
}

/// 帳號對應的持久 profile 目錄（一次登入永久有效）
fn profile_dir_for(email: &str) -> PathBuf {
    let local = email.split('@').next().unwrap_or("account");
    PathBuf::from(PROFILE_ROOT).join(local)
}

/// 載入中文字體（egui 預設字體不含中文，會顯示為方塊）
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // 優先 Microsoft JhengHei（正黑體，繁中），fallback 雅黑/黑體
    let candidates = [
        r"C:\Windows\Fonts\msjh.ttc",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
    ];
    for path in candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts
                .font_data
                .insert("cjk".to_owned(), egui::FontData::from_owned(data));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.get_mut(&family).unwrap().push("cjk".to_owned());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

struct WmApp {
    cfg: AppConfig,
    fingerprints: FingerprintStore,
    domains: Vec<String>,
    accounts: Vec<Account>,
    logs: Vec<String>,
    status: String,
    running: bool,
    stopping: Arc<AtomicBool>,
    done_count: usize,
    total_count: usize,
    rt: tokio::runtime::Runtime,
    log_tx: tokio::sync::mpsc::UnboundedSender<String>,
    log_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
}

impl WmApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // egui 預設字體不含中文，載入系統正黑體
        setup_fonts(&cc.egui_ctx);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let cfg = std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("建立 tokio runtime 失敗");

        let mut app = Self {
            cfg,
            fingerprints: FingerprintStore::load(&fingerprint_path()),
            domains: Vec::new(),
            accounts: Vec::new(),
            logs: Vec::new(),
            status: "就緒".into(),
            running: false,
            stopping: Arc::new(AtomicBool::new(false)),
            done_count: 0,
            total_count: 0,
            rt,
            log_tx: tx,
            log_rx: rx,
        };
        app.log("啟動完成");
        app
    }

    fn log(&mut self, msg: impl Into<String>) {
        let t = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("{} {}", t, msg.into()));
        if self.logs.len() > 2000 {
            self.logs.drain(0..self.logs.len() - 2000);
        }
    }

    fn drain_logs(&mut self) {
        while let Ok(msg) = self.log_rx.try_recv() {
            // 特殊訊息：進度
            if let Some(n) = msg.strip_prefix("__DONE__") {
                if let Ok(n) = n.parse::<usize>() {
                    self.done_count = n;
                    if self.running && n >= self.total_count {
                        self.running = false;
                        self.status = "✅ 全部完成".into();
                    }
                    continue;
                }
            }
            if let Some(ds) = msg.strip_prefix("__DOMAINS__") {
                if let Ok(list) = serde_json::from_str::<Vec<String>>(ds) {
                    let n = list.len();
                    self.domains = list;
                    self.status = format!("已載入 {} 個域名", n);
                    continue;
                }
            }
            if let Some(as_) = msg.strip_prefix("__ACCOUNTS__") {
                if let Ok(list) = serde_json::from_str::<Vec<Account>>(as_) {
                    let n = list.len();
                    self.accounts = list;
                    self.status = format!("已載入 {} 個帳號", n);
                    continue;
                }
            }
            self.logs.push(msg);
        }
        if self.logs.len() > 2000 {
            self.logs.drain(0..self.logs.len() - 2000);
        }
    }

    fn save_config(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.cfg) {
            let _ = std::fs::write(config_path(), json);
        }
    }

    fn start_task(&mut self) {
        if self.running {
            return;
        }
        if self.accounts.is_empty() {
            self.status = "⚠️ 帳號列表是空的，請先「載入帳號」".into();
            return;
        }
        if self.cfg.url.trim().is_empty() {
            self.status = "⚠️ 請填寫目標網址".into();
            return;
        }
        self.save_config();

        self.running = true;
        self.stopping.store(false, Ordering::SeqCst);
        self.done_count = 0;
        self.total_count = self.accounts.len();
        self.status = format!("開始執行：共 {} 個帳號", self.total_count);

        let accounts = self.accounts.clone();
        let task = TaskConfig {
            url: self.cfg.url.clone(),
            fields: self.cfg.fields.clone(),
            submit_selector: self.cfg.submit_selector.clone(),
            wait_after_ms: self.cfg.wait_after_ms,
            step_delay_ms: self.cfg.step_delay_ms,
        };
        let parallel = self.cfg.parallel.max(1);
        let chrome_path = self.cfg.chrome_path.clone();
        let tx = self.log_tx.clone();
        let stopping = self.stopping.clone();
        let done = Arc::new(AtomicUsize::new(0));

        // 確保每個帳號都有固定指紋（一號一指紋，生成後永久不變）並保存
        let mut fp_store = self.fingerprints.clone();
        for acc in &accounts {
            fp_store.get_or_create(&acc.email);
        }
        let _ = fp_store.save(&fingerprint_path());
        self.log(format!("指紋已就緒：{} 個帳號（時區/語言台灣，其餘固定隨機）", accounts.len()));

        let rt = self.rt.handle().clone();

        // 任務執行者
        {
            let tx = tx.clone();
            let done = done.clone();
            let stopping = stopping.clone();
            let rt_worker = rt.clone();
            rt.spawn(async move {
                let sem = Arc::new(tokio::sync::Semaphore::new(parallel));
                let mut handles = Vec::new();
                for (i, acc) in accounts.into_iter().enumerate() {
                    if stopping.load(Ordering::SeqCst) {
                        let _ = tx.send("⏹ 已停止，不再啟動新實例".into());
                        break;
                    }
                    let permit = sem.clone().acquire_owned().await.unwrap();
                    let tx = tx.clone();
                    let done = done.clone();
                    let chrome_path = chrome_path.clone();
                    let task = task.clone();
                    let fp = fp_store.get_or_create(&acc.email);
                    let profile_dir = profile_dir_for(&acc.email);
                    handles.push(rt_worker.spawn(async move {
                        let _permit = permit;
                        let _ = browser::run_account_task(
                            chrome_path, i, acc.email, acc.password, acc.note, task, fp, profile_dir, tx,
                        )
                        .await;
                        done.fetch_add(1, Ordering::SeqCst);
                    }));
                }
                for h in handles {
                    let _ = h.await;
                }
                let _ = tx.send(format!(
                    "🏁 全部完成（共 {} 個）",
                    done.load(Ordering::SeqCst)
                ));
            });
        }

        // 進度回報者
        {
            let tx = tx.clone();
            let done = done.clone();
            let stopping = stopping.clone();
            rt.spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    let n = done.load(Ordering::SeqCst);
                    let _ = tx.send(format!("__DONE__{}", n));
                    if stopping.load(Ordering::SeqCst) {
                        break;
                    }
                }
            });
        }
    }

    fn spawn_firestore_job(&mut self, job: FirestoreJob) {
        let mut client = FirestoreClient::new(self.cfg.firebase.clone());
        let tx = self.log_tx.clone();
        self.rt.spawn(async move {
            match job {
                FirestoreJob::Domains => {
                    let login = client.login().await;
                    let result = match login {
                        Ok(_) => client.list_domains().await,
                        Err(e) => Err(e),
                    };
                    match result {
                        Ok(ds) => {
                            let _ = tx.send(format!(
                                "__DOMAINS__{}",
                                serde_json::to_string(&ds).unwrap_or_default()
                            ));
                        }
                        Err(e) => {
                            let _ = tx.send(format!("❌ 讀取域名失敗：{}", e));
                        }
                    }
                }
                FirestoreJob::Accounts(domain) => {
                    let login = client.login().await;
                    let result = match login {
                        Ok(_) => client.fetch_accounts(&domain).await,
                        Err(e) => Err(e),
                    };
                    match result {
                        Ok(accs) => {
                            let _ = tx.send(format!(
                                "__ACCOUNTS__{}",
                                serde_json::to_string(&accs).unwrap_or_default()
                            ));
                        }
                        Err(e) => {
                            let _ = tx.send(format!("❌ 讀取帳號失敗：{}", e));
                        }
                    }
                }
            }
        });
    }
}

enum FirestoreJob {
    Domains,
    Accounts(String),
}

impl eframe::App for WmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_logs();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.heading("📮 Workspace Mail 自動化");
            ui.label(format!(
                "狀態：{}　進度：{}/{}",
                self.status, self.done_count, self.total_count
            ));
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("log")
            .resizable(true)
            .default_height(180.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("執行紀錄");
                    if ui.small_button("清空").clicked() {
                        self.logs.clear();
                    }
                });
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    for l in &self.logs {
                        ui.monospace(l);
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // ===== Firebase 設定 =====
                ui.collapsing("🔑 Firebase 設定", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("API Key");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.firebase.api_key).desired_width(320.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Project ID");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.firebase.project_id).desired_width(320.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("登入帳號");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.firebase.email).desired_width(320.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("登入密碼");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.firebase.password).password(true).desired_width(320.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Chrome 路徑");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.chrome_path).desired_width(320.0));
                    });
                });

                ui.separator();

                // ===== 任務設定 =====
                ui.heading("🎯 任務設定");
                ui.horizontal(|ui| {
                    ui.label("域名");
                    egui::ComboBox::from_id_source("domain_sel")
                        .selected_text(if self.cfg.domain.is_empty() { "（未選）" } else { &self.cfg.domain })
                        .show_ui(ui, |ui| {
                            for d in &self.domains {
                                ui.selectable_value(&mut self.cfg.domain, d.clone(), d);
                            }
                        });
                    if ui.button("載入域名").clicked() {
                        self.status = "讀取域名中…".into();
                        self.spawn_firestore_job(FirestoreJob::Domains);
                    }
                    if ui.button("載入帳號").clicked() {
                        if self.cfg.domain.is_empty() {
                            self.status = "⚠️ 請先選擇域名".into();
                        } else {
                            self.status = "讀取帳號中…".into();
                            self.spawn_firestore_job(FirestoreJob::Accounts(self.cfg.domain.clone()));
                        }
                    }
                    ui.label(format!("已載入 {} 個帳號", self.accounts.len()));
                });
                ui.horizontal(|ui| {
                    ui.label("目標網址");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.url).desired_width(480.0));
                });

                ui.add_space(6.0);
                ui.strong("填寫欄位（CSS selector → 內容，支援 {email} {password} {note} 變數）");
                let mut remove_idx: Option<usize> = None;
                for (i, f) in self.cfg.fields.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}:", i + 1));
                        ui.add(egui::TextEdit::singleline(&mut f.selector).desired_width(200.0).hint_text("CSS selector"));
                        ui.add(egui::TextEdit::singleline(&mut f.value).desired_width(260.0).hint_text("要輸入的內容"));
                        if ui.small_button("🗑").clicked() {
                            remove_idx = Some(i);
                        }
                    });
                }
                if let Some(i) = remove_idx {
                    self.cfg.fields.remove(i);
                }
                if ui.small_button("＋ 新增欄位").clicked() {
                    self.cfg
                        .fields
                        .push(FieldAction { selector: String::new(), value: "{email}".into() });
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("送出按鈕 selector");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.submit_selector).desired_width(220.0));
                    ui.label("（留空則不點擊）");
                });
                ui.horizontal(|ui| {
                    ui.label("並行數");
                    ui.add(egui::DragValue::new(&mut self.cfg.parallel).clamp_range(1..=100));
                    ui.label("欄位間隔 ms");
                    ui.add(egui::DragValue::new(&mut self.cfg.wait_after_ms).clamp_range(0..=30000));
                    ui.label("送出後等待 ms");
                    ui.add(egui::DragValue::new(&mut self.cfg.step_delay_ms).clamp_range(0..=30000));
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if !self.running {
                        if ui
                            .button(egui::RichText::new("▶ 開始執行").size(16.0))
                            .clicked()
                        {
                            self.start_task();
                        }
                    } else {
                        if ui
                            .button(egui::RichText::new("⏹ 停止").size(16.0))
                            .clicked()
                        {
                            self.stopping.store(true, Ordering::SeqCst);
                            self.status = "停止中…".into();
                        }
                    }
                    if ui.button("儲存設定").clicked() {
                        self.save_config();
                        self.status = "設定已儲存".into();
                    }
                });
                ui.add_space(8.0);
            });
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 720.0])
            .with_title("Workspace Mail 自動化"),
        ..Default::default()
    };
    eframe::run_native(
        "workspace-mail-automation",
        options,
        Box::new(|cc| Box::new(WmApp::new(cc))),
    )
}
