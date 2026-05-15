# email-mcp

Multi-account email MCP for Gmail, Outlook, and iCloud. Runs as a stdio MCP server.

## Install

```bash
npm install -g @kembec/email-mcp
```

## Setup

### Gmail

1. Create a project in [Google Cloud Console](https://console.cloud.google.com)
2. Enable the **Gmail API**
3. Create OAuth2 credentials (Desktop app) — copy Client ID and Client Secret
4. Add `http://127.0.0.1` to Authorized redirect URIs (any port)

```json
{
  "name": "auth_start",
  "arguments": {
    "provider": "gmail",
    "account_id": "work",
    "client_id": "YOUR_CLIENT_ID",
    "client_secret": "YOUR_CLIENT_SECRET"
  }
}
```

Open the URL returned, accept, done.

### Outlook

1. Register an app in [Azure AD](https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps/ApplicationsListBlade)
2. Add redirect URI: `http://localhost` (Mobile and desktop applications)
3. Copy the Application (client) ID

```json
{
  "name": "auth_start",
  "arguments": {
    "provider": "outlook",
    "account_id": "work",
    "client_id": "YOUR_APP_ID"
  }
}
```

### iCloud

1. Go to [appleid.apple.com](https://appleid.apple.com) → Sign-In & Security → App-Specific Passwords
2. Generate a password named `email-mcp`

```json
{
  "name": "auth_add_icloud",
  "arguments": {
    "account_id": "personal",
    "email": "you@icloud.com",
    "app_password": "xxxx-xxxx-xxxx-xxxx"
  }
}
```

## Tools

| Tool | Description |
|------|-------------|
| `list_accounts` | List configured accounts |
| `auth_start` | Start OAuth2 for Gmail or Outlook |
| `auth_add_icloud` | Add iCloud account |
| `auth_remove` | Remove an account |
| `list_messages` | List recent emails |
| `get_message` | Get full email content |
| `send_message` | Send an email |
| `search_messages` | Search emails |

## Multi-account

Configure as many accounts as needed — each with a unique `account_id`. All tools accept `account_id` to select which account to use.

## License

MIT
