use crate::config::{Account, AppState, Provider};
use crate::{gmail, oauth, outlook};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn list() -> Value {
    json!([
        {
            "name": "list_accounts",
            "description": "List all configured email accounts and their authentication status",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "auth_start",
            "description": "Start OAuth2 authentication for a Gmail or Outlook account. Returns a URL to open in the browser; auth completes automatically when the user accepts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider": { "type": "string", "enum": ["gmail", "outlook"] },
                    "account_id": { "type": "string", "description": "Unique label for this account (e.g. 'work', 'personal')" },
                    "client_id": { "type": "string", "description": "OAuth2 client ID from Google Cloud Console or Azure AD" },
                    "client_secret": { "type": "string", "description": "OAuth2 client secret (required for Gmail; optional for Outlook public clients)" }
                },
                "required": ["provider", "account_id", "client_id"]
            }
        },
        {
            "name": "auth_add_icloud",
            "description": "Add an iCloud account using an App-Specific Password. Generate one at appleid.apple.com → Sign-In & Security → App-Specific Passwords.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "account_id": { "type": "string" },
                    "email": { "type": "string", "description": "iCloud email (e.g. user@icloud.com)" },
                    "app_password": { "type": "string", "description": "App-Specific Password from appleid.apple.com" }
                },
                "required": ["account_id", "email", "app_password"]
            }
        },
        {
            "name": "auth_remove",
            "description": "Remove a configured account",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "account_id": { "type": "string" }
                },
                "required": ["account_id"]
            }
        },
        {
            "name": "list_messages",
            "description": "List recent emails from an account",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "account_id": { "type": "string" },
                    "folder": { "type": "string", "description": "inbox (default), sent, drafts, trash, spam" },
                    "max_results": { "type": "integer", "description": "Max messages to return (default 20, max 50)" }
                },
                "required": ["account_id"]
            }
        },
        {
            "name": "get_message",
            "description": "Get full content of a specific email",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "account_id": { "type": "string" },
                    "message_id": { "type": "string" }
                },
                "required": ["account_id", "message_id"]
            }
        },
        {
            "name": "send_message",
            "description": "Send an email from a configured account",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "account_id": { "type": "string" },
                    "to": { "type": "string", "description": "Recipient email address" },
                    "subject": { "type": "string" },
                    "body": { "type": "string", "description": "Plain text body" },
                    "cc": { "type": "string", "description": "CC address (optional)" }
                },
                "required": ["account_id", "to", "subject", "body"]
            }
        },
        {
            "name": "search_messages",
            "description": "Search emails. Gmail supports operators (from:, subject:, has:attachment, etc). Outlook uses KQL syntax.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "account_id": { "type": "string" },
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "description": "Max results (default 20, max 50)" }
                },
                "required": ["account_id", "query"]
            }
        }
    ])
}

pub async fn call(name: &str, args: Value, state: &Arc<AppState>) -> Value {
    match do_call(name, args, state).await {
        Ok(v) => v,
        Err(e) => json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!("Error: {e}") }]
        }),
    }
}

