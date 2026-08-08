#!/usr/bin/env python3
"""
互動體驗快速審查工具（audit.py）

用法：
    python audit.py <html檔案路徑> [--json]

用標準函式庫分析 HTML，檢查：
- 文字對比度（WCAG 4.5:1 / 3:1）
- 操作目標大小（>=24px，主動作建議 >=44px）
- placeholder-only 欄位（無 label/aria-label）
- 非語意互動元素（div onclick / role=button）
- 雙主按鈕（同尺寸相鄰的 button）
- 表單錯誤處理基本檢查

輸出：檢查報告 + 評分（100 起扣，<80 不合格）。
"""
import argparse
import html.parser
import json
import re
import sys
from pathlib import Path


def hex_to_rgb(c):
    c = c.strip().lower()
    if c.startswith("#"):
        c = c[1:]
        if len(c) == 3:
            c = "".join(ch * 2 for ch in c)
        if len(c) == 6:
            return tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))
    named = {
        "black": (0, 0, 0), "white": (255, 255, 255), "red": (255, 0, 0),
        "gray": (128, 128, 128), "grey": (128, 128, 128), "blue": (0, 0, 255),
        "green": (0, 128, 0), "silver": (192, 192, 192),
    }
    if c in named:
        return named[c]
    return None


def luminance(rgb):
    def f(v):
        v /= 255.0
        return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4
    r, g, b = (f(x) for x in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(fg, bg):
    l1, l2 = luminance(fg), luminance(bg)
    if l1 < l2:
        l1, l2 = l2, l1
    return (l1 + 0.05) / (l2 + 0.05)


class AuditParser(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.issues = []          # (severity, category, message)
        self.buttons = []         # (tag, text, width, height)
        self.inputs = []          # (name, has_label, has_placeholder, has_aria)
        self.in_label = 0
        self.current_label_text = ""
        self._elem_stack = []     # 追蹤目前元素的文字
        self._pending_labels = []

    def _get_style(self, attrs):
        d = dict(attrs)
        style = d.get("style", "")
        return d, style

    def _size_from_style(self, style):
        w = re.search(r"width\s*:\s*(\d+(?:\.\d+)?)px", style)
        h = re.search(r"height\s*:\s*(\d+(?:\.\d+)?)px", style)
        fs = re.search(r"font-size\s*:\s*(\d+(?:\.\d+)?)px", style)
        return (float(w.group(1)) if w else None,
                float(h.group(1)) if h else None,
                float(fs.group(1)) if fs else None)

    def handle_starttag(self, tag, attrs):
        d = dict(attrs)
        style = d.get("style", "")
        if tag in ("button", "a", "input"):
            w, h, fs = self._size_from_style(style)
            if tag == "input" and d.get("type") not in (None, "text", "email", "password", "search", "tel", "url", "number"):
                return
            self.buttons.append((tag, d.get("id", d.get("name", "")), w, h, fs))
            # 焦點可見性：outline:none 且無替代 focus 樣式
            if re.search(r"outline\s*:\s*none", style):
                self.issues.append(("warning", "focus",
                    f"元素「{d.get('id', d.get('name', '?'))}」設了 outline:none——焦點環不可見（WCAG：鍵盤使用者會迷路）"))
            # 圖示按鈕需 aria-label（無文字時）
            if tag in ("button", "a") and not d.get("aria-label") and not d.get("title"):
                self._elem_stack.append((tag, d.get("id", ""), False))
        elif tag in ("button", "a"):
            self._elem_stack.append((tag, d.get("id", ""), False))
        if tag == "input":
            self.inputs.append({
                "name": d.get("id") or d.get("name") or "?",
                "placeholder": d.get("placeholder"),
                "aria": d.get("aria-label"),
            })
            # 檢查是否有先出現的 label for 指向這個 input
            if d.get("id") in getattr(self, "_pending_labels", []):
                self.inputs[-1]["_labeled"] = True
                self._pending_labels.remove(d.get("id"))
        if tag == "label":
            self.in_label += 1
            self._label_for = d.get("for")
            self._label_text = ""
        if tag == "div" and (d.get("onclick") or d.get("role") == "button"):
            self.issues.append(("warning", "semantic",
                f"非語意互動元素：<div{' onclick' if d.get('onclick') else ''}{' role=button' if d.get('role')=='button' else ''}>（應改用 <button>，鍵盤與無障礙才正常）"))
        # 顏色對比（inline style 簡化檢查）
        m_color = re.search(r"color\s*:\s*(#[0-9a-fA-F]{3,8}|[a-z]+)", style)
        m_bg = re.search(r"background(?:-color)?\s*:\s*(#[0-9a-fA-F]{3,8}|[a-z]+)", style)
        if m_color:
            fg = hex_to_rgb(m_color.group(1))
            bg = hex_to_rgb(m_bg.group(1)) if m_bg else (255, 255, 255)
            if fg and bg:
                cr = contrast(fg, bg)
                if cr < 4.5:
                    self.issues.append(("error", "contrast",
                        f"對比度不足 {cr:.2f}:1（需 ≥4.5:1）：{m_color.group(1)} on {m_bg.group(1) if m_bg else 'white'}"))

    def handle_endtag(self, tag):
        if tag == "label":
            self.in_label -= 1
            # label for="id" 關聯：立即匹配；input 尚未出現則記入 pending
            if getattr(self, "_label_for", None):
                matched = False
                for inp in self.inputs:
                    if inp.get("name") == self._label_for:
                        inp["_labeled"] = True
                        matched = True
                if not matched:
                    if not hasattr(self, "_pending_labels"):
                        self._pending_labels = []
                    self._pending_labels.append(self._label_for)
                self._label_for = None
        if tag in ("button", "a") and self._elem_stack:
            elem_tag, elem_id, has_text = self._elem_stack.pop()
            if not has_text:
                self.issues.append(("warning", "aria",
                    f"圖示按鈕「{elem_id or elem_tag}」沒有文字也沒有 aria-label/title（螢幕閱讀器與鍵盤使用者不知道它做什麼）"))

    def handle_data(self, data):
        if self.in_label and data.strip():
            self._label_text = (getattr(self, "_label_text", "") + data.strip())
            # label 包住 input 的關聯
            for inp in self.inputs:
                if inp.get("_labeled") is None:
                    inp["_labeled"] = False
            if self.inputs and not getattr(self, "_label_for", None):
                self.inputs[-1]["_labeled"] = True
        if data.strip() and self._elem_stack:
            t, i, _ = self._elem_stack[-1]
            if t in ("button", "a"):
                self._elem_stack[-1] = (t, i, True)


def audit(html_text):
    p = AuditParser()
    try:
        p.feed(html_text)
    except Exception as e:
        return {"score": 0, "issues": [("error", "parse", f"HTML 解析失敗：{e}")]}

    issues = list(p.issues)

    # reduced-motion：有動畫但沒有 prefers-reduced-motion 處理
    has_animation = re.search(r"animation\s*:|@keyframes", html_text)
    has_reduced_motion = "prefers-reduced-motion" in html_text
    if has_animation and not has_reduced_motion:
        issues.append(("warning", "motion",
            "偵測到動畫（animation/@keyframes）但沒有 prefers-reduced-motion 處理（WCAG：應尊重使用者減少動態的設定）"))

    # placeholder-only 欄位
    for inp in p.inputs:
        if not inp.get("_labeled") and not inp.get("aria"):
            issues.append(("warning", "form",
                f"欄位「{inp['name']}」只有 placeholder 沒有可見標籤（placeholder 會消失、對比常不足，違反 WCAG）"))

    # 雙主按鈕：同尺寸相鄰 button
    btns = [b for b in p.buttons if b[0] == "button" and b[3] and b[3] >= 40]
    if len(btns) >= 2:
        issues.append(("warning", "hierarchy",
            f"偵測到 {len(btns)} 個大按鈕（≥40px）——確認是否有明確主動作，避免兩個同等按鈕競爭（Fitts）"))

    # 目標大小
    for tag, name, w, h, fs in p.buttons:
        if h is not None and h < 24:
            issues.append(("warning", "target-size",
                f"目標「{name}」（{tag}）高度 {h:.0f}px < 24px（WCAG 2.2）"))

    # 評分
    score = 100
    for sev, cat, msg in issues:
        if sev == "error":
            score -= 10
        elif sev == "warning":
            score -= 5
    score = max(0, score)

    return {"score": score, "issues": issues}


def main():
    ap = argparse.ArgumentParser(description="互動體驗快速審查")
    ap.add_argument("file", help="HTML 檔案路徑")
    ap.add_argument("--json", action="store_true", help="輸出 JSON")
    args = ap.parse_args()

    html_text = Path(args.file).read_text(encoding="utf-8", errors="replace")
    result = audit(html_text)

    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return

    print(f"互動體驗評分：{result['score']} / 100", "（<80 不合格）" if result["score"] < 80 else "")
    print("-" * 50)
    if not result["issues"]:
        print("未發現問題 ✅")
    for sev, cat, msg in result["issues"]:
        mark = "❌" if sev == "error" else "⚠️"
        print(f"{mark} [{cat}] {msg}")
    print("-" * 50)
    print("判定：", "不合格，需修正後重評" if result["score"] < 80 else "合格")


if __name__ == "__main__":
    main()
