---
name: interaction-experience-design
description: 專業級「使用者交互體驗」設計 skill。以學術理論為基礎（Nielsen 啟發式、Shneiderman 黃金法則、Norman 設計心理學、Fitts/Hick 定律、Miller 7±2、Gestalt、認知負荷理論、資訊架構學、WCAG、ISO 9241）。當任務涉及登入流程、多功能 App 組織、導覽與資訊架構、按鈕層級、疊頁與返回、可學習性、操作回饋、可用性審查、無障礙時使用。詳細理論在 references/ 各檔，依需要讀取。
version: 3.0.0
---

# 互動體驗設計（Interaction Experience Design）

專業版 v3：多檔結構。本檔是入口——含核心原則速覽與使用流程；**詳細理論與檢查表在 references/ 各檔**，依任務需要讀取（不要全部載入）。

## 何時使用

登入／註冊流程、多功能 App 的導覽與組織、分頁與側邊欄規劃、按鈕層級、返回與頁面切換、表單流程、onboarding、可用性審查、無障礙檢查。純視覺美化不在此範圍。

## 核心原則速覽（詳細版見 references）

1. **一頁一個主動作**：主動作大按鈕、次要動作降級（Fitts's Law）
2. **功能先分組再導覽**：3-5 組、深度 ≤3 層（Hick's Law、IA）
3. **疊頁與返回**：下鑽蓋舊頁、返回固定、前景後景分明（IA 導覽）
4. **用慣例**：別發明新位置（Jakob's Law）
5. **狀態可見**：我在哪、載入中、下一步（Nielsen 1）
6. **辨識勝於回憶**：別讓使用者記（Nielsen 6）
7. **錯誤可恢復**：錯誤預防＋白話診斷（Nielsen 5、9）
8. **操作有回饋**：按了要有反應（Shneiderman 3）
9. **降低認知負荷**：一次一件事（Sweller）
10. **無障礙**：對比、鍵盤、焦點（WCAG）

## 使用流程

1. **分析任務**：這是什麼流程？使用者要完成什麼？目標是誰？
2. **資訊架構**：功能怎麼分組？導覽幾層？（讀 references/information-architecture.md）
3. **互動模式**：疊頁？Tab？步驟條？返回怎麼做？
4. **逐條審查**：拿 Nielsen 10 啟發式檢查（references/nielsen-heuristics.md）
5. **驗證**：任務分析走一遍；有條件做可用性測試（references/evaluation-methods.md）

## 依任務讀取（決策樹——照著走，不要憑感覺）

**接到任務後，先判斷任務類型，讀對應的 reference 再動手：**

| 任務類型 | 必讀 | 用途 |
|---|---|---|
| 審查／找出介面問題 | `references/nielsen-heuristics.md` ＋ `references/anti-patterns-checklist.md` | 逐條檢查＋對照反模式 |
| 規劃導覽／頁面組織／分組 | `references/information-architecture.md` | 組織方案、導覽系統、疊頁返回 |
| 決定按鈕大小／選項數／視覺分組 | `references/design-laws.md` | Fitts、Hick、Miller、Gestalt |
| 流程複雜／資訊量大／長表單 | `references/cognitive-load.md` | 三類負荷與拆步策略 |
| 檢查對比／鍵盤／目標大小 | `references/accessibility-wcag.md` | WCAG 2.2 互動要求 |
| 設計完成要驗證 | `references/evaluation-methods.md` ＋ `references/scoring.md` | 評估方法＋評分閘門（<80 不准交付） |
| 交付前最終檢查 | `references/anti-patterns-checklist.md` ＋ `references/scoring.md` | 自檢打勾＋計分判定（強制） |
| 需要具體設計參考 | `references/case-studies.md` | 真實產品好/壞案例對照 |

**規則**：
- 任務橫跨多類 → 讀對應的多份（例如「登入流程重設計」＝ design-laws ＋ information-architecture ＋ nielsen-heuristics ＋ case-studies）
- 不確定讀哪份 → 讀 `references/nielsen-heuristics.md`（最通用的審查基準）
- 完成設計後**一定**用 `references/anti-patterns-checklist.md` 自檢，再用 `references/scoring.md` 計分——**<80 分不准交付**（強制步驟，不是選配）

## 檔案導覽（references/ 完整索引）

| 檔案 | 內容 | 何時讀 |
|---|---|---|
| `nielsen-heuristics.md` | 十大啟發式完整版：每條＋實例＋常見違反 | 審查任何介面時 |
| `design-laws.md` | Fitts／Hick／Miller／Gestalt／Jakob／Norman 詳解 | 決定按鈕大小、選項數、分組時 |
| `information-architecture.md` | 組織方案、心智模型、Card Sorting、深度廣度、導覽系統、疊頁返回 | 規劃導覽與頁面組織時 |
| `cognitive-load.md` | 認知負荷理論：三類負荷與減載策略 | 流程複雜、資訊量大時 |
| `accessibility-wcag.md` | WCAG 2.2 互動相關要求與對應實作 | 檢查無障礙時 |
| `evaluation-methods.md` | 啟發式評估／任務分析／可用性測試／A-B／心智模型測試 操作步驟 | 設計完成要驗證時 |
| `scoring.md` | 評分閘門：100 分制扣分表，<80 不准交付 | 每次設計完成強制評分 |
| `case-studies.md` | 真實產品互動案例（Linear／Slack／Stripe／Notion／Airbnb／Gmail 好壞對照） | 需要具體設計參考時 |
| `anti-patterns-checklist.md` | 反模式清單＋自檢清單（含學理對應） | 設計中與交付前 |

## 可執行工具（scripts/）

- `scripts/audit.py`：**互動體驗快速審查**。餵 HTML 檔案自動檢查（對比度／目標大小／placeholder-only／非語意元素／雙主按鈕）並輸出評分。
  用法：`python scripts/audit.py <html檔案> [--json]`；<80 分不合格需修正重評。
  有 HTML 成品時一律執行，與 `references/scoring.md` 互補（工具抓機械問題，人工評分抓設計問題）。

## 反模式速覽（詳細版見 references/anti-patterns-checklist.md）

❌ 10 功能 10 分頁 ❌ 兩個大按鈕競爭 ❌ 無返回／位置亂跳 ❌ 深層迷路 ❌ 破壞慣例 ❌ 操作無回饋 ❌ 只靠 hover ❌ 讓使用者記很多 ❌ 錯誤訊息講術語

## 協同使用

- 與**前端設計總指揮**搭配：總指揮三階段流程中，本 skill 負責「階段二 UX 互動」
- 與 **Impeccable** 搭配：它管視覺品質，本 skill 管互動架構
- 與 **UI UX Pro Max** 搭配：它的 Navigation Patterns 可查具體數值