async fn do_call(name: &str, args: Value, state: &Arc<AppState>) -> anyhow::Result<Value> {
    match name {
        "list_accounts" => {
            let accounts = state.load_accounts()?;
            let list: Vec<Value> = accounts
                .iter()
                .map(|a| {
                    json!({
                        "id": a.id,
                        "provider": a.provider.to_string(),
                        "email": a.email,
                        "authenticated": a.access_token.is_some() || a.icloud_password.is_some(),
                    })
                })
                .collect();

            let pending: Vec<String> = state
                .pending_auths
                .lock()
                .unwrap()
                .keys()
                .cloned()
                .collect();

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&json!({
                        "accounts": list,
                        "pending_auth": pending,
                    }))?
                }]
            }))
        }

        "auth_start" => {
            let provider = match args["provider"].as_str().unwrap_or("") {
                "gmail" => Provider::Gmail,
                "outlook" => Provider::Outlook,
                p => anyhow::bail!("unknown provider '{}'; use 'gmail' or 'outlook'", p),
            };
            let account_id = args["account_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("account_id is required"))?
                .to_string();
            let client_id = args["client_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("client_id is required"))?
                .to_string();
            let client_secret = args["client_secret"].as_str().map(|s| s.to_string());

            let result =
                oauth::start_oauth(state.clone(), provider, account_id, client_id, client_secret)
                    .await?;

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "Open this URL in your browser to authenticate:\n\n{}\n\nAuth will complete automatically when you accept. Use list_accounts to verify.",
                        result.url
                    )
                }]
            }))
        }

        "auth_add_icloud" => {
            let account_id = args["account_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("account_id is required"))?
                .to_string();
            let email = args["email"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("email is required"))?
                .to_string();
            let app_password = args["app_password"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("app_password is required"))?
                .to_string();

            let account = Account {
                id: account_id.clone(),
                provider: Provider::ICloud,
                email: Some(email),
                client_id: String::new(),
                client_secret: None,
                access_token: None,
                refresh_token: None,
                expires_at: None,
                icloud_password: Some(app_password),
            };

            state.upsert_account(account)?;
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("iCloud account '{}' added. Note: IMAP operations not yet implemented in v0.1.", account_id)
                }]
            }))
        }

        "auth_remove" => {
            let account_id = args["account_id"].as_str().unwrap_or("");
            let removed = state.remove_account(account_id)?;
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": if removed {
                        format!("Account '{}' removed.", account_id)
                    } else {
                        format!("Account '{}' not found.", account_id)
                    }
                }]
            }))
        }

        "list_messages" => {
            let account_id = args["account_id"].as_str().unwrap_or("");
            let folder = args["folder"].as_str().unwrap_or("inbox");
            let max = args["max_results"].as_u64().unwrap_or(20).min(50) as usize;

            let account = state
                .get_account(account_id)?
                .ok_or_else(|| anyhow::anyhow!("account '{}' not found", account_id))?;

            let result = match account.provider {
                Provider::Gmail => {
                    gmail::GmailClient::new(state, account_id)
                        .await?
                        .list_messages(folder, max)
                        .await?
                }
                Provider::Outlook => {
                    outlook::OutlookClient::new(state, account_id)
                        .await?
                        .list_messages(folder, max)
                        .await?
                }
                Provider::ICloud => anyhow::bail!("iCloud IMAP not yet implemented in v0.1"),
            };

            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result)? }]
            }))
        }

        "get_message" => {
            let account_id = args["account_id"].as_str().unwrap_or("");
            let message_id = args["message_id"].as_str().unwrap_or("");

            let account = state
                .get_account(account_id)?
                .ok_or_else(|| anyhow::anyhow!("account '{}' not found", account_id))?;

            let result = match account.provider {
                Provider::Gmail => {
                    gmail::GmailClient::new(state, account_id)
                        .await?
                        .get_message(message_id)
                        .await?
                }
                Provider::Outlook => {
                    outlook::OutlookClient::new(state, account_id)
                        .await?
                        .get_message(message_id)
                        .await?
                }
                Provider::ICloud => anyhow::bail!("iCloud IMAP not yet implemented in v0.1"),
            };

            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result)? }]
            }))
        }

        "send_message" => {
            let account_id = args["account_id"].as_str().unwrap_or("");
            let to = args["to"].as_str().unwrap_or("");
            let subject = args["subject"].as_str().unwrap_or("");
            let body = args["body"].as_str().unwrap_or("");
            let cc = args["cc"].as_str();

            let account = state
                .get_account(account_id)?
                .ok_or_else(|| anyhow::anyhow!("account '{}' not found", account_id))?;

            let msg_id = match account.provider {
                Provider::Gmail => {
                    gmail::GmailClient::new(state, account_id)
                        .await?
                        .send_message(to, subject, body, cc)
                        .await?
                }
                Provider::Outlook => {
                    outlook::OutlookClient::new(state, account_id)
                        .await?
                        .send_message(to, subject, body, cc)
                        .await?
                }
                Provider::ICloud => anyhow::bail!("iCloud SMTP not yet implemented in v0.1"),
            };

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Message sent successfully. ID: {}", msg_id)
                }]
            }))
        }

        "search_messages" => {
            let account_id = args["account_id"].as_str().unwrap_or("");
            let query = args["query"].as_str().unwrap_or("");
            let max = args["max_results"].as_u64().unwrap_or(20).min(50) as usize;

            let account = state
                .get_account(account_id)?
                .ok_or_else(|| anyhow::anyhow!("account '{}' not found", account_id))?;

            let result = match account.provider {
                Provider::Gmail => {
                    gmail::GmailClient::new(state, account_id)
                        .await?
                        .search(query, max)
                        .await?
                }
                Provider::Outlook => {
                    outlook::OutlookClient::new(state, account_id)
                        .await?
                        .search(query, max)
                        .await?
                }
                Provider::ICloud => anyhow::bail!("iCloud search not yet implemented in v0.1"),
            };

            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result)? }]
            }))
        }

        _ => anyhow::bail!("unknown tool: {name}"),
    }
}
