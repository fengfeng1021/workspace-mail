// 瀏覽器控制：每帳號一個持久 Chrome profile（預先登入後免重登）＋ 固定指紋
use crate::fingerprint::{fingerprint_js, Fingerprint};
use anyhow::{anyhow, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::{
    SetLocaleOverrideParams, SetTimezoneOverrideParams,
};
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::Page;
use futures::StreamExt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FieldAction {
    /// CSS selector，例如 "#email" / "input[name=user]" / "textarea"
    pub selector: String,
    /// 要輸入的文字；支援 {email} {password} {note} 變數
    pub value: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TaskConfig {
    /// 目標網址
    pub url: String,
    /// 依序填寫的欄位
    pub fields: Vec<FieldAction>,
    /// 最後要點擊的按鈕/元素（CSS selector），可留空
    pub submit_selector: String,
    /// 點擊送出後等待毫秒
    pub wait_after_ms: u64,
    /// 元素操作間隔毫秒
    pub step_delay_ms: u64,
}

/// 替換文字中的變數
fn apply_vars(template: &str, email: &str, password: &str, note: &str) -> String {
    template
        .replace("{email}", email)
        .replace("{password}", password)
        .replace("{note}", note)
}

/// 建立瀏覽器設定（持久 profile + 固定指紋 + 輕量參數）
fn build_browser_config(chrome_path: &str, profile_dir: &PathBuf, fp: &Fingerprint) -> Result<BrowserConfig> {
    BrowserConfig::builder()
        .with_head()
        .window_size(fp.width, fp.height)
        .viewport(Viewport {
            width: fp.width,
            height: fp.height,
            ..Default::default()
        })
        .chrome_executable(chrome_path)
        .user_data_dir(profile_dir)
        // 輕量參數：省記憶體
        .arg("--disable-gpu")
        .arg("--disable-extensions")
        .arg("--disable-background-networking")
        .arg("--disable-component-update")
        .arg("--disable-sync")
        .arg("--mute-audio")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // 語言：影響 Accept-Language header 與 Intl 預設
        .arg("--lang=zh-TW")
        .arg("--accept-lang=zh-TW,zh;q=0.9,en-US;q=0.8")
        .build()
        .map_err(|e| anyhow!("瀏覽器設定失敗: {}", e))
}

/// 套用固定指紋到頁面（UA/時區/語言/視窗 + JS 雜訊）
async fn apply_fingerprint(page: &Page, fp: &Fingerprint) -> Result<()> {
    page.set_user_agent(&fp.user_agent)
        .await
        .map_err(|e| anyhow!("設定 UA 失敗: {}", e))?;
    page.execute(SetTimezoneOverrideParams::new(&fp.timezone))
        .await
        .map_err(|e| anyhow!("設定時區失敗: {}", e))?;
    page.execute(SetLocaleOverrideParams {
        locale: Some(fp.locale.clone()),
    })
    .await
    .map_err(|e| anyhow!("設定語言失敗: {}", e))?;
    // 對之後所有新文檔注入指紋雜訊 JS
    let _ = page.evaluate_on_new_document(fingerprint_js(fp)).await;
    Ok(())
}

/// 用一個帳號跑完整流程（持久 profile + 固定指紋）
pub async fn run_account_task(
    chrome_path: String,
    index: usize,
    email: String,
    password: String,
    note: String,
    task: TaskConfig,
    fp: Fingerprint,
    profile_dir: PathBuf,
    log: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<()> {
    let tag = format!("[{} {}]", index, email);
    let _ = log.send(format!("{} 啟動瀏覽器（profile: {}）…", tag, profile_dir.display()));

    let config = build_browser_config(&chrome_path, &profile_dir, &fp)?;
    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| anyhow!("Chrome 啟動失敗: {}", e))?;

    // handler 必須在背景輪詢
    tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    let result = run_task_in_browser(&mut browser, &tag, &email, &password, &note, &task, &fp, &log).await;

    // 停留模式：不關閉瀏覽器——leak Browser（避免 drop 時 kill_on_drop 殺掉 Chrome），
    // 使用者手動關掉視窗即結束
    std::mem::forget(browser);

    match &result {
        Ok(()) => {
            if task.fields.is_empty() && task.submit_selector.is_empty() {
                let _ = log.send(format!("{} ✅ 已開啟並停留在目標頁（手動關閉視窗即結束）", tag));
            } else {
                let _ = log.send(format!("{} ✅ 完成，瀏覽器保持開啟（手動關閉視窗即結束）", tag));
            }
        }
        Err(e) => {
            let _ = log.send(format!("{} ❌ {}", tag, e));
        }
    }
    result
}

async fn run_task_in_browser(
    browser: &mut Browser,
    tag: &str,
    email: &str,
    password: &str,
    note: &str,
    task: &TaskConfig,
    fp: &Fingerprint,
    log: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<()> {
    let _ = log.send(format!("{} 開新分頁並套用指紋…", tag));
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| anyhow!("開啟頁面失敗: {}", e))?;
    apply_fingerprint(&page, fp).await?;

    let _ = log.send(format!("{} 導航 {}…", tag, task.url));
    page.goto(&task.url)
        .await
        .map_err(|e| anyhow!("導航失敗: {}", e))?;
    let _ = page.wait_for_navigation().await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // 依序填寫欄位
    for f in &task.fields {
        let value = apply_vars(&f.value, email, password, note);
        let _ = log.send(format!("{} 填寫 {} ← {}", tag, f.selector, value));
        let el = page
            .find_element(&f.selector)
            .await
            .map_err(|e| anyhow!("找不到元素 {}: {}", f.selector, e))?;
        el.click()
            .await
            .map_err(|e| anyhow!("點擊 {} 失敗: {}", f.selector, e))?;
        type_text(&page, &f.selector, &value).await?;
        tokio::time::sleep(Duration::from_millis(task.step_delay_ms)).await;
    }

    // 點擊送出
    if !task.submit_selector.is_empty() {
        let _ = log.send(format!("{} 點擊送出 {}", tag, task.submit_selector));
        let btn = page
            .find_element(&task.submit_selector)
            .await
            .map_err(|e| anyhow!("找不到送出按鈕 {}: {}", task.submit_selector, e))?;
        btn.click()
            .await
            .map_err(|e| anyhow!("點擊送出失敗: {}", e))?;
        tokio::time::sleep(Duration::from_millis(task.wait_after_ms)).await;
    }

    Ok(())
}

/// 用 JS 設定 input/textarea 值並觸發事件（支援中文與 React/Vue 表單）
async fn type_text(
    page: &chromiumoxide::page::Page,
    selector: &str,
    value: &str,
) -> Result<()> {
    let js = format!(
        "(() => {{ const el = document.querySelector({sel:?}); if (!el) return false; \
         const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; \
         const setter = Object.getOwnPropertyDescriptor(proto, 'value').set; \
         setter.call(el, {val:?}); \
         el.dispatchEvent(new Event('input', {{bubbles:true}})); \
         el.dispatchEvent(new Event('change', {{bubbles:true}})); \
         return true; }})()",
        sel = selector,
        val = value
    );
    let result = page
        .evaluate_expression(&js)
        .await
        .map_err(|e| anyhow!("輸入到 {} 失敗: {}", selector, e))?;
    if result.value() != Some(&serde_json::Value::Bool(true)) {
        return Err(anyhow!("找不到元素 {}（JS 注入失敗）", selector));
    }
    Ok(())
}

/// 依文字點擊按鈕（Google 登入頁的下一步/登入按鈕無穩定 id，用文字匹配）
async fn click_button_by_text(page: &Page, text: &str) -> Result<()> {
    let js = format!(
        "(() => {{ const btn = [...document.querySelectorAll('button')].find(b => (b.innerText||'').trim().includes({txt:?})); if (!btn) return false; btn.click(); return true; }})()",
        txt = text
    );
    let result = page
        .evaluate_expression(&js)
        .await
        .map_err(|e| anyhow!("點擊「{}」失敗: {}", text, e))?;
    if result.value() != Some(&serde_json::Value::Bool(true)) {
        return Err(anyhow!("找不到「{}」按鈕", text));
    }
    Ok(())
}

/// 輪詢等待 selector 出現（最多 timeout 秒）
async fn wait_for_selector(page: &Page, selector: &str, timeout_secs: u64) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if page.find_element(selector).await.is_ok() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(anyhow!("等待元素 {} 超時（{} 秒）", selector, timeout_secs));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// 登入 Google 結果
#[derive(Debug, Clone, PartialEq)]
pub enum LoginResult {
    /// 登入成功（cookie 已存入持久 profile）
    Success,
    /// 需要人工處理（驗證碼／裝置驗證等）
    NeedManual(String),
}

/// 登入 Google 帳號（一次即可，cookie 存入持久 profile，之後開瀏覽器自動帶入登入狀態）
pub async fn login_google_task(
    chrome_path: String,
    index: usize,
    email: String,
    password: String,
    profile_dir: PathBuf,
    log: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<LoginResult> {
    let tag = format!("[{} {}]", index, email);
    let _ = log.send(format!("{} 🔑 開始登入 Google…", tag));

    let mut fp_store = crate::fingerprint::FingerprintStore::default();
    let fp = fp_store.get_or_create(&email);

    let config = build_browser_config(&chrome_path, &profile_dir, &fp)?;
    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| anyhow!("Chrome 啟動失敗: {}", e))?;
    tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    let result = async {
        let page = browser
            .new_page("https://accounts.google.com")
            .await
            .map_err(|e| anyhow!("開啟登入頁失敗: {}", e))?;
        apply_fingerprint(&page, &fp).await?;

        // 1. 等 email 欄位
        wait_for_selector(&page, "#identifierId", 20).await?;
        tokio::time::sleep(Duration::from_millis(800)).await;
        type_text(&page, "#identifierId", &email).await?;
        let _ = log.send(format!("{} 已填入帳號 {}", tag, email));
        tokio::time::sleep(Duration::from_millis(400)).await;
        click_button_by_text(&page, "下一步").await?;

        // 2. 等密碼欄位
        wait_for_selector(&page, "input[name=Passwd]", 15).await?;
        tokio::time::sleep(Duration::from_millis(800)).await;
        type_text(&page, "input[name=Passwd]", &password).await?;
        let _ = log.send(format!("{} 已填入密碼", tag));
        tokio::time::sleep(Duration::from_millis(400)).await;
        click_button_by_text(&page, "下一步").await?;

        // 3. 等待登入結果
        tokio::time::sleep(Duration::from_secs(6)).await;
        let url = page
            .url()
            .await
            .map_err(|e| anyhow!("讀取頁面狀態失敗: {}", e))?
            .unwrap_or_default();

        // 錯誤訊息檢查——只認「真正的錯誤」，忽略提示性訊息（如「密碼已在 X 小時前變更」是資訊提示，不是失敗）
        let err_js = r#"(() => { const el = document.querySelector('[role=alert], .error, [jsname="B34EJ"]'); return el ? el.innerText.slice(0, 120) : ''; })()"#;
        if let Ok(res) = page.evaluate_expression(err_js).await {
            if let Some(v) = res.value() {
                if let Some(msg) = v.as_str() {
                    let m = msg.to_lowercase();
                    let is_real_error = ["無法登入", "不正確", "密碼錯誤", "找不到您的", "未註冊", "無法識別",
                        "couldn't find", "can't sign", "incorrect", "invalid", "isn't a google", "not registered"]
                        .iter().any(|k| m.contains(k));
                    if is_real_error && url.contains("accounts.google.com") {
                        return Err(anyhow!("登入被拒：{}", msg));
                    }
                    // 「密碼已在 X 小時前變更」＝資訊提示：密碼已填妥，繼續點下一步
                    if (msg.contains("變更") || m.contains("changed")) && url.contains("accounts.google.com") {
                        let _ = log.send(format!("{} ℹ️ 偵測到「密碼已變更」提示（資訊非錯誤），繼續登入…", tag));
                        let _ = click_button_by_text(&page, "下一步").await;
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        let url_after = page.url().await.ok().flatten().unwrap_or_default();
                        if !url_after.contains("accounts.google.com") {
                            let _ = log.send(format!("{} ✅ 登入成功（cookie 已存入 profile）", tag));
                            return Ok(LoginResult::Success);
                        }
                    }
                }
            }
        }

        // 驗證碼／裝置驗證偵測（仍在 accounts.google.com 且沒有跳到正常頁）
        if url.contains("accounts.google.com") {
            let captcha_js = r#"(() => {
                const captcha = !!document.querySelector('iframe[src*="recaptcha"], #captcha, [jsname*="captcha"]');
                const challenge = (document.body.innerText||'').includes('这是你吗') || (document.body.innerText||'').includes('這是你嗎');
                const verify = (document.body.innerText||'').includes('验证码') || (document.body.innerText||'').includes('驗證碼');
                return captcha || challenge || verify;
            })()"#;
            if let Ok(res) = page.evaluate_expression(captcha_js).await {
                if res.value().and_then(|v| v.as_bool()).unwrap_or(false) {
                    return Ok(LoginResult::NeedManual(
                        "偵測到驗證碼／裝置驗證，請在開啟的瀏覽器完成後再關閉".to_string(),
                    ));
                }
            }
            return Ok(LoginResult::NeedManual(
                "登入頁未完成跳轉，請檢查開啟的瀏覽器並手動完成".to_string(),
            ));
        }

        let _ = log.send(format!("{} ✅ 登入成功（cookie 已存入 profile）", tag));
        Ok(LoginResult::Success)
    }
    .await;

    match &result {
        Ok(LoginResult::Success) => {
            // 登入成功：cookie 已寫入持久 profile，關閉瀏覽器（下次開自動帶入）
            let _ = browser.close().await;
        }
        Ok(LoginResult::NeedManual(msg)) => {
            // 需人工：保持瀏覽器開啟，使用者處理後手動關閉
            let _ = log.send(format!("{} ⚠️ {}（處理完手動關閉視窗）", tag, msg));
            std::mem::forget(browser);
        }
        Err(e) => {
            let _ = log.send(format!("{} ❌ {}", tag, e));
            let _ = browser.close().await;
        }
    }
    result
}
