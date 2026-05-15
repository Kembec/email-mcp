use crate::config::{Account, AppState, PendingAuth, Provider};
use anyhow::Result;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn random_string(len: usize) -> String {
    let mut bytes = vec![0u8; len * 2];
    rand::thread_rng().fill_bytes(&mut bytes);
    let encoded = URL_SAFE_NO_PAD.encode(&bytes);
    encoded[..len].to_string()
}

fn pkce_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

pub struct AuthStartResult {
    pub message: String,
}

pub async fn start_oauth(
    state: Arc<AppState>,
    provider: Provider,
    account_id: String,
    client_id: String,
    client_secret: Option<String>,
) -> Result<AuthStartResult> {
    match provider {
        Provider::Outlook => {
            start_device_code(state, account_id, client_id, client_secret).await
        }
        Provider::Gmail => {
            start_pkce(state, account_id, client_id, client_secret).await
        }
        Provider::ICloud => anyhow::bail!("iCloud uses app passwords, not OAuth"),
    }
}

async fn start_pkce(
    state: Arc<AppState>,
    account_id: String,
    client_id: String,
    client_secret: Option<String>,
) -> Result<AuthStartResult> {
    let code_verifier = random_string(64);
    let code_challenge = pkce_challenge(&code_verifier);

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

    let scope = "https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/userinfo.email";

    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(scope),
        code_challenge,
    );

    {
        let mut pending = state.pending_auths.lock().unwrap();
        pending.insert(
            account_id.clone(),
            PendingAuth {
                account_id: account_id.clone(),
                code_verifier,
                redirect_uri,
                provider: Provider::Gmail,
                client_id,
                client_secret,
            },
        );
    }

    let state_clone = state.clone();
    let account_id_clone = account_id.clone();
    tokio::spawn(async move {
        if let Err(e) = handle_oauth_callback(listener, state_clone, account_id_clone).await {
            eprintln!("OAuth callback error: {e}");
        }
    });

    Ok(AuthStartResult {
        message: format!(
            "Open this URL in your browser to authenticate:\n\n{}\n\nAuth will complete automatically when you accept. Use list_accounts to verify.",
            auth_url
        ),
    })
}

// --- Outlook device code flow ---

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
    #[serde(default)]
    message: String,
}

async fn start_device_code(
    state: Arc<AppState>,
    account_id: String,
    client_id: String,
    client_secret: Option<String>,
) -> Result<AuthStartResult> {
    let scope = "https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/Mail.Send https://graph.microsoft.com/User.Read offline_access";

    let client = reqwest::Client::new();
    let resp = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .form(&[("client_id", client_id.as_str()), ("scope", scope)])
        .send()
        .await?;

    if !resp.status().is_success() {
        let body = resp.text().await?;
        anyhow::bail!("Device code request failed: {body}");
    }

    let dc: DeviceCodeResponse = resp.json().await?;

    let user_message = if !dc.message.is_empty() {
        dc.message.clone()
    } else {
        format!(
            "Go to {} and enter the code: {}",
            dc.verification_uri, dc.user_code
        )
    };

    let state_clone = state.clone();
    let account_id_clone = account_id.clone();
    let client_id_clone = client_id.clone();
    let client_secret_clone = client_secret.clone();
    let device_code = dc.device_code.clone();
    let interval = dc.interval;
    let expires_in = dc.expires_in;

    let handle = tokio::spawn(async move {
        if let Err(e) = poll_device_code(
            state_clone,
            account_id_clone,
            client_id_clone,
            client_secret_clone,
            device_code,
            interval,
            expires_in,
        )
        .await
        {
            eprintln!("Device code poll error: {e}");
        }
    });
    state.background_tasks.lock().unwrap().push(handle);

    Ok(AuthStartResult {
        message: format!(
            "{}\n\nAuth will complete automatically once you approve. Use list_accounts to verify.",
            user_message
        ),
    })
}

async fn poll_device_code(
    state: Arc<AppState>,
    account_id: String,
    client_id: String,
    client_secret: Option<String>,
    device_code: String,
    interval: u64,
    expires_in: u64,
) -> Result<()> {
    let token_url = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
    let client = reqwest::Client::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(expires_in);

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval.max(5))).await;

        if std::time::Instant::now() > deadline {
            eprintln!("Device code expired for account '{account_id}'");
            return Ok(());
        }

        let mut params: Vec<(&str, &str)> = vec![
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", &client_id),
            ("device_code", &device_code),
        ];
        if let Some(ref secret) = client_secret {
            params.push(("client_secret", secret));
        }

        let resp = client.post(token_url).form(&params).send().await?;

        if resp.status().is_success() {
            let token: TokenResponse = resp.json().await?;
            let email = get_user_email(&Provider::Outlook, &token.access_token)
                .await
                .ok();
            let expires_at =
                chrono::Utc::now().timestamp() + token.expires_in.unwrap_or(3600) as i64;
            let account = Account {
                id: account_id.clone(),
                provider: Provider::Outlook,
                email,
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
                access_token: Some(token.access_token),
                refresh_token: token.refresh_token,
                expires_at: Some(expires_at),
                icloud_password: None,
            };
            state.upsert_account(account)?;
            eprintln!("Device code auth complete for account '{account_id}'");
            return Ok(());
        }

        let body = resp.text().await?;
        if body.contains("authorization_pending") || body.contains("slow_down") {
            continue;
        }
        if body.contains("expired_token") {
            eprintln!("Device code expired for account '{account_id}'");
            return Ok(());
        }
        anyhow::bail!("Device code poll failed: {body}");
    }
}

