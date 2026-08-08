// ===== Workspace Mail — 主程式 =====
import { initializeApp } from "https://www.gstatic.com/firebasejs/10.12.2/firebase-app.js";
import {
  getAuth, onAuthStateChanged, signInWithEmailAndPassword, signOut
} from "https://www.gstatic.com/firebasejs/10.12.2/firebase-auth.js";
import {
  getFirestore, collection, doc, addDoc, setDoc, getDocs, query, where,
  orderBy, deleteDoc, onSnapshot, writeBatch
} from "https://www.gstatic.com/firebasejs/10.12.2/firebase-firestore.js";

// ========== 設定區（Firebase 建立好後填入） ==========
const FIREBASE_CONFIG = {
  apiKey: "AIzaSyBJ_TWOMc7IHhI-5at21Aru0Bf1U-qpFBM",
  authDomain: "mailhub-d64e2.firebaseapp.com",
  projectId: "mailhub-d64e2",
  storageBucket: "mailhub-d64e2.firebasestorage.app",
  messagingSenderId: "856993222359",
  appId: "1:856993222359:web:881605dbb66c732f4d6a92"
};

// AI 接口（預留）：呼叫後預期回傳 JSON 陣列 [{"email","password","note"}]
const AI_ENDPOINT = ""; // 例如 "https://your-ai-server.example.com/api/generate-accounts"
const AI_API_KEY = "";  // 選填
// ==================================================

const app = initializeApp(FIREBASE_CONFIG);
const auth = getAuth(app);
const db = getFirestore(app);

// ---------- 狀態 ----------
let currentDomain = null;   // 目前選中的域名（字串）
let allDomains = [];        // 域名快取 [{id, name, count}]
let allAccounts = [];       // 目前域名的帳號快取（已排序）
let searchTerm = "";
let editingId = null;       // 編輯中的帳號 doc id

// ---------- DOM ----------
const $ = (id) => document.getElementById(id);
const screens = { login: $("login-screen"), main: $("main-screen") };

// ---------- 登入 ----------
onAuthStateChanged(auth, (user) => {
  if (user) {
    screens.login.classList.add("hidden");
    screens.main.classList.remove("hidden");
    $("user-badge").textContent = user.email;
    init();
  } else {
    screens.login.classList.remove("hidden");
    screens.main.classList.add("hidden");
  }
});

$("login-btn").addEventListener("click", async () => {
  const email = $("login-email").value.trim();
  const pw = $("login-password").value;
  const msg = $("login-msg");
  msg.className = "msg"; msg.textContent = "登入中…";
  try {
    await signInWithEmailAndPassword(auth, email, pw);
    msg.textContent = "";
  } catch (e) {
    msg.className = "msg err";
    msg.textContent = e.code === "auth/invalid-credential"
      ? "信箱或密碼錯誤" : "登入失敗：" + e.message;
  }
});

$("logout-btn").addEventListener("click", () => signOut(auth));

// ---------- 初始化：監聽域名 ----------
function init() {
  onSnapshot(collection(db, "domains"), async (snap) => {
    allDomains = snap.docs.map(d => ({ id: d.id, ...d.data() }));
    renderDomains();
    if (!currentDomain && allDomains.length > 0) {
      selectDomain(allDomains[0].name);
    }
  });
}

// ---------- 左側域名列表（A-Z 排序） ----------
function renderDomains() {
  const list = $("domain-list");
  const sorted = [...allDomains].sort((a, b) => a.name.localeCompare(b.name));
  list.innerHTML = "";
  for (const d of sorted) {
    const li = document.createElement("li");
    li.className = "domain-item" + (d.name === currentDomain ? " active" : "");
    const counts = allAccounts.filter(a => a.domain === d.name).length;
    li.innerHTML = `
      <span class="name"></span>
      <span class="count"></span>
      <button class="del" title="刪除域名">✕</button>`;
    li.querySelector(".name").textContent = d.name;
    li.querySelector(".count").textContent = counts || "0";
    li.querySelector(".name").addEventListener("click", () => selectDomain(d.name));
    li.querySelector(".del").addEventListener("click", (e) => {
      e.stopPropagation();
      deleteDomain(d);
    });
    list.appendChild(li);
  }
}

