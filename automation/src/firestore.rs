// Firestore 存取：用 Firebase Auth ID token 走 REST API（與網頁同帳號）
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FirebaseConfig {
    pub api_key: String,
    pub project_id: String,
    pub email: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub email: String,
    pub password: String,
    pub note: String,
    pub domain: String,
}

#[derive(Clone)]
pub struct FirestoreClient {
    http: reqwest::Client,
    config: FirebaseConfig,
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct SignInResponse {
    #[serde(rename = "idToken")]
    id_token: Option<String>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

impl FirestoreClient {
    pub fn new(config: FirebaseConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
            id_token: None,
        }
    }

    /// 登入 Firebase Auth，取得 ID token
    pub async fn login(&mut self) -> Result<()> {
        let url = format!(
            "https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?key={}",
            self.config.api_key
        );
        let resp: SignInResponse = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "email": self.config.email,
                "password": self.config.password,
                "returnSecureToken": true,
            }))
            .send()
            .await?
            .json()
            .await?;

        match resp.id_token {
            Some(tok) => {
                self.id_token = Some(tok);
                Ok(())
            }
            None => Err(anyhow!(
                "Firebase 登入失敗: {}",
                resp.error.map(|e| e.message).unwrap_or("未知錯誤".into())
            )),
        }
    }

    fn token(&self) -> Result<&str> {
        self.id_token
            .as_deref()
            .ok_or_else(|| anyhow!("尚未登入 Firebase"))
    }

    /// 讀取全部帳號（不分域名，供資料夾分組顯示）
    pub async fn fetch_all_accounts(&mut self) -> Result<Vec<Account>> {
        let url = format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents:runQuery",
            self.config.project_id
        );
        for attempt in 0..2 {
            let body = serde_json::json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "accounts" }]
                }
            });
            let resp = self
                .http
                .post(&url)
                .bearer_auth(self.token()?)
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                let txt = resp.text().await?;
                if attempt == 0 && (txt.contains("UNAUTHENTICATED") || txt.contains("permission denied")) {
                    self.login().await?;
                    continue;
                }
                return Err(anyhow!("讀取全部帳號失敗: {}", txt));
            }
            let rows: Vec<serde_json::Value> = resp.json().await?;
            let mut accounts = Vec::new();
            for row in rows {
                let doc = row.get("document").or(row.get("doc"));
                let Some(doc) = doc else { continue };
                let Some(fields) = doc.get("fields") else { continue };
                let get = |k: &str| -> String {
                    fields
                        .get(k)
                        .and_then(|v| v.get("stringValue"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                accounts.push(Account {
                    email: get("account"),
                    password: get("password"),
                    note: get("note"),
                    domain: get("domain"),
                });
            }
            return Ok(accounts);
        }
        Err(anyhow!("登入重試後仍失敗"))
    }


}
