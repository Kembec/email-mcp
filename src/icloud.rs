use anyhow::Result;
use async_imap::Session;
use futures::TryStreamExt;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use webpki_roots::TLS_SERVER_ROOTS;

const IMAP_HOST: &str = "imap.mail.me.com";
const IMAP_PORT: u16 = 993;

pub struct ICloudClient {
    email: String,
    password: String,
}

impl ICloudClient {
    pub fn new(email: &str, password: &str) -> Self {
        Self {
            email: email.to_string(),
            password: password.to_string(),
        }
    }

    async fn connect(&self) -> Result<Session<tokio_rustls::client::TlsStream<TcpStream>>> {
        let tcp = TcpStream::connect((IMAP_HOST, IMAP_PORT)).await?;

        let mut roots = RootCertStore::empty();
        roots.extend(TLS_SERVER_ROOTS.iter().cloned());
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from(IMAP_HOST)?;
        let tls = connector.connect(server_name, tcp).await?;

        let client = async_imap::Client::new(tls);
        let session = client
            .login(&self.email, &self.password)
            .await
            .map_err(|(e, _)| anyhow::anyhow!("IMAP login failed: {e}"))?;

        Ok(session)
    }

    pub async fn list_messages(&self, folder: &str, max: usize) -> Result<Value> {
        let mailbox = imap_folder(folder);
        let mut session = self.connect().await?;
        session.select(mailbox).await?;

        // Fetch the last `max` messages by sequence
        let count = session.select(mailbox).await?.exists;
        let start = count.saturating_sub(max as u32) + 1;
        let range = format!("{}:{}", start, count);

        if start > count {
            let _ = session.logout().await;
            return Ok(json!({ "messages": [] }));
        }

        let fetch_spec = "(UID FLAGS BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID)])";
        let messages_stream = session.fetch(&range, fetch_spec).await?;
        let messages: Vec<_> = messages_stream.try_collect().await?;

        let result: Vec<Value> = messages
            .iter()
            .rev()
            .map(|msg| {
                let headers = msg.header().map(parse_headers).unwrap_or_default();
                let unread = !msg
                    .flags()
                    .any(|f| matches!(f, async_imap::types::Flag::Seen));
                json!({
                    "id": msg.uid.unwrap_or(0),
                    "subject": headers.get("subject").cloned().unwrap_or_default(),
                    "from": headers.get("from").cloned().unwrap_or_default(),
                    "date": headers.get("date").cloned().unwrap_or_default(),
                    "unread": unread,
                })
            })
            .collect();

        let _ = session.logout().await;
        Ok(json!({ "messages": result }))
    }

    pub async fn get_message(&self, uid: u32) -> Result<Value> {
        let mut session = self.connect().await?;
        session.select("INBOX").await?;

        let messages_stream = session
            .uid_fetch(uid.to_string(), "BODY.PEEK[]")
            .await?;
        let messages: Vec<_> = messages_stream.try_collect().await?;
        let _ = session.logout().await;

        let msg = messages
            .first()
            .ok_or_else(|| anyhow::anyhow!("message {} not found", uid))?;

        let raw = msg.body().unwrap_or(b"");
        let (headers_raw, body_raw) = split_headers_body(raw);
        let headers = parse_headers(headers_raw);

        Ok(json!({
            "id": uid,
            "subject": headers.get("subject").cloned().unwrap_or_default(),
            "from": headers.get("from").cloned().unwrap_or_default(),
            "to": headers.get("to").cloned().unwrap_or_default(),
            "date": headers.get("date").cloned().unwrap_or_default(),
            "body": String::from_utf8_lossy(body_raw).to_string(),
        }))
    }

    pub async fn search(&self, query: &str, max: usize) -> Result<Value> {
        let mut session = self.connect().await?;
        session.select("INBOX").await?;

        // IMAP SEARCH TEXT searches subject, body, and headers
        let imap_query = format!("TEXT {:?}", query);
        let uids = session.search(&imap_query).await?;

        let mut uid_list: Vec<u32> = uids.into_iter().collect();
        uid_list.sort_unstable_by(|a, b| b.cmp(a));
        uid_list.truncate(max);
        if uid_list.is_empty() {
            let _ = session.logout().await;
            return Ok(json!({ "messages": [] }));
        }

        let range = uid_list
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let fetch_spec = "(UID FLAGS BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE)])";
        let messages_stream = session.uid_fetch(&range, fetch_spec).await?;
        let messages: Vec<_> = messages_stream.try_collect().await?;
        let _ = session.logout().await;

        let result: Vec<Value> = messages
            .iter()
            .map(|msg| {
                let headers = msg.header().map(parse_headers).unwrap_or_default();
                json!({
                    "id": msg.uid.unwrap_or(0),
                    "subject": headers.get("subject").cloned().unwrap_or_default(),
                    "from": headers.get("from").cloned().unwrap_or_default(),
                    "date": headers.get("date").cloned().unwrap_or_default(),
                })
            })
            .collect();

        Ok(json!({ "messages": result }))
    }

    pub async fn send_message(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        cc: Option<&str>,
    ) -> Result<()> {
        use lettre::message::header::ContentType;
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

        let mut email_builder = Message::builder()
            .from(self.email.parse()?)
            .to(to.parse()?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN);

        if let Some(cc_addr) = cc {
            email_builder = email_builder.cc(cc_addr.parse()?);
        }

        let email = email_builder.body(body.to_string())?;

        let creds = Credentials::new(self.email.clone(), self.password.clone());
        let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay("smtp.mail.me.com")?
            .credentials(creds)
            .build();

        mailer.send(email).await?;
        Ok(())
    }
}

fn imap_folder(folder: &str) -> &str {
    match folder {
        "inbox" | "" => "INBOX",
        "sent" => "Sent Messages",
        "drafts" => "Drafts",
        "trash" => "Deleted Messages",
        "spam" => "Junk",
        other => other,
    }
}

fn parse_headers(raw: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(raw);
    let mut map: HashMap<String, String> = HashMap::new();
    let mut key = String::new();
    let mut val = String::new();

    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            val.push(' ');
            val.push_str(line.trim());
        } else {
            if !key.is_empty() {
                map.insert(key.to_lowercase(), val.trim().to_string());
            }
            if let Some(pos) = line.find(':') {
                key = line[..pos].trim().to_string();
                val = line[pos + 1..].trim().to_string();
            } else {
                key.clear();
                val.clear();
            }
        }
    }
    if !key.is_empty() {
        map.insert(key.to_lowercase(), val.trim().to_string());
    }
    map
}

fn split_headers_body(raw: &[u8]) -> (&[u8], &[u8]) {
    // Headers end at \r\n\r\n or \n\n
    if let Some(pos) = find_body_start(raw) {
        (&raw[..pos], &raw[pos..])
    } else {
        (raw, b"")
    }
}

fn find_body_start(raw: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < raw.len() {
        if raw[i] == b'\r' && raw[i + 1] == b'\n' {
            if i + 3 < raw.len() && raw[i + 2] == b'\r' && raw[i + 3] == b'\n' {
                return Some(i + 4);
            }
            i += 2;
        } else if raw[i] == b'\n' {
            if i + 1 < raw.len() && raw[i + 1] == b'\n' {
                return Some(i + 2);
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    None
}
