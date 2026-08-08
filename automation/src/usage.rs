// 帳號使用狀態：標記使用過 → 冷卻期間不可用 → 冷卻結束恢復可用
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountUsage {
    pub last_used_at: Option<u64>, // epoch ms；None = 從未使用
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountStatus {
    /// 可用（未使用過，或已過冷卻）
    Available,
    /// 冷卻中，剩餘分鐘
    Cooling { remain_min: u64 },
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct UsageStore {
    map: HashMap<String, AccountUsage>,
}

impl UsageStore {
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

    /// 確保帳號有 entry（載入帳號時同步）
    pub fn ensure(&mut self, email: &str) {
        self.map.entry(email.to_string()).or_default();
    }

    /// 標記為已使用（進入冷卻）
    pub fn mark_used(&mut self, email: &str) {
        self.map
            .entry(email.to_string())
            .or_default()
            .last_used_at = Some(now_ms());
    }

    /// 查狀態
    pub fn status(&self, email: &str, cooldown_min: u64) -> AccountStatus {
        match self.map.get(email).and_then(|u| u.last_used_at) {
            None => AccountStatus::Available,
            Some(t) => {
                let cd_ms = cooldown_min.saturating_mul(60_000);
                let elapsed = now_ms().saturating_sub(t);
                if cd_ms == 0 || elapsed >= cd_ms {
                    AccountStatus::Available
                } else {
                    AccountStatus::Cooling {
                        remain_min: (cd_ms - elapsed).div_ceil(60_000),
                    }
                }
            }
        }
    }

    pub fn is_available(&self, email: &str, cooldown_min: u64) -> bool {
        self.status(email, cooldown_min) == AccountStatus::Available
    }
}