function selectDomain(name) {
  currentDomain = name;
  $("current-domain-title").textContent = name;
  renderDomains();
  loadAccounts(name);
}

// ---------- 新增域名 ----------
$("add-domain-btn").addEventListener("click", async () => {
  const name = prompt("輸入域名（例如 example.com）：");
  if (!name || !name.trim()) return;
  const n = name.trim().toLowerCase();
  if (allDomains.some(d => d.name === n)) { toast("該域名已存在", "err"); return; }
  await addDoc(collection(db, "domains"), { name: n, createdAt: Date.now() });
  toast("域名已新增");
});

async function deleteDomain(d) {
  if (!confirm(`確定刪除域名「${d.name}」以及底下所有帳號？`)) return;
  // 刪除該域名所有帳號
  const q = query(collection(db, "accounts"), where("domain", "==", d.name));
  const snap = await getDocs(q);
  const batch = writeBatch(db);
  snap.docs.forEach(x => batch.delete(x.ref));
  await batch.commit();
  await deleteDoc(doc(db, "domains", d.id));
  if (currentDomain === d.name) {
    currentDomain = null;
    $("current-domain-title").textContent = "請選擇域名";
    $("account-tbody").innerHTML = "";
    $("account-empty").classList.remove("hidden");
  }
  toast("域名已刪除");
}

// ---------- 帳號載入（A-Z 排序） ----------
function loadAccounts(domain) {
  const q = query(
    collection(db, "accounts"),
    where("domain", "==", domain),
    orderBy("sortKey", "asc")
  );
  onSnapshot(q, (snap) => {
    allAccounts = snap.docs.map(d => ({ id: d.id, ...d.data() }));
    renderAccounts();
  });
}

