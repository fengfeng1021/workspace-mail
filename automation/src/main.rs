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
const LOGINS_FILE: &str = "wm-logins.json";
/// 持久 Chrome profile 儲存位置：Workspace-Mail 專案目錄下（隨專案走，統一管理）
const PROFILE_ROOT: &str = r"D:\Desktop\App\Workspace-Mail\wm-profiles";

/// 色彩系統（品牌統一：一處定義，全 UI 使用）
mod colors {
    use egui::Color32;
    // 品牌主色（深藍：白字對比 ≥5:1，WCAG AA 保險餘量）
    pub const PRIMARY: Color32 = Color32::from_rgb(38, 82, 200);
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
    /// 冷卻天數（自訂）
    cooldown_days: u64,
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
            url: "https://www.google.com/maps".into(),
            // 預設純停留模式：不填表不點擊（需要時在 UI 新增欄位）
            fields: vec![],
            submit_selector: "".into(),
            wait_after_ms: 3000,
            step_delay_ms: 500,
            parallel: 5,
            cooldown_enabled: true,
            cooldown_days: 1,
            auto_mark_used: true,
        }
    }
}

/// 剩餘冷卻時間格式化（分鐘 → 天／小時）
fn format_remain(remain_min: u64) -> String {
    if remain_min >= 1440 {
        let days = remain_min / 1440;
        let hours = (remain_min % 1440) / 60;
        if hours > 0 {
            format!("⏳ 冷卻 {}天{}小時", days, hours)
        } else {
            format!("⏳ 冷卻 {}天", days)
        }
    } else if remain_min >= 60 {
        format!("⏳ 冷卻 {}小時", remain_min / 60)
    } else {
        format!("⏳ 冷卻 {}分", remain_min)
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
fn logins_path() -> PathBuf {
    exe_dir().join(LOGINS_FILE)
}

/// 登入狀態標記（email → 登入成功時間）
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct LoginStore {
    map: std::collections::HashMap<String, u64>,
}

impl LoginStore {
    fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    fn save(&self, path: &std::path::Path) -> Result<(), anyhow::Error> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
    fn mark(&mut self, email: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.map.insert(email.to_string(), now);
    }
    fn is_logged_in(&self, email: &str) -> bool {
        self.map.contains_key(email)
    }
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
    logins: LoginStore,
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
            logins: LoginStore::load(&logins_path()),
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
            if let Some(email) = msg.strip_prefix("__LOGINED__") {
                self.logins.mark(email);
                let _ = self.logins.save(&logins_path());
                self.log(format!("🔑 {} 登入成功（已儲存，之後開瀏覽器自動帶入）", email));
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

    fn cooldown_days(&self) -> u64 {
        if self.cfg.cooldown_enabled { self.cfg.cooldown_days } else { 0 }
    }

    fn is_available(&self, email: &str) -> bool {
        self.usage.is_available(email, self.cooldown_days())
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
        // URL 未填 → 開 about:blank（瀏覽器仍會開啟，方便先登入 Google）
        let url = if self.cfg.url.trim().is_empty() {
            self.log("ℹ️ 目標網址未填寫，瀏覽器將開啟空白頁（可在目標網址填 accounts.google.com 做登入）");
            "about:blank".to_string()
        } else {
            self.cfg.url.trim().to_string()
        };
        self.save_config();

        self.running = true;
        self.stopping.store(false, Ordering::SeqCst);
        self.done_count = 0;
        self.total_count = accounts.len();
        self.status = format!("開始執行：共 {} 個帳號", self.total_count);

        let task = TaskConfig {
            url: url.clone(),
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

    /// 批次登入 Google（對勾選帳號執行一次登入，cookie 存入持久 profile）
    fn launch_login(&mut self, accounts: Vec<Account>) {
        if accounts.is_empty() {
            self.status = "⚠️ 沒有勾選的帳號".into();
            return;
        }
        let chrome_path = self.cfg.chrome_path.clone();
        let tx = self.log_tx.clone();
        let rt = self.rt.handle().clone();
        self.status = format!("🔑 開始登入：共 {} 個帳號", accounts.len());
        self.log(format!("🔑 開始登入 {} 個帳號（登入一次，之後開瀏覽器自動帶入）", accounts.len()));

        // 確保指紋存在
        let mut fp_store = self.fingerprints.clone();
        for acc in &accounts {
            fp_store.get_or_create(&acc.email);
        }
        let _ = fp_store.save(&fingerprint_path());

        rt.spawn(async move {
            for (i, acc) in accounts.into_iter().enumerate() {
                let profile_dir = profile_dir_for(&acc.email);
                let tx = tx.clone();
                let chrome_path = chrome_path.clone();
                let r = browser::login_google_task(
                    chrome_path, i, acc.email.clone(), acc.password, profile_dir, tx.clone(),
                )
                .await;
                match r {
                    Ok(browser::LoginResult::Success) => {
                        let _ = tx.send(format!("__LOGINED__{}", acc.email));
                    }
                    Ok(browser::LoginResult::NeedManual(msg)) => {
                        let _ = tx.send(format!("⚠️ {} 需人工處理：{}", acc.email, msg));
                    }
                    Err(e) => {
                        let _ = tx.send(format!("❌ {} 登入失敗：{}", acc.email, e));
                    }
                }
            }
            let _ = tx.send("🏁 登入批次結束".to_string());
        });
    }

    fn spawn_firestore_job(&mut self) {
        let mut client = FirestoreClient::new(self.cfg.firebase.clone());
        let tx = self.log_tx.clone();
        self.rt.spawn(async move {
            let login = client.login().await;
            let result = match login {
                Ok(_) => client.fetch_all_accounts().await,
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
        });
    }
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
                    ui.strong(format!("📁 帳號資料夾（{}）", self.accounts.len()));
                    ui.separator();
                    // 一鍵選取所有可用帳號（解決逐個勾選麻煩）
                    if ui
                        .small_button("☑ 全選可用")
                        .on_hover_text("一鍵勾選所有「可用」狀態的帳號")
                        .clicked()
                    {
                        for acc in &self.accounts {
                            if self.is_available(&acc.email) {
                                self.selected.insert(acc.email.clone());
                            }
                        }
                    }
                    if ui.small_button("✖ 清除").on_hover_text("取消所有勾選").clicked() {
                        self.selected.clear();
                    }
                    ui.separator();
                    if ui
                        .small_button("🔑 登入勾選")
                        .on_hover_text("對勾選的帳號執行 Google 登入（一次即可，之後開瀏覽器自動帶入登入狀態）")
                        .clicked()
                    {
                        let to_login: Vec<Account> = self
                            .accounts
                            .iter()
                            .filter(|a| self.selected.contains(&a.email))
                            .cloned()
                            .collect();
                        self.launch_login(to_login);
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
                        .small_button("🗑 標記勾選已使用")
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
                    if ui
                        .small_button("🔓 解除勾選冷卻")
                        .on_hover_text("把勾選中「冷卻中」的帳號立即恢復為可用")
                        .clicked()
                    {
                        let mut n = 0;
                        for e in self.selected.iter() {
                            if !self.is_available(e) {
                                self.usage.clear_used(e);
                                n += 1;
                            }
                        }
                        let _ = self.usage.save(&usage_path());
                        self.log(format!("手動解除 {} 個帳號的冷卻", n));
                    }
                });
                ui.separator();

                // ===== 資料夾樹：域名 = 資料夾，帳號在內 =====
                let term = self.search.trim().to_lowercase();
                let mut by_domain: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
                for acc in &self.accounts {
                    // 搜尋 + 篩選
                    if !term.is_empty() && !acc.email.to_lowercase().contains(&term) {
                        continue;
                    }
                    let ok = match self.filter {
                        AccFilter::All => true,
                        AccFilter::Available => self.is_available(&acc.email),
                        AccFilter::Cooling => !self.is_available(&acc.email),
                    };
                    if !ok {
                        continue;
                    }
                    let key = if acc.domain.is_empty() { "（未分類）".to_string() } else { acc.domain.clone() };
                    by_domain.entry(key).or_default().push(acc.email.clone());
                }

                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (domain, emails) in &by_domain {
                        let avail = emails
                            .iter()
                            .filter(|e| self.is_available(e))
                            .count();
                        let all_sel = emails.iter().all(|e| self.selected.contains(e));
                        let header = format!(
                            "📁 {}（{} 個{}{}）",
                            domain,
                            emails.len(),
                            if avail > 0 { format!("\u{ff0c}可用 {}", avail) } else { String::new() },
                            if all_sel { "\u{ff0c}已全選" } else { "" },
                        );
                        ui.horizontal(|ui| {
                            // 資料夾級勾選（一次選取整個域名）
                            let mut sel = all_sel;
                            if ui.checkbox(&mut sel, "").on_hover_text("選取／取消整個資料夾").changed() {
                                let to_toggle: Vec<String> = emails.clone();
                                for e in to_toggle {
                                    if sel {
                                        self.selected.insert(e);
                                    } else {
                                        self.selected.remove(&e);
                                    }
                                }
                            }
                            egui::CollapsingHeader::new(header)
                                .default_open(emails.len() <= 10)
                                .show(ui, |ui| {
                                    for email in emails {
                                        let Some(acc) = self.accounts.iter().find(|a| &a.email == email).cloned() else {
                                            continue;
                                        };
                                        let status = self.usage.status(&acc.email, self.cooldown_days());
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
                                            if self.logins.is_logged_in(&acc.email) {
                                                ui.colored_label(colors::INFO, "🔑 已登入")
                                                    .on_hover_text("已完成 Google 登入，開瀏覽器自動帶入登入狀態");
                                            }
                                            match status {
                                                AccountStatus::Available => {
                                                    ui.colored_label(colors::SUCCESS, "✅ 可用")
                                                        .on_hover_text("可以執行");
                                                }
                                                AccountStatus::Cooling { remain_min } => {
                                                    ui.colored_label(
                                                        colors::WARNING,
                                                        format!("⏳ {}", format_remain(remain_min)),
                                                    )
                                                    .on_hover_text("等待冷卻結束才能再用，或手動解除");
                                                }
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .small_button("▶ 單開")
                                                        .on_hover_text("只執行這一個帳號")
                                                        .clicked()
                                                    {
                                                        self.run_single(&acc);
                                                    }
                                                },
                                            );
                                        });
                                    }
                                });
                        });
                    }
                    if by_domain.is_empty() {
                        ui.label(
                            egui::RichText::new("（沒有帳號——點右側「📥 載入帳號」取得帳號清單）").weak(),
                        );
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

                // 帳號來源（一次載入全部，左側自動分資料夾）
                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new("📥 載入帳號").size(15.0))
                        .on_hover_text("從 Firebase 一次讀取全部域名的帳號")
                        .clicked()
                    {
                        self.status = "讀取帳號中…".into();
                        self.spawn_firestore_job();
                    }
                    if self.accounts.is_empty() {
                        ui.label(egui::RichText::new("尚未載入帳號").weak());
                    } else {
                        ui.label(
                            egui::RichText::new(format!("已載入 {} 個帳號", self.accounts.len())).weak(),
                        );
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
                        ui.add(
                            egui::DragValue::new(&mut self.cfg.cooldown_days)
                                .clamp_range(0..=365)
                                .suffix(" 天"),
                        );
                        ui.label("（0 = 停用）");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_stay_mode() {
        // 停留模式預設：目標 Google Maps、不填表、不點擊
        let cfg = AppConfig::default();
        assert_eq!(cfg.url, "https://www.google.com/maps");
        assert!(cfg.fields.is_empty(), "停留模式預設不填表");
        assert!(cfg.submit_selector.is_empty(), "停留模式預設不點擊");
    }

    #[test]
    fn format_remain_readable() {
        assert!(format_remain(30).contains("分"));
        assert!(format_remain(90).contains("小時"));
        assert!(format_remain(1500).contains("天"));
        assert!(format_remain(1500).contains("小時"));
    }
}
