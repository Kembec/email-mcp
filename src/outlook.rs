use crate::config::AppState;
use crate::oauth;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct OutlookClient {
    access_token: String,
    http: reqwest::Client,
}

impl OutlookClient {
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
        let folder_path = match folder {
            "inbox" | "" => "Inbox",
            "sent" => "SentItems",
            "drafts" => "Drafts",
            "trash" => "DeletedItems",
            "spam" => "JunkEmail",
            other => other,
        };

        let url = format!(
            "https://graph.microsoft.com/v1.0/me/mailFolders/{}/messages?$top={}&$select=id,subject,from,receivedDateTime,isRead,bodyPreview&$orderby=receivedDateTime desc",
            folder_path, max
        );

        let resp: Value = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp["error"].as_object() {
            anyhow::bail!("{}", err["message"].as_str().unwrap_or("API error"));
        }

        let messages: Vec<Value> = resp["value"]
            .as_array()
            .map(|arr| arr.iter().map(summarize).collect())
            .unwrap_or_default();

        Ok(json!({ "messages": messages }))
    }

    pub async fn get_message(&self, message_id: &str) -> Result<Value> {
        let url = format!("https://graph.microsoft.com/v1.0/me/messages/{}", message_id);
        let msg: Value = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = msg["error"].as_object() {
            anyhow::bail!("{}", err["message"].as_str().unwrap_or("API error"));
        }

        let to_addrs: Vec<&str> = msg["toRecipients"]
            .as_array()
            .map(|r| {
                r.iter()
                    .filter_map(|x| x["emailAddress"]["address"].as_str())
                    .collect()
            })
            .unwrap_or_default();

        Ok(json!({
            "id": msg["id"],
            "subject": msg["subject"],
            "from": msg["from"]["emailAddress"]["address"],
            "to": to_addrs,
            "date": msg["receivedDateTime"],
            "body": msg["body"]["content"],
            "body_type": msg["body"]["contentType"],
            "is_read": msg["isRead"],
        }))
    }

    pub async fn send_message(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        cc: Option<&str>,
    ) -> Result<String> {
        let mut cc_recipients: Vec<Value> = vec![];
        if let Some(addr) = cc {
            cc_recipients.push(json!({ "emailAddress": { "address": addr } }));
        }

        let payload = json!({
            "message": {
                "subject": subject,
                "body": { "contentType": "Text", "content": body },
                "toRecipients": [{ "emailAddress": { "address": to } }],
                "ccRecipients": cc_recipients,
            }
        });

        let resp = self
            .http
            .post("https://graph.microsoft.com/v1.0/me/sendMail")
            .bearer_auth(&self.access_token)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await?;
            anyhow::bail!("Send failed: {body}");
        }

        Ok("sent".to_string())
    }

    pub async fn search(&self, query: &str, max: usize) -> Result<Value> {
        // Graph API search requires $search with KQL
        let url = format!(
            "https://graph.microsoft.com/v1.0/me/messages?$search={}&$top={}&$select=id,subject,from,receivedDateTime,isRead,bodyPreview",
            serde_json::to_string(query).unwrap_or_default(),
            max
        );

        let resp: Value = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .header("ConsistencyLevel", "eventual")
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp["error"].as_object() {
            anyhow::bail!("{}", err["message"].as_str().unwrap_or("API error"));
        }

        let messages: Vec<Value> = resp["value"]
            .as_array()
            .map(|arr| arr.iter().map(summarize).collect())
            .unwrap_or_default();

        Ok(json!({ "messages": messages }))
    }
}

fn summarize(msg: &Value) -> Value {
    json!({
        "id": msg["id"],
        "subject": msg["subject"],
        "from": msg["from"]["emailAddress"]["address"],
        "date": msg["receivedDateTime"],
        "snippet": msg["bodyPreview"],
        "unread": !msg["isRead"].as_bool().unwrap_or(true),
    })
}
