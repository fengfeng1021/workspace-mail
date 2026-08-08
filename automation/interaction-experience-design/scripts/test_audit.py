#!/usr/bin/env python3
"""audit.py 正式自測（skill 內建測試）。用法：python scripts/test_audit.py"""
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

AUDIT_PATH = Path(__file__).parent / "audit.py"
spec = importlib.util.spec_from_file_location("audit", AUDIT_PATH)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)

GOOD_HTML = """<!DOCTYPE html><html><body>
<form>
  <label for="email">Email</label>
  <input type="email" id="email" style="color:#111111; font-size:16px">
  <label for="pw">密碼</label>
  <input type="password" id="pw" style="color:#111111">
  <button type="submit" style="height:48px;width:220px">登入</button>
  <a href="#">忘記密碼？</a>
</form>
<button type="button">次要動作</button>
</body></html>"""

BAD_HTML = """<!DOCTYPE html><html><body>
<form>
  <input type="email" id="email" placeholder="輸入 email" style="color:#888888; font-size:14px">
  <input type="password" id="pw" placeholder="密碼" style="color:#888888">
  <button type="submit" style="height:48px;width:200px">登入</button>
  <button type="button" style="height:48px;width:200px">註冊</button>
</form>
<div onclick="doSomething()">按這裡</div>
<a href="#" style="height:18px">小連結</a>
</body></html>"""


class TestAudit(unittest.TestCase):
    def test_good_html_scores_high(self):
        r = audit.audit(GOOD_HTML)
        self.assertGreaterEqual(r["score"], 90, f"好案例應 ≥90，得到 {r['score']}")

    def test_bad_html_fails_gate(self):
        r = audit.audit(BAD_HTML)
        self.assertLess(r["score"], 80, f"壞案例應 <80，得到 {r['score']}")

    def test_bad_html_catches_all_categories(self):
        r = audit.audit(BAD_HTML)
        cats = {cat for _, cat, _ in r["issues"]}
        for expected in ("contrast", "semantic", "form", "hierarchy", "target-size"):
            self.assertIn(expected, cats, f"缺少 [{expected}] 檢查")

    def test_good_html_no_label_for_false_positive(self):
        r = audit.audit(GOOD_HTML)
        cats = {cat for _, cat, _ in r["issues"]}
        self.assertNotIn("form", cats, "label for 關聯不應誤報")

    def test_json_serializable(self):
        r = audit.audit(BAD_HTML)
        json.dumps({"score": r["score"], "issues": r["issues"]}, ensure_ascii=False)

    def test_file_input_matches_string_input(self):
        r_str = audit.audit(BAD_HTML)
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "test.html"
            p.write_text(BAD_HTML, encoding="utf-8")
            r_file = audit.audit(p.read_text(encoding="utf-8"))
        self.assertEqual(r_file["score"], r_str["score"])

    def test_icon_button_without_label_detected(self):
        html = '<button style="height:32px;width:32px"></button>'
        r = audit.audit(html)
        cats = {cat for _, cat, _ in r["issues"]}
        self.assertIn("aria", cats, "無文字無 aria-label 的圖示按鈕應被抓到")

    def test_icon_button_with_aria_label_ok(self):
        html = '<button aria-label="搜尋" style="height:32px;width:32px"></button>'
        r = audit.audit(html)
        cats = {cat for _, cat, _ in r["issues"]}
        self.assertNotIn("aria", cats)

    def test_outline_none_detected(self):
        html = '<a href="#" style="outline:none">連結</a>'
        r = audit.audit(html)
        cats = {cat for _, cat, _ in r["issues"]}
        self.assertIn("focus", cats, "outline:none 應被抓到")

    def test_animation_without_reduced_motion_detected(self):
        html = '<div style="animation: fade 1s"></div>'
        r = audit.audit(html)
        cats = {cat for _, cat, _ in r["issues"]}
        self.assertIn("motion", cats)

    def test_animation_with_reduced_motion_ok(self):
        html = '<style>@media (prefers-reduced-motion: reduce) { * { animation: none } }</style><div style="animation: fade 1s"></div>'
        r = audit.audit(html)
        cats = {cat for _, cat, _ in r["issues"]}
        self.assertNotIn("motion", cats)

    def test_benchmark_suite(self):
        """benchmarks/ 案例集回歸測試：每個案例的期望分數區間"""
        bench_dir = Path(__file__).parent.parent / "benchmarks"
        expectations = {
            "login-good.html": (90, 101),
            "login-bad.html": (0, 79),
            "dashboard-good.html": (90, 101),
            "dashboard-bad.html": (0, 79),
            "settings-good.html": (90, 101),
            "settings-bad.html": (0, 79),
        }
        for fname, (lo, hi) in expectations.items():
            p = bench_dir / fname
            self.assertTrue(p.exists(), f"缺少 benchmark 檔 {fname}")
            r = audit.audit(p.read_text(encoding="utf-8"))
            self.assertTrue(lo <= r["score"] <= hi,
                f"{fname}: 期望 {lo}-{hi}，實際 {r['score']}（問題：{[msg for _,_,msg in r['issues']]}）")


if __name__ == "__main__":
    unittest.main(verbosity=2)
