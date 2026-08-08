// Workspace Mail 自動化 — 主程式（egui 介面）
mod browser;
mod fingerprint;
mod firestore;
mod usage;

use browser::{FieldAction, TaskConfig};
use fingerprint::FingerprintStore;
use firestore::{Account, FirebaseConfig, FirestoreClient};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use usage::{AccountStatus, UsageStore};

const CONFIG_FILE: &str = "wm-automation-config.json";
const FINGERPRINT_FILE: &str = "wm-fingerprints.json";
const USAGE_FILE: &str = "wm-account-usage.json";
/// 持久 Chrome profile 儲存位置：Workspace-Mail 專案目錄下（隨專案走，統一管理）
const PROFILE_ROOT: &str = r"D:\Desktop\App\Workspace-Mail\wm-profiles";

/// 色彩系統（品牌統一：一處定義，全 UI 使用）
mod colors {
    use egui::Color32;
    // 品牌主色
    pub const PRIMARY: Color32 = Color32::from_rgb(79, 140, 255);
    // 語義色
    pub const SUCCESS: Color32 = Color32::from_rgb(80, 220, 120);
    pub const WARNING: Color32 = Color32::from_rgb(240, 170, 60);
    pub const ERROR: Color32 = Color32::from_rgb(235, 87, 87);
    pub const INFO: Color32 = Color32::from_rgb(120, 170, 230);
    // 中性色（深色主題）
    pub const BG: Color32 = Color32::from_rgb(20, 24, 34);
    pub const PANEL: Color32 = Color32::from_rgb(27, 32, 45);
    pub const PANEL_ALT: Color32 = Color32::from_rgb(34, 40, 56);
    pub const BORDER: Color32 = Color32::from_rgb(48, 56, 78);
    pub const TEXT: Color32 = Color32::from_rgb(220, 225, 235);
    pub const TEXT_WEAK: Color32 = Color32::from_rgb(150, 158, 175);
    // 間距系統（8px 節奏）
    pub const SPACE: f32 = 8.0;
}

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
    /// 冷卻開關
    cooldown_enabled: bool,
    /// 冷卻分鐘數（自訂）
    cooldown_minutes: u64,
    /// 執行成功後自動標記已使用
    auto_mark_used: bool,
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
            cooldown_enabled: true,
            cooldown_minutes: 30,
            auto_mark_used: true,
        }
    }
}

fn config_path() -> PathBuf {
    exe_dir().join(CONFIG_FILE)
}
fn fingerprint_path() -> PathBuf {
    exe_dir().join(FINGERPRINT_FILE)
}
fn usage_path() -> PathBuf {
    exe_dir().join(USAGE_FILE)
}
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_default()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
}

/// 帳號對應的持久 profile 目錄（一次登入永久有效）
fn profile_dir_for(email: &str) -> PathBuf {
    let local = email.split('@').next().unwrap_or("account");
    PathBuf::from(PROFILE_ROOT).join(local)
}

/// 載入中文字體（egui 預設字體不含中文，會顯示為方塊）
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
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

/// 帳號篩選
#[derive(Clone, Copy, PartialEq, Eq)]
enum AccFilter {
    All,
    Available,
    Cooling,
}