// --- PKCE callback handler ---

async fn handle_oauth_callback(
    listener: TcpListener,
    state: Arc<AppState>,
    account_id: String,
) -> Result<()> {
    let (mut stream, _) = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        listener.accept(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("OAuth auth timed out after 5 minutes"))??;

    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let code = request.lines().next().and_then(|line| {
        let path = line.split_whitespace().nth(1)?;
        let query = path.split('?').nth(1)?;
        query.split('&').find_map(|param| {
            let mut parts = param.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next()?;
            (key == "code").then(|| val.to_string())
        })
    });

    let html = if code.is_some() {
        "<html><body><h1>Authentication successful!</h1><p>You can close this tab.</p></body></html>"
    } else {
        "<html><body><h1>Authentication failed.</h1><p>No code received.</p></body></html>"
    };

    let http_resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    stream.write_all(http_resp.as_bytes()).await?;
    drop(stream);

    let code = match code {
        Some(c) => c,
        None => {
            state.pending_auths.lock().unwrap().remove(&account_id);
            return Ok(());
        }
    };

    let pending = state.pending_auths.lock().unwrap().remove(&account_id);
    let pending = match pending {
        Some(p) => p,
        None => return Ok(()),
    };

    let token = exchange_code(
        &pending.provider,
        &pending.client_id,
        pending.client_secret.as_deref(),
        &code,
        &pending.redirect_uri,
        &pending.code_verifier,
    )
    .await?;

    let email = get_user_email(&pending.provider, &token.access_token)
        .await
        .ok();

    let expires_at = chrono::Utc::now().timestamp() + token.expires_in.unwrap_or(3600) as i64;

    let account = Account {
        id: account_id.clone(),
        provider: pending.provider,
        email,
        client_id: pending.client_id,
        client_secret: pending.client_secret,
        access_token: Some(token.access_token),
        refresh_token: token.refresh_token,
        expires_at: Some(expires_at),
        icloud_password: None,
    };

    state.upsert_account(account)?;
    eprintln!("OAuth complete for account '{account_id}'");

    Ok(())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

async fn exchange_code(
    provider: &Provider,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse> {
    let token_url = match provider {
        Provider::Gmail => "https://oauth2.googleapis.com/token",
        Provider::Outlook => "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
        Provider::ICloud => unreachable!(),
    };

    let mut params: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", code_verifier),
    ];

    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }

    let client = reqwest::Client::new();
    let resp = client.post(token_url).form(&params).send().await?;

    if !resp.status().is_success() {
        let body = resp.text().await?;
        anyhow::bail!("Token exchange failed: {body}");
    }

    Ok(resp.json::<TokenResponse>().await?)
}

pub async fn refresh_token(account: &mut Account) -> Result<()> {
    let refresh_tok = account
        .refresh_token
        .clone()
        .ok_or_else(|| anyhow::anyhow!("no refresh token for account '{}'", account.id))?;

    let token_url = match account.provider {
        Provider::Gmail => "https://oauth2.googleapis.com/token",
        Provider::Outlook => "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
        Provider::ICloud => anyhow::bail!("iCloud does not use OAuth tokens"),
    };

    let mut params: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("client_id", account.client_id.as_str()),
        ("refresh_token", refresh_tok.as_str()),
    ];

    if let Some(ref secret) = account.client_secret {
        params.push(("client_secret", secret.as_str()));
    }

    let client = reqwest::Client::new();
    let resp = client.post(token_url).form(&params).send().await?;

    if !resp.status().is_success() {
        let body = resp.text().await?;
        anyhow::bail!("Token refresh failed: {body}");
    }

    let token: TokenResponse = resp.json().await?;
    account.access_token = Some(token.access_token);
    if let Some(rt) = token.refresh_token {
        account.refresh_token = Some(rt);
    }
    account.expires_at =
        Some(chrono::Utc::now().timestamp() + token.expires_in.unwrap_or(3600) as i64);

    Ok(())
}

async fn get_user_email(provider: &Provider, access_token: &str) -> Result<String> {
    let client = reqwest::Client::new();
    match provider {
        Provider::Gmail => {
            let resp: serde_json::Value = client
                .get("https://www.googleapis.com/oauth2/v1/userinfo")
                .bearer_auth(access_token)
                .send()
                .await?
                .json()
                .await?;
            Ok(resp["email"].as_str().unwrap_or("unknown").to_string())
        }
        Provider::Outlook => {
            let resp: serde_json::Value = client
                .get("https://graph.microsoft.com/v1.0/me")
                .bearer_auth(access_token)
                .send()
                .await?
                .json()
                .await?;
            Ok(resp["mail"]
                .as_str()
                .or_else(|| resp["userPrincipalName"].as_str())
                .unwrap_or("unknown")
                .to_string())
        }
        Provider::ICloud => anyhow::bail!("not applicable"),
    }
}
