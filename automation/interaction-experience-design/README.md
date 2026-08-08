# 互動體驗設計（Interaction Experience Design）

專業級「使用者交互體驗」設計 skill，以學術理論為基礎。讓 AI 設計出的介面「好看之外也好用」：使用者一看就懂怎麼用、不會迷路、知道下一步。

## 這顆 skill 做什麼

- **審查任何介面**：拿 Nielsen 十大啟發式逐條檢查，找出可用性問題
- **規劃導覽與資訊架構**：功能分組、疊頁返回、導覽深度，10 個功能不會做成 10 個分頁
- **設計互動**：主動作層級（登入大按鈕＋忘記密碼小字）、操作回饋、表單流程
- **量化品質**：內建評分閘門（<80 分不准交付）＋ 可執行審查工具（audit.py）

## 安裝

### Claude Code
```bash
# 從 Skill Hub 複製（或直接下載本資料夾到 ~/.claude/skills/）
xcopy /E /I /Y skills\interaction-experience-design %USERPROFILE%\.claude\skills\interaction-experience-design
```

### Codex / Cursor
本 skill 附多平台目錄：`.codex/` 與 `.cursor/` 內含相同 SKILL.md，分別複製到對應的 skills 目錄即可。

## 使用範例

| 你對 AI 說 | AI 會做 |
|---|---|
| 「審查我的登入頁」 | 讀 nielsen-heuristics + anti-patterns → 逐條檢查 → 給評分 |
| 「重新設計多功能 App 的導覽」 | 讀 information-architecture + design-laws + case-studies → 分組、疊頁、返回 |
| 「檢查這個 HTML 有沒有無障礙問題」 | 跑 `python scripts/audit.py 檔案.html` → 輸出評分與問題清單 |
| 「設計完成，可以交付嗎」 | 跑評分閘門：<80 分列出修正清單，修完重評 |

## 目錄結構

```
interaction-experience-design/
├── SKILL.md                      # 入口：決策樹索引、核心速覽、工具說明
├── references/                   # 詳細理論（依任務讀取）
│   ├── nielsen-heuristics.md     #   十大啟發式完整版（每條＋實例＋違反）
│   ├── design-laws.md            #   Fitts/Hick/Miller/Gestalt/Jakob/Norman
│   ├── information-architecture.md  # 組織方案、卡片分類、導覽、疊頁返回
│   ├── cognitive-load.md         #   認知負荷理論
│   ├── accessibility-wcag.md     #   WCAG 2.2 互動要求
│   ├── evaluation-methods.md     #   5 種評估方法操作步驟
│   ├── scoring.md                #   評分閘門（<80 不准交付）
│   ├── case-studies.md           #   真實產品好壞案例（Linear/Slack/Stripe…）
│   ├── desktop-apps.md           #   egui／桌面應用適用指引
│   └── anti-patterns-checklist.md  # 反模式＋自檢清單
├── scripts/
│   ├── audit.py                  # 可執行審查：餵 HTML → 評分＋問題清單
│   └── test_audit.py             # 內建自測（12 項，含 benchmark 回歸）
├── benchmarks/                   # 6 個好壞案例基準（登入/儀表板/設定）
├── .codex/  .cursor/             # 多平台目錄
└── README.md
```

## 自測

```bash
python scripts/test_audit.py   # 12 項測試：audit.py 行為 + benchmark 回歸
```

## 學術基礎

Nielsen 十大啟發式（NN/g 2024）、Shneiderman 八條黃金法則、Norman 設計心理學、Fitts's Law、Hick's Law、Miller 7±2、Gestalt 原則、認知負荷理論（Sweller）、資訊架構（Morville & Rosenfeld）、WCAG 2.2、ISO 9241-11。

## 與其他 skill 搭配

- **前端設計總指揮**：三階段流程（UI → UX → 動效）中本 skill 負責「階段二」
- **Impeccable**：它管視覺品質，本 skill 管互動架構
- **UI UX Pro Max**：它的規則資料庫可查具體數值
- **Taste／Hue**：學風格／品牌系統，與互動架構互補

## FAQ

**Q：audit.py 能檢查我自己的網頁嗎？** 可以——餵 HTML 檔案路徑即可；僅限靜態 HTML 與 inline style 的基本機械檢查，設計問題仍要靠人工評分（scoring.md）。

**Q：我是做桌面軟體（egui）的，這有用嗎？** 有用——學理全適用，參考 `references/desktop-apps.md` 有桌面應用的調整與評分方式。

**Q：評分 <80 一定不能交付嗎？** 這是設計品質閘門——不合格代表有明確可修的問題（對比、層級、回饋、架構），修完重評即可，通常一次就能過。