struct WmApp {
    cfg: AppConfig,
    fingerprints: FingerprintStore,
    usage: UsageStore,
    domains: Vec<String>,
    accounts: Vec<Account>,
    selected: HashSet<String>,
    search: String,
    filter: AccFilter,
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
            usage: UsageStore::load(&usage_path()),
            domains: Vec::new(),
            accounts: Vec::new(),
            selected: HashSet::new(),
            search: String::new(),
            filter: AccFilter::All,
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
            if let Some(n) = msg.strip_prefix("__DONE__") {
                if let Ok(n) = n.parse::<usize>() {
                    self.done_count = n;
                    if self.running && n >= self.total_count {
                        self.running = false;
                        self.status = "✅ 全部完成".into();
                    }
                }
                continue;
            }
            if let Some(email) = msg.strip_prefix("__USED__") {
                // 執行完成自動標記已使用
                if self.cfg.auto_mark_used {
                    self.usage.mark_used(email);
                    let _ = self.usage.save(&usage_path());
                }
                continue;
            }
            if let Some(ds) = msg.strip_prefix("__DOMAINS__") {
                if let Ok(list) = serde_json::from_str::<Vec<String>>(ds) {
                    let n = list.len();
                    self.domains = list;
                    self.status = format!("已載入 {} 個域名", n);
                }
                continue;
            }
            if let Some(as_) = msg.strip_prefix("__ACCOUNTS__") {
                if let Ok(list) = serde_json::from_str::<Vec<Account>>(as_) {
                    let n = list.len();
                    self.accounts = list;
                    for acc in &self.accounts {
                        self.usage.ensure(&acc.email);
                    }
                    let _ = self.usage.save(&usage_path());
                    self.status = format!("已載入 {} 個帳號（勾選要執行的）", n);
                }
                continue;
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

    fn cooldown_min(&self) -> u64 {
        if self.cfg.cooldown_enabled { self.cfg.cooldown_minutes } else { 0 }
    }

    fn is_available(&self, email: &str) -> bool {
        self.usage.is_available(email, self.cooldown_min())
    }

    /// 可見帳號（依搜尋 + 篩選）
    fn visible_accounts(&self) -> Vec<&Account> {
        let term = self.search.trim().to_lowercase();
        self.accounts
            .iter()
            .filter(|a| {
                if !term.is_empty() && !a.email.to_lowercase().contains(&term) {
                    return false;
                }
                match self.filter {
                    AccFilter::All => true,
                    AccFilter::Available => self.is_available(&a.email),
                    AccFilter::Cooling => !self.is_available(&a.email),
                }
            })
            .collect()
    }

    /// 啟動批次任務（傳入要跑的帳號）
    fn launch_task(&mut self, accounts: Vec<Account>) {
        if self.running {
            self.status = "⚠️ 已有任務在執行中".into();
            return;
        }
        if accounts.is_empty() {
            self.status = "⚠️ 沒有可執行的帳號（請勾選且需為可用狀態）".into();
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
        self.total_count = accounts.len();
        self.status = format!("開始執行：共 {} 個帳號", self.total_count);

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

        // 確保每個帳號都有固定指紋（一號一指紋）並保存
        let mut fp_store = self.fingerprints.clone();
        for acc in &accounts {
            fp_store.get_or_create(&acc.email);
        }
        let _ = fp_store.save(&fingerprint_path());
        self.log(format!("指紋已就緒：{} 個帳號（時區/語言台灣，其餘固定隨機）", accounts.len()));

        let rt = self.rt.handle().clone();

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
                    let email = acc.email.clone();
                    handles.push(rt_worker.spawn(async move {
                        let _permit = permit;
                        let r = browser::run_account_task(
                            chrome_path, i, acc.email, acc.password, acc.note, task, fp, profile_dir, tx.clone(),
                        )
                        .await;
                        // 成功 → 通知 UI 標記已使用
                        if r.is_ok() {
                            let _ = tx.send(format!("__USED__{}", email));
                        }
                        done.fetch_add(1, Ordering::SeqCst);
                    }));
                }
                for h in handles {
                    let _ = h.await;
                }
                let _ = tx.send(format!("🏁 全部完成（共 {} 個）", done.load(Ordering::SeqCst)));
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
                    let _ = tx.send(format!("__DONE__{}", done.load(Ordering::SeqCst)));
                    if stopping.load(Ordering::SeqCst) {
                        break;
                    }
                }
            });
        }
    }

    /// 開始執行勾選的可用帳號（批量開）
    fn start_selected(&mut self) {
        let to_run: Vec<Account> = self
            .accounts
            .iter()
            .filter(|a| self.selected.contains(&a.email) && self.is_available(&a.email))
            .cloned()
            .collect();
        let skipped = self
            .selected
            .iter()
            .filter(|e| !self.is_available(e))
            .count();
        if skipped > 0 {
            self.log(format!("⚠️ 跳過 {} 個冷卻中/不可用的帳號", skipped));
        }
        self.launch_task(to_run);
    }

    /// 單開一個帳號
    fn run_single(&mut self, acc: &Account) {
        if !self.is_available(&acc.email) {
            self.status = format!("⚠️ {} 冷卻中，不可用", acc.email);
            return;
        }
        self.log(format!("▶ 單開 {}", acc.email));
        self.launch_task(vec![acc.clone()]);
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
                            let _ = tx.send(format!("__DOMAINS__{}", serde_json::to_string(&ds).unwrap_or_default()));
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
                            let _ = tx.send(format!("__ACCOUNTS__{}", serde_json::to_string(&accs).unwrap_or_default()));
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
        // ===== 深色主題（品牌統一） =====
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = colors::PANEL;
        visuals.window_fill = colors::BG;
        visuals.extreme_bg_color = colors::BG;
        visuals.faint_bg_color = colors::PANEL_ALT;
        visuals.override_text_color = Some(colors::TEXT);
        visuals.selection.bg_fill = colors::PRIMARY;
        visuals.widgets.inactive.bg_fill = colors::PANEL_ALT;
        visuals.widgets.inactive.weak_bg_fill = colors::PANEL_ALT;
        visuals.widgets.hovered.bg_fill = colors::PANEL_ALT;
        visuals.widgets.hovered.weak_bg_fill = colors::BORDER;
        visuals.widgets.active.bg_fill = colors::BORDER;
        visuals.widgets.noninteractive.bg_fill = colors::PANEL_ALT;
        visuals.widgets.noninteractive.weak_bg_fill = colors::PANEL;
        visuals.widgets.inactive.fg_stroke.color = colors::TEXT;
        ctx.set_visuals(visuals);

        // ===== 快捷鍵（Nielsen 7：彈性與效率） =====
        let (ctrl_s, ctrl_e, esc) = (
            ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S)),
            ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::E)),
            ctx.input(|i| i.key_pressed(egui::Key::Escape)),
        );
        if ctrl_s {
            self.save_config();
            self.status = "✅ 設定已儲存（Ctrl+S）".into();
        }
        if ctrl_e {
            if self.running {
                self.status = "⚠️ 已有任務執行中（Esc 停止）".into();
            } else {
                self.start_selected();
            }
        }
        if esc && self.running {
            self.stopping.store(true, Ordering::SeqCst);
            self.status = "停止中…（Esc）".into();
        }

        self.drain_logs();

        // ===== 頂部狀態列 =====
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(colors::SPACE);
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("📮 Workspace Mail 自動化").color(colors::TEXT));
                ui.separator();
                // 狀態帶語義色（Nielsen 1：狀態可見）
                let status_color = if self.running {
                    colors::INFO
                } else if self.status.starts_with("✅") || self.status.starts_with("已") {
                    colors::SUCCESS
                } else if self.status.starts_with("⚠️") {
                    colors::WARNING
                } else if self.status.starts_with("❌") || self.status.starts_with("⏹") {
                    colors::ERROR
                } else {
                    colors::TEXT_WEAK
                };
                ui.label(egui::RichText::new(format!("狀態：{}", self.status)).color(status_color));
                ui.separator();
                if self.running {
                    // 執行中進度條（Nielsen 1）
                    let frac = if self.total_count > 0 {
                        self.done_count as f32 / self.total_count as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(200.0)
                            .text(format!("{}/{}", self.done_count, self.total_count))
                            .fill(colors::PRIMARY),
                    );
                } else {
                    ui.label(egui::RichText::new(format!("進度：{}/{}", self.done_count, self.total_count)).weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("Ctrl+S 儲存 · Ctrl+E 執行 · Esc 停止").weak());
                });
            });
            ui.add_space(colors::SPACE);
        });

        // ===== 底部 log =====
        egui::TopBottomPanel::bottom("log")
            .resizable(true)
            .default_height(150.0)
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

        // ===== 左：帳號列表 =====
        egui::SidePanel::left("accounts")
            .resizable(true)
            .default_width(430.0)
            .show(ctx, |ui| {
                ui.add_space(colors::SPACE);
                ui.horizontal(|ui| {
                    ui.strong(format!("帳號列表（{}）", self.accounts.len()));
                    if ui.small_button("全選").on_hover_text("選取全部帳號").clicked() {
                        for a in &self.accounts {
                            self.selected.insert(a.email.clone());
                        }
                    }
                    if ui.small_button("清空選取").on_hover_text("取消所有勾選").clicked() {
                        self.selected.clear();
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(150.0).hint_text("搜尋帳號…"));
                    ui.separator();
                    ui.selectable_value(&mut self.filter, AccFilter::All, "全部");
                    ui.selectable_value(&mut self.filter, AccFilter::Available, "可用");
                    ui.selectable_value(&mut self.filter, AccFilter::Cooling, "冷卻中");
                });
                ui.horizontal(|ui| {
                    ui.label("圖例:");
                    ui.colored_label(colors::SUCCESS, "✅ 可用")
                        .on_hover_text("未使用或已過冷卻時間");
                    ui.colored_label(colors::WARNING, "⏳ 冷卻中")
                        .on_hover_text("使用過，等待冷卻結束才能再用");
                    ui.separator();
                    if ui
                        .small_button("🗑 標記勾選為已使用")
                        .on_hover_text("把勾選的帳號標記為已使用（進入冷卻）")
                        .clicked()
                    {
                        let n = self.selected.len();
                        for e in self.selected.iter() {
                            self.usage.mark_used(e);
                        }
                        let _ = self.usage.save(&usage_path());
                        self.log(format!("手動標記 {} 個帳號為已使用", n));
                    }
                });
                ui.separator();

                let visible_emails: Vec<String> = self
                    .visible_accounts()
                    .into_iter()
                    .map(|a| a.email.clone())
                    .collect();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for email in &visible_emails {
                        let Some(acc) = self.accounts.iter().find(|a| &a.email == email).cloned() else {
                            continue;
                        };
                        let status = self.usage.status(&acc.email, self.cooldown_min());
                        let mut sel = self.selected.contains(&acc.email);
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut sel, "").changed() {
                                if sel {
                                    self.selected.insert(acc.email.clone());
                                } else {
                                    self.selected.remove(&acc.email);
                                }
                            }
                            ui.label(&acc.email);
                            match status {
                                AccountStatus::Available => {
                                    ui.colored_label(colors::SUCCESS, "✅ 可用")
                                        .on_hover_text("可以執行");
                                }
                                AccountStatus::Cooling { remain_min } => {
                                    ui.colored_label(
                                        colors::WARNING,
                                        format!("⏳ 冷卻 {}m", remain_min),
                                    )
                                    .on_hover_text("等待冷卻結束才能再用");
                                }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .small_button("▶ 單開")
                                    .on_hover_text("只執行這一個帳號")
                                    .clicked()
                                {
                                    let acc = acc.clone();
                                    self.run_single(&acc);
                                }
                            });
                        });
                    }
                    if visible_emails.is_empty() {
                        ui.label(egui::RichText::new("（沒有符合條件的帳號，請先載入帳號）").weak());
                    }
                });
            });

        // ===== 右：任務設定 =====
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                ui.heading("🎯 任務設定");

                // Firebase / Chrome
                ui.collapsing("🔑 連線設定", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("API Key");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.firebase.api_key).desired_width(300.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("登入帳號");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.firebase.email).desired_width(300.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("登入密碼");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.firebase.password).password(true).desired_width(300.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Chrome 路徑");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.chrome_path).desired_width(300.0));
                    });
                });

                ui.separator();

                // 帳號來源
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
                });

                // 目標
                ui.horizontal(|ui| {
                    ui.label("目標網址");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.url).desired_width(480.0));
                });

                // 欄位
                ui.add_space(4.0);
                ui.strong("填寫欄位（CSS selector → 內容；支援 {email} {password} {note}）");
                let mut remove_idx: Option<usize> = None;
                for (i, f) in self.cfg.fields.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}:", i + 1));
                        ui.add(egui::TextEdit::singleline(&mut f.selector).desired_width(170.0).hint_text("CSS selector"));
                        ui.add(egui::TextEdit::singleline(&mut f.value).desired_width(220.0).hint_text("內容"));
                        if ui.small_button("🗑").clicked() {
                            remove_idx = Some(i);
                        }
                    });
                }
                if let Some(i) = remove_idx {
                    self.cfg.fields.remove(i);
                }
                if ui.small_button("＋ 新增欄位").clicked() {
                    self.cfg.fields.push(FieldAction { selector: String::new(), value: "{email}".into() });
                }

                ui.horizontal(|ui| {
                    ui.label("送出按鈕 selector");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.submit_selector).desired_width(220.0));
                    ui.label("（留空則不點擊）");
                });

                // 執行參數
                ui.horizontal(|ui| {
                    ui.label("並行數");
                    ui.add(egui::DragValue::new(&mut self.cfg.parallel).clamp_range(1..=100));
                    ui.label("欄位間隔 ms");
                    ui.add(egui::DragValue::new(&mut self.cfg.wait_after_ms).clamp_range(0..=30000));
                    ui.label("送出後等待 ms");
                    ui.add(egui::DragValue::new(&mut self.cfg.step_delay_ms).clamp_range(0..=30000));
                });

                ui.separator();

                // 冷卻設定
                ui.heading("⏳ 冷卻設定");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.cfg.cooldown_enabled, "啟用冷卻（使用過的帳號需等待才能再用）");
                    ui.add_enabled_ui(self.cfg.cooldown_enabled, |ui| {
                        ui.label("冷卻時間");
                        ui.add(egui::DragValue::new(&mut self.cfg.cooldown_minutes).clamp_range(0..=10080).suffix(" 分鐘"));
                    });
                });
                ui.checkbox(&mut self.cfg.auto_mark_used, "執行成功後自動標記為已使用");

                ui.separator();

                // 執行控制（主動作：大而醒目——Fitts）
                ui.add_space(colors::SPACE);
                ui.horizontal(|ui| {
                    let sel_n = self.selected.len();
                    if !self.running {
                        let label = if sel_n > 0 {
                            format!("▶ 執行勾選（{}）", sel_n)
                        } else {
                            "▶ 執行勾選（請先勾選帳號）".into()
                        };
                        let btn = egui::Button::new(
                            egui::RichText::new(label).size(17.0).color(colors::TEXT),
                        )
                        .fill(colors::PRIMARY)
                        .min_size(egui::vec2(220.0, 44.0))
                        .rounding(6.0);
                        if ui
                            .add(btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("執行勾選且可用的帳號（Ctrl+E）")
                            .clicked()
                        {
                            self.start_selected();
                        }
                    } else {
                        let btn = egui::Button::new(
                            egui::RichText::new("⏹ 停止").size(17.0).color(colors::TEXT),
                        )
                        .fill(colors::ERROR)
                        .min_size(egui::vec2(120.0, 44.0))
                        .rounding(6.0);
                        if ui
                            .add(btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("停止啟動新的實例（Esc）")
                            .clicked()
                        {
                            self.stopping.store(true, Ordering::SeqCst);
                            self.status = "停止中…".into();
                        }
                    }
                    ui.add_space(colors::SPACE);
                    if ui
                        .button("💾 儲存設定")
                        .on_hover_text("儲存全部設定（Ctrl+S）")
                        .clicked()
                    {
                        self.save_config();
                        self.status = "✅ 設定已儲存".into();
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("提示：勾選要執行的帳號 → 執行勾選；或點帳號右側「▶ 單開」只跑一個。")
                        .weak(),
                );
                ui.add_space(8.0);
            });
        });

        // 刷新節奏（動效：執行中 250ms 讓進度條流暢；閒置 1s 讓冷卻倒數更新）
        let refresh = if self.running {
            std::time::Duration::from_millis(250)
        } else {
            std::time::Duration::from_millis(1000)
        };
        ctx.request_repaint_after(refresh);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_title("Workspace Mail 自動化"),
        ..Default::default()
    };
    eframe::run_native(
        "workspace-mail-automation",
        options,
        Box::new(|cc| Box::new(WmApp::new(cc))),
    )
}
