# Workspace Mail 📮

Workspace 郵箱帳號管理網頁，以 GitHub Pages 線上開啟，資料存放於 Firebase Firestore。

## 線上網址

**https://fengfeng1021.github.io/workspace-mail/**

## 功能

- **域名分類**：左側按 A-Z 排列的域名列表，可新增/刪除域名
- **帳號記錄**：每個域名底下的信箱帳號 + 密碼，按 A-Z 排序
- **新增 / 編輯 / 刪除**：單筆操作
- **批量新增**：一次貼上多行（每行 `帳號 密碼 備註`）
- **批量刪除**：勾選後一次刪除
- **AI 接口（預留）**：呼叫外部 AI 服務批量生成帳號，接口位置在 `app.js` 的 `AI_ENDPOINT`
- **登入保護**：Firebase Auth 登入後才能讀寫（Firestore 規則限定）
- **密碼顯示切換 / 一鍵複製 / 搜尋過濾**

## 技術架構

- 前端：純 HTML + CSS + JavaScript（Firebase JS SDK v10，ES Module）
- 後端：Firebase（專案 **MailHub** / mailhub-d64e2）
  - Firestore：`domains`（域名）+ `accounts`（帳號）
  - Authentication：Email/密碼登入
  - 安全規則：`allow read, write: if request.auth != null`
- 託管：GitHub Pages（repo: fengfeng1021/workspace-mail）

## Firestore 資料結構

```
domains/{docId}
  name: "example.com"
  createdAt: timestamp

accounts/{docId}
  domain: "example.com"
  account: "a01@example.com"
  password: "..."
  note: "備註（選填）"
  sortKey: "a01@example.com"   // 小寫，用於 A-Z 排序
  createdAt / updatedAt: timestamp
```

## 本地開發

```bash
cd D:\Desktop\App\Workspace-Mail
python -m http.server 8765
# 開啟 http://localhost:8765
```

## AI 接口格式

`POST {AI_ENDPOINT}` body：`{"prompt": "...", "domain": "..."}`
回傳：`[{"email":"...","password":"...","note":"..."}]`

設定位置：`app.js` 頂部的 `AI_ENDPOINT` 與 `AI_API_KEY`。

## 相關專案

- `workspace-mail-automation/`：Rust 寫的批量自動化軟體（同時開啟多個無痕瀏覽器自動填表送出）
