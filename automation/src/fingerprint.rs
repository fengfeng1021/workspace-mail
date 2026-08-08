// 指紋管理：每帳號生成一次固定指紋（時區/語言固定台灣，其餘隨機），存檔後永久套用
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fingerprint {
    /// 隨機 Chrome UA（保留 Chrome 版本結構）
    pub user_agent: String,
    /// 固定台灣時區
    pub timezone: String,
    /// 固定台灣語言
    pub locale: String,
    /// Accept-Language
    pub accept_language: String,
    /// 隨機 viewport 寬
    pub width: u32,
    /// 隨機 viewport 高
    pub height: u32,
    /// canvas/WebGL 雜訊種子（固定）
    pub noise_seed: u32,
    /// 隨機硬體並行數
    pub hardware_concurrency: u32,
    /// 隨機裝置記憶體 (GB)
    pub device_memory: u32,
    /// 螢幕色彩深度
    pub color_depth: u32,
    /// 平台（固定 Windows）
    pub platform: String,
}

/// 產生一組新指紋（時區/語言固定台灣，其餘隨機）
pub fn generate(seed: u64) -> Fingerprint {
    use rand_simple_fixed::*;
    // 用固定種子產生可重現的隨機（相同 seed → 相同指紋）
    let mut rng = SimpleRng::new(seed);

    let chrome_major = 118 + (rng.next() % 30); // 118-147
    let chrome_minor = rng.next() % 100;
    let chrome_build = rng.next() % 100;
    let chrome_patch = rng.next() % 1000;
    let user_agent = format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.{}.{}.{} Safari/537.36",
        chrome_major, chrome_minor, chrome_build, chrome_patch
    );

    Fingerprint {
        user_agent,
        timezone: "Asia/Taipei".into(),
        locale: "zh-TW".into(),
        accept_language: "zh-TW,zh;q=0.9,en-US;q=0.8,en;q=0.7".into(),
        width: (1024 + (rng.next() % 513)) as u32, // 1024-1536
        height: (700 + (rng.next() % 301)) as u32, // 700-1000
        noise_seed: rng.next() as u32,
        hardware_concurrency: (4 + (rng.next() % 13)) as u32, // 4-16
        device_memory: (4 + (rng.next() % 9)) as u32,         // 4-12 GB
        color_depth: if rng.next() % 2 == 0 { 24 } else { 30 },
        platform: "Win32".into(),
    }
}

/// 指紋庫：email → Fingerprint
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct FingerprintStore {
    map: HashMap<String, Fingerprint>,
}

impl FingerprintStore {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// 取指定帳號的指紋；不存在則生成並儲存
    pub fn get_or_create(&mut self, email: &str) -> Fingerprint {
        if let Some(fp) = self.map.get(email) {
            return fp.clone();
        }
        // seed 由 email 決定 → 同帳號永遠同指紋
        let seed = fnv_hash(email);
        let fp = generate(seed);
        self.map.insert(email.to_string(), fp.clone());
        fp
    }
}

