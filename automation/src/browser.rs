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

    // 關閉瀏覽器（無論成敗）
    let _ = browser.close().await;

    match &result {
        Ok(()) => {
            let _ = log.send(format!("{} ✅ 完成", tag));
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
