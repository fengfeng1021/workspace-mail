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

#[derive(Deserialize)]
struct QueryResponse {
    documents: Option<Vec<QueryDoc>>,
}

#[derive(Deserialize)]
struct QueryDoc {
    fields: Option<serde_json::Value>,
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

    /// 讀取指定域名的所有帳號
    pub async fn fetch_accounts(&mut self, domain: &str) -> Result<Vec<Account>> {
        let url = format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents:runQuery",
            self.config.project_id
        );
        let body = serde_json::json!({
            "structuredQuery": {
                "from": [{"collectionId": "accounts"}],
                "where": {
                    "fieldFilter": {
                        "field": {"fieldPath": "domain"},
                        "op": "EQUAL",
                        "value": {"stringValue": domain}
                    }
                }
            }
        });

        // token 過期時重新登入重試一次（避免 async 遞迴）
        for attempt in 0..2 {
            let resp = self
                .http
                .post(&url)
                .bearer_auth(self.token()?)
                .json(&body)
                .send()
                .await?;

            if !resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                let auth_fail =
                    txt.contains("UNAUTHENTICATED") || txt.contains("permission denied");
                if attempt == 0 && auth_fail {
                    self.login().await?;
                    continue;
                }
                return Err(anyhow!("查詢失敗: {}", txt));
            }

            let docs: Vec<serde_json::Value> = resp.json().await?;
            let mut accounts = Vec::new();
            for doc in docs {
                let d = doc.get("document").and_then(|x| x.get("fields"));
                let Some(d) = d else { continue };
                let get = |k: &str| -> String {
                    d.get(k)
                        .and_then(|v| v.get("stringValue"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                accounts.push(Account {
                    email: get("account"),
                    password: get("password"),
                    note: get("note"),
                });
            }
            return Ok(accounts);
        }
        unreachable!("重試迴圈結束")
    }

    /// 列出所有域名（供 UI 下拉選單）
    pub async fn list_domains(&mut self) -> Result<Vec<String>> {
        let url = format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents/domains",
            self.config.project_id
        );

        for attempt in 0..2 {
            let resp = self
                .http
                .get(&url)
                .bearer_auth(self.token()?)
                .send()
                .await?;
            if !resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                let auth_fail =
                    txt.contains("UNAUTHENTICATED") || txt.contains("permission denied");
                if attempt == 0 && auth_fail {
                    self.login().await?;
                    continue;
                }
                return Err(anyhow!("讀取域名失敗: {}", txt));
            }
            let body: QueryResponse = resp.json().await?;
            let mut domains = Vec::new();
            if let Some(docs) = body.documents {
                for doc in docs {
                    if let Some(fields) = doc.fields {
                        if let Some(n) = fields
                            .get("name")
                            .and_then(|v| v.get("stringValue"))
                            .and_then(|v| v.as_str())
                        {
                            domains.push(n.to_string());
                        }
                    }
                }
            }
            domains.sort();
            return Ok(domains);
        }
        unreachable!("重試迴圈結束")
    }
}