/// 簡單 FNV-1a 雜湊（不引入額外依賴）
fn fnv_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 注入頁面的指紋偽裝 JS（canvas/WebGL/Audio 雜訊，固定種子）
pub fn fingerprint_js(fp: &Fingerprint) -> String {
    let seed = fp.noise_seed;
    format!(
        r#"
(function() {{
  const seed = {seed};
  let state = seed;
  function rnd() {{ state = (state * 1664525 + 1013904223) >>> 0; return state / 4294967296; }}
  function noise(v) {{ return v + (rnd() - 0.5) * 0.002; }}

  // Canvas 雜訊
  const origGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function(type, ...args) {{
    const ctx = origGetContext.call(this, type, ...args);
    if (ctx && (type === '2d')) {{
      const origFillText = ctx.fillText;
      ctx.fillText = function(text, x, y, ...rest) {{
        return origFillText.call(this, text, x + rnd() * 0.02, y + rnd() * 0.02, ...rest);
      }};
      const origGetImageData = ctx.getImageData;
      ctx.getImageData = function(...args) {{
        const img = origGetImageData.apply(this, args);
        for (let i = 0; i < img.data.length; i += 4) {{
          img.data[i] ^= Math.floor(rnd() * 2);
          img.data[i+1] ^= Math.floor(rnd() * 2);
          img.data[i+2] ^= Math.floor(rnd() * 2);
        }}
        return img;
      }};
    }}
    return ctx;
  }};

  // WebGL 雜訊
  const origGetParameter = WebGLRenderingContext.prototype.getParameter;
  if (origGetParameter) {{
    WebGLRenderingContext.prototype.getParameter = function(param, ...args) {{
      const v = origGetParameter.call(this, param, ...args);
      if (typeof v === 'number' && v !== 0) return noise(v);
      return v;
    }};
  }}

  // Audio 雜訊
  const origGetChannelData = AudioBuffer.prototype.getChannelData;
  if (origGetChannelData) {{
    AudioBuffer.prototype.getChannelData = function(...args) {{
      const data = origGetChannelData.apply(this, args);
      for (let i = 0; i < data.length; i++) data[i] = data[i] * (1 + (rnd() - 0.5) * 0.002);
      return data;
    }};
  }}

  // 硬體資訊覆蓋
  Object.defineProperty(navigator, 'hardwareConcurrency', {{ get: () => {hw} }});
  Object.defineProperty(navigator, 'deviceMemory', {{ get: () => {dm} }});
  Object.defineProperty(navigator, 'platform', {{ get: () => '{platform}' }});
  Object.defineProperty(screen, 'colorDepth', {{ get: () => {cd} }});

  // 語言覆蓋（navigator.language 不受 CDP setLocaleOverride 控制，需 JS 覆蓋）
  Object.defineProperty(navigator, 'language', {{ get: () => 'zh-TW' }});
  Object.defineProperty(navigator, 'languages', {{ get: () => ['zh-TW', 'zh', 'en-US'] }});
}})();
"#,
        seed = seed,
        hw = fp.hardware_concurrency,
        dm = fp.device_memory,
        platform = fp.platform,
        cd = fp.color_depth,
    )
}

/// 純隨機（無 rand crate 依賴，用 XORShift）
mod rand_simple_fixed {
    pub struct SimpleRng(u64);
    impl SimpleRng {
        pub fn new(seed: u64) -> Self {
            Self(seed.max(1))
        }
        pub fn next(&mut self) -> u64 {
            // xorshift64*
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545F4914F6CDD1D)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_account_fixed_fingerprint() {
        let mut store = FingerprintStore::default();
        let fp1 = store.get_or_create("a01@aiapi.tw");
        let fp2 = store.get_or_create("a01@aiapi.tw");
        assert_eq!(fp1.user_agent, fp2.user_agent);
        assert_eq!(fp1.noise_seed, fp2.noise_seed);
        assert_eq!(fp1.width, fp2.width);
    }

    #[test]
    fn different_accounts_differ() {
        let mut store = FingerprintStore::default();
        let fp1 = store.get_or_create("a01@aiapi.tw");
        let fp2 = store.get_or_create("a02@aiapi.tw");
        assert_ne!(fp1.user_agent, fp2.user_agent);
        assert_ne!(fp1.noise_seed, fp2.noise_seed);
    }

    #[test]
    fn taiwan_timezone_and_language_fixed() {
        let mut store = FingerprintStore::default();
        for email in ["a01@aiapi.tw", "a02@aiapi.tw", "a03@aiapi.tw"] {
            let fp = store.get_or_create(email);
            assert_eq!(fp.timezone, "Asia/Taipei");
            assert_eq!(fp.locale, "zh-TW");
            assert!(fp.accept_language.starts_with("zh-TW"));
        }
    }

    #[test]
    fn user_agent_shape_valid() {
        let mut store = FingerprintStore::default();
        let fp = store.get_or_create("a01@aiapi.tw");
        assert!(fp.user_agent.contains("Windows NT 10.0"));
        assert!(fp.user_agent.contains("Chrome/"));
        assert!(fp.user_agent.contains("Safari/537.36"));
    }

    #[test]
    fn store_roundtrip_persists() {
        let mut store = FingerprintStore::default();
        store.get_or_create("a01@aiapi.tw");
        let path = std::env::temp_dir().join("fp-test-roundtrip.json");
        store.save(&path).unwrap();
        let mut loaded = FingerprintStore::load(&path);
        let fp = loaded.get_or_create("a01@aiapi.tw");
        assert_eq!(fp.user_agent, store.get_or_create("a01@aiapi.tw").user_agent);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fingerprint_js_contains_expected_overrides() {
        let mut store = FingerprintStore::default();
        let fp = store.get_or_create("a01@aiapi.tw");
        let js = fingerprint_js(&fp);
        assert!(js.contains("hardwareConcurrency"));
        assert!(js.contains("zh-TW"));
        assert!(js.contains("getImageData"));
    }
}
