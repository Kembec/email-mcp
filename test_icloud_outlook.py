#!/usr/bin/env python3
"""Test iCloud and Outlook for email-mcp."""
import json
import os
import subprocess
import time

BIN = "./target/debug/email-mcp"
ENV_FILE = "/Users/guineaserver/Development/openclaw-local/.env.life"

def load_env(path):
    env = os.environ.copy()
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                env[k.strip()] = v.strip().strip('"')
    return env

def send(proc, msg):
    proc.stdin.write(json.dumps(msg) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    return json.loads(line)

env = load_env(ENV_FILE)
proc = subprocess.Popen(
    [BIN],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    env=env,
)

send(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                       "clientInfo": {"name": "test", "version": "1.0"}}})

# --- iCloud ---
print("\n=== Add iCloud account (id@kembec.com) ===")
resp = send(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                   "params": {"name": "auth_add_icloud", "arguments": {
                       "account_id": "icloud",
                       "email": env.get("ICLOUD_USER", ""),
                       "app_password": env.get("ICLOUD_PW", ""),
                   }}})
print(resp["result"]["content"][0]["text"])

print("\n=== iCloud: list inbox (last 5) ===")
resp = send(proc, {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "list_messages", "arguments": {
                       "account_id": "icloud", "max_results": 5}}})
result = resp.get("result", resp)
print(json.dumps(result, indent=2, ensure_ascii=False)[:2000])

# --- Outlook device code ---
print("\n\n=== Outlook device code auth (kembec@outlook.com) ===")
resp = send(proc, {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                   "params": {"name": "auth_start", "arguments": {
                       "provider": "outlook",
                       "account_id": "outlook",
                       "client_id": env.get("OUTLOOK_CLIENT_ID", ""),
                   }}})
print(resp["result"]["content"][0]["text"])

# Wait for the poll task to save the token, then verify
print("\nWaiting for authentication (up to 60s)...")
for i in range(12):
    time.sleep(5)
    resp = send(proc, {"jsonrpc": "2.0", "id": 100 + i, "method": "tools/call",
                       "params": {"name": "list_accounts", "arguments": {}}})
    accounts_text = resp["result"]["content"][0]["text"]
    accounts = json.loads(accounts_text).get("accounts", [])
    outlook = next((a for a in accounts if a["id"] == "outlook"), None)
    if outlook and outlook.get("authenticated"):
        print(f"✓ Outlook authenticated as {outlook.get('email')}")
        break
    print(f"  [{i*5+5}s] still waiting...")
else:
    print("✗ Timed out — Outlook auth did not complete")
    proc.stdin.close()
    proc.wait()
    exit(1)

print("\n=== Outlook: list inbox (last 5) ===")
resp = send(proc, {"jsonrpc": "2.0", "id": 5, "method": "tools/call",
                   "params": {"name": "list_messages", "arguments": {
                       "account_id": "outlook", "max_results": 5}}})
result = resp.get("result", resp)
print(json.dumps(result, indent=2, ensure_ascii=False)[:3000])

proc.stdin.close()
proc.wait()
print("\nAll done.")
