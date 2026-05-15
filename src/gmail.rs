use crate::config::AppState;
use crate::oauth;
use anyhow::Result;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct GmailClient {
    pub access_token: String,
    http: reqwest::Client,
}

impl GmailClient {
    pub async fn new(state: &Arc<AppState>, account_id: &str) -> Result<Self> {
        let mut account = state
            .get_account(account_id)?
            .ok_or_else(|| anyhow::anyhow!("account '{}' not found", account_id))?;

        let now = chrono::Utc::now().timestamp();
        if account.access_token.is_none()
            || account.expires_at.map(|e| e < now + 60).unwrap_or(true)
        {
            oauth::refresh_token(&mut account).await?;
            state.upsert_account(account.clone())?;
        }

        let access_token = account
            .access_token
            .ok_or_else(|| anyhow::anyhow!("account '{}' is not authenticated", account_id))?;

        Ok(Self {
            access_token,
            http: reqwest::Client::new(),
        })
    }

    pub async fn list_messages(&self, folder: &str, max: usize) -> Result<Value> {
        let label = match folder {
            "inbox" | "" => "INBOX",
            "sent" => "SENT",
            "spam" => "SPAM",
            "trash" => "TRASH",
            other => other,
        };

        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults={}&labelIds={}",
            max, label
        );

        let list: Value = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await?;

        let ids: Vec<String> = list["messages"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let mut handles = vec![];
        for id in ids.iter().take(max) {
            let url = format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
                id
            );
            let http = self.http.clone();
            let token = self.access_token.clone();
            handles.push(tokio::spawn(
                async move { http.get(&url).bearer_auth(&token).send().await?.json::<Value>().await },
            ));
        }

        let mut messages = vec![];
        for handle in handles {
            if let Ok(Ok(msg)) = handle.await {
                messages.push(summarize(&msg));
            }
        }

        Ok(json!({
            "messages": messages,
            "total_estimate": list["resultSizeEstimate"],
        }))
    }

    pub async fn get_message(&self, message_id: &str) -> Result<Value> {
        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=full",
            message_id
        );
        let msg: Value = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await?;

        Ok(json!({
            "id": msg["id"],
            "thread_id": msg["threadId"],
            "subject": header(&msg, "Subject"),
            "from": header(&msg, "From"),
            "to": header(&msg, "To"),
            "cc": header(&msg, "Cc"),
            "date": header(&msg, "Date"),
            "body": extract_body(&msg["payload"]),
            "snippet": msg["snippet"],
            "unread": msg["labelIds"].as_array()
                .map(|ids| ids.iter().any(|id| id == "UNREAD"))
                .unwrap_or(false),
        }))
    }

    pub async fn send_message(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        cc: Option<&str>,
    ) -> Result<String> {
        let mut raw = format!(
            "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n",
            to, subject
        );
        if let Some(cc) = cc {
            raw.push_str(&format!("Cc: {}\r\n", cc));
        }
        raw.push_str("\r\n");
        raw.push_str(body);

        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());
        let resp: Value = self
            .http
            .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
            .bearer_auth(&self.access_token)
            .json(&json!({ "raw": encoded }))
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp["error"].as_object() {
            anyhow::bail!("{}", err["message"].as_str().unwrap_or("send failed"));
        }

        Ok(resp["id"].as_str().unwrap_or("").to_string())
    }

    pub async fn search(&self, query: &str, max: usize) -> Result<Value> {
        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults={}&q={}",
            max,
            urlencoding::encode(query)
        );

        let list: Value = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await?;

        let ids: Vec<String> = list["messages"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let mut handles = vec![];
        for id in ids.iter().take(max) {
            let url = format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
                id
            );
            let http = self.http.clone();
            let token = self.access_token.clone();
            handles.push(tokio::spawn(
                async move { http.get(&url).bearer_auth(&token).send().await?.json::<Value>().await },
            ));
        }

        let mut messages = vec![];
        for handle in handles {
            if let Ok(Ok(msg)) = handle.await {
                messages.push(summarize(&msg));
            }
        }

        Ok(json!({ "messages": messages }))
    }
}

fn header(msg: &Value, name: &str) -> String {
    msg["payload"]["headers"]
        .as_array()
        .and_then(|hs| {
            hs.iter().find(|h| {
                h["name"]
                    .as_str()
                    .map(|n| n.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
            })
        })
        .and_then(|h| h["value"].as_str())
        .unwrap_or("")
        .to_string()
}

fn summarize(msg: &Value) -> Value {
    json!({
        "id": msg["id"],
        "thread_id": msg["threadId"],
        "subject": header(msg, "Subject"),
        "from": header(msg, "From"),
        "date": header(msg, "Date"),
        "snippet": msg["snippet"],
        "unread": msg["labelIds"].as_array()
            .map(|ids| ids.iter().any(|id| id == "UNREAD"))
            .unwrap_or(false),
    })
}

fn extract_body(payload: &Value) -> String {
    if let Some(data) = payload["body"]["data"].as_str() {
        if !data.is_empty() {
            if let Ok(bytes) = URL_SAFE_NO_PAD.decode(data) {
                return String::from_utf8_lossy(&bytes).to_string();
            }
        }
    }

    if let Some(parts) = payload["parts"].as_array() {
        for mime in ["text/plain", "text/html"] {
            for part in parts {
                if part["mimeType"].as_str() == Some(mime) {
                    if let Some(data) = part["body"]["data"].as_str() {
                        if let Ok(bytes) = URL_SAFE_NO_PAD.decode(data) {
                            return String::from_utf8_lossy(&bytes).to_string();
                        }
                    }
                }
            }
        }
        for part in parts {
            let nested = extract_body(part);
            if !nested.is_empty() {
                return nested;
            }
        }
    }

    String::new()
}
