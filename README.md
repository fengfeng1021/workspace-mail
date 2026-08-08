# Workspace Mail 📮

Workspace 郵箱帳號管理網頁，以 GitHub Pages 線上開啟，資料存放於 Firebase Firestore。

## 功能

- **域名分類**：左側按 A-Z 排列的域名列表，可新增/刪除域名
- **帳號記錄**：每個域名底下的信箱帳號 + 密碼，按 A-Z 排序
- **新增 / 編輯 / 刪除**：單筆操作
- **批量新增**：一次貼上多行（每行 `帳號 密碼 備註`）
- **AI 接口（預留）**：呼叫外部 AI 服務批量生成帳號，接口位置在 `app.js` 的 `AI_ENDPOINT`
- **登入保護**：Firebase Auth 登入後才能讀寫（Firestore 規則限定）

## 部署步驟

1. 在 Firebase 主控台建立專案 → 開啟 **Firestore Database**
2. 專案設定 → 新增 Web App → 複製 firebaseConfig
3. 填入 `app.js` 頂部的 `FIREBASE_CONFIG`
4. 啟用 **Authentication** → 登入方式 → Email/密碼
5. Firestore 規則（只允許登入者讀寫）：

```
rules_version = '2';
service cloud.firestore {
  match /databases/{database}/documents {
    match /{document=**} {
      allow read, write: if request.auth != null;
    }
  }
}
```

6. 推上 GitHub 並開啟 Pages

## AI 接口格式

`POST {AI_ENDPOINT}` body：`{"prompt": "...", "domain": "..."}`
回傳：`[{"email":"...","password":"...","note":"..."}]`