function renderAccounts() {
  const tbody = $("account-tbody");
  const empty = $("account-empty");
  tbody.innerHTML = "";
  const term = searchTerm.toLowerCase();
  const rows = allAccounts.filter(a =>
    !term || a.account.toLowerCase().includes(term) || (a.note || "").toLowerCase().includes(term)
  );
  empty.classList.toggle("hidden", rows.length > 0);
  $("select-all").checked = false;
  $("account-toolbar").classList.add("hidden");

  for (const acc of rows) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="col-check"><input type="checkbox" class="row-check" data-id="${acc.id}"></td>
      <td class="acc-email"></td>
      <td><div class="pw-cell">
          <span class="pw-text pw-masked" data-id="${acc.id}">••••••••</span>
          <button class="row-btn pw-toggle" data-id="${acc.id}" title="顯示/隱藏密碼">👁</button>
          <button class="row-btn pw-copy" data-id="${acc.id}" title="複製密碼">📋</button>
        </div></td>
      <td class="note"></td>
      <td class="col-actions">
        <button class="row-btn edit" data-id="${acc.id}">✏️ 編輯</button>
        <button class="row-btn danger del" data-id="${acc.id}">🗑</button>
      </td>`;
    tr.querySelector(".acc-email").textContent = acc.account;
    tr.querySelector(".note").textContent = acc.note || "";
    // 複製帳號點擊
    tr.querySelector(".acc-email").style.cursor = "copy";
    tr.querySelector(".acc-email").title = "點擊複製帳號";
    tr.querySelector(".acc-email").addEventListener("click", async () => {
      await navigator.clipboard.writeText(acc.account);
      toast("帳號已複製");
    });
    tr.querySelector(".pw-toggle").addEventListener("click", () => togglePw(acc.id, acc.password));
    tr.querySelector(".pw-copy").addEventListener("click", async () => {
      await navigator.clipboard.writeText(acc.password);
      toast("密碼已複製");
    });
    tr.querySelector(".edit").addEventListener("click", () => openAccountModal(acc));
    tr.querySelector(".del").addEventListener("click", () => deleteAccount(acc));
    tr.querySelector(".row-check").addEventListener("change", updateToolbar);
    tbody.appendChild(tr);
  }
  updateToolbar();
}

function togglePw(id, pw) {
  const span = document.querySelector(`.pw-text[data-id="${id}"]`);
  if (!span) return;
  if (span.classList.contains("pw-masked")) {
    span.textContent = pw;
    span.classList.remove("pw-masked");
  } else {
    span.textContent = "••••••••";
    span.classList.add("pw-masked");
  }
}

// ---------- 刪除帳號 ----------
async function deleteAccount(acc) {
  if (!confirm(`確定刪除 ${acc.account}？`)) return;
  await deleteDoc(doc(db, "accounts", acc.id));
  toast("已刪除", "ok");
}

// ---------- 批量刪除（勾選） ----------
function updateToolbar() {
  const checks = [...document.querySelectorAll(".row-check:checked")];
  $("selected-count").textContent = `已選 ${checks.length} 項`;
  $("account-toolbar").classList.toggle("hidden", checks.length === 0);
}
$("select-all").addEventListener("change", (e) => {
  document.querySelectorAll(".row-check").forEach(c => c.checked = e.target.checked);
  updateToolbar();
});
$("delete-selected-btn").addEventListener("click", async () => {
  const ids = [...document.querySelectorAll(".row-check:checked")].map(c => c.dataset.id);
  if (!ids.length) return;
  if (!confirm(`確定刪除所選 ${ids.length} 個帳號？`)) return;
  const batch = writeBatch(db);
  ids.forEach(id => batch.delete(doc(db, "accounts", id)));
  await batch.commit();
  toast(`已刪除 ${ids.length} 個帳號`);
});

// ---------- 新增 / 編輯帳號 Modal ----------
$("add-account-btn").addEventListener("click", () => openAccountModal(null));

function openAccountModal(acc) {
  editingId = acc ? acc.id : null;
  $("account-modal-title").textContent = acc ? "編輯帳號" : "新增帳號";
  $("acc-email").value = acc ? acc.account : "";
  $("acc-password").value = acc ? acc.password : "";
  $("acc-note").value = acc ? (acc.note || "") : "";
  $("acc-msg").textContent = "";
  $("account-modal").classList.remove("hidden");
}
$("acc-cancel").addEventListener("click", () => $("account-modal").classList.add("hidden"));
$("acc-save").addEventListener("click", async () => {
  const email = $("acc-email").value.trim();
  const pw = $("acc-password").value.trim();
  const note = $("acc-note").value.trim();
  const msg = $("acc-msg");
  if (!currentDomain) { msg.className = "msg err"; msg.textContent = "請先選擇域名"; return; }
  if (!email) { msg.className = "msg err"; msg.textContent = "請輸入帳號"; return; }
  if (!pw) { msg.className = "msg err"; msg.textContent = "請輸入密碼"; return; }
  try {
    const data = { domain: currentDomain, account: email, password: pw, note, sortKey: email.toLowerCase(), updatedAt: Date.now() };
    if (editingId) {
      await setDoc(doc(db, "accounts", editingId), data, { merge: true });
    } else {
      data.createdAt = Date.now();
      await addDoc(collection(db, "accounts"), data);
    }
    $("account-modal").classList.add("hidden");
    toast(editingId ? "已更新" : "已新增");
  } catch (e) {
    msg.className = "msg err"; msg.textContent = "失敗：" + e.message;
  }
});

// ---------- 批量新增 ----------
$("batch-add-btn").addEventListener("click", () => {
  if (!currentDomain) { toast("請先選擇域名", "err"); return; }
  $("batch-input").value = "";
  $("batch-msg").textContent = "";
  $("batch-modal").classList.remove("hidden");
});
$("batch-cancel").addEventListener("click", () => $("batch-modal").classList.add("hidden"));
$("batch-save").addEventListener("click", async () => {
  const text = $("batch-input").value;
  const msg = $("batch-msg");
  if (!text.trim()) { msg.className = "msg err"; msg.textContent = "請輸入內容"; return; }
  const lines = text.split("\n").map(l => l.trim()).filter(Boolean);
  const batch = writeBatch(db);
  let added = 0;
  for (const line of lines) {
    // 支援格式：帳號 密碼 備註（空白分隔）；或 CSV 逗號分隔
    let parts = line.split(/\s+/);
    if (parts.length === 1 && line.includes(",")) parts = line.split(",");
    const [account, password = "", ...noteParts] = parts.map(p => p.trim());
    if (!account || !password) {
      msg.className = "msg err";
      msg.textContent = `格式錯誤，跳過：${line}`;
      continue;
    }
    const ref = doc(collection(db, "accounts"));
    batch.set(ref, {
      domain: currentDomain, account, password,
      note: noteParts.join(" "),
      sortKey: account.toLowerCase(), createdAt: Date.now()
    });
    added++;
  }
  if (added) {
    await batch.commit();
    msg.className = "msg ok";
    msg.textContent = `✅ 已新增 ${added} 筆`;
    $("batch-input").value = "";
  }
});

// ---------- AI 新增（預留接口） ----------
$("ai-add-btn").addEventListener("click", () => {
  if (!currentDomain) { toast("請先選擇域名", "err"); return; }
  $("ai-input").value = "";
  $("ai-msg").textContent = "";
  $("ai-modal").classList.remove("hidden");
});
$("ai-cancel").addEventListener("click", () => $("ai-modal").classList.add("hidden"));
$("ai-run").addEventListener("click", async () => {
  const prompt = $("ai-input").value.trim();
  const msg = $("ai-msg");
  if (!AI_ENDPOINT) {
    msg.className = "msg err";
    msg.textContent = "尚未設定 AI 接口（app.js 的 AI_ENDPOINT）";
    return;
  }
  if (!prompt) { msg.className = "msg err"; msg.textContent = "請輸入 AI 指令"; return; }
  msg.className = "msg"; msg.textContent = "呼叫 AI 中…";
  try {
    const res = await fetch(AI_ENDPOINT, {
      method: "POST",
      headers: { "Content-Type": "application/json", ...(AI_API_KEY ? { "Authorization": `Bearer ${AI_API_KEY}` } : {}) },
      body: JSON.stringify({ prompt, domain: currentDomain })
    });
    if (!res.ok) throw new Error("HTTP " + res.status);
    const accounts = await res.json();
    if (!Array.isArray(accounts) || !accounts.length) throw new Error("AI 回傳格式不正確");
    const batch = writeBatch(db);
    for (const a of accounts) {
      if (!a.email || !a.password) continue;
      batch.set(doc(collection(db, "accounts")), {
        domain: currentDomain, account: a.email, password: a.password,
        note: a.note || "", sortKey: a.email.toLowerCase(), createdAt: Date.now()
      });
    }
    await batch.commit();
    msg.className = "msg ok";
    msg.textContent = `✅ AI 已新增 ${accounts.filter(a => a.email && a.password).length} 筆`;
  } catch (e) {
    msg.className = "msg err";
    msg.textContent = "AI 呼叫失敗：" + e.message;
  }
});

// ---------- 搜尋 ----------
$("search-box").addEventListener("input", (e) => {
  searchTerm = e.target.value;
  renderAccounts();
});

// ---------- Toast ----------
let toastTimer = null;
function toast(text, type = "ok") {
  const t = $("toast");
  t.textContent = text;
  t.className = "toast " + type;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.add("hidden"), 2200);
}
