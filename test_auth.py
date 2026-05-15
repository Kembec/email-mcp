#!/usr/bin/env python3
"""Interactive auth tester for email-mcp. Keeps process alive during OAuth flow."""
import json
import os
import subprocess
import sys

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
    stderr=subprocess.DEVNULL,
    text=True,
    env=env,
)

send(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                       "clientInfo": {"name": "test", "version": "1.0"}}})

client_id = env.get("GMAIL_OAUTH_CLIENT_ID", "")
client_secret = env.get("GMAIL_OAUTH_CLIENT_SECRET", "")

# --- auth guinea ---
print("\n=== auth guinea (manuel.benancio@guinea.pe) ===")
resp = send(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                   "params": {"name": "auth_start", "arguments": {
                       "provider": "gmail",
                       "account_id": "guinea",
                       "client_id": client_id,
                       "client_secret": client_secret,
                   }}})
print(resp["result"]["content"][0]["text"])

input("\n>>> Abre la URL, acepta permisos, espera 'Authentication successful!' y presiona Enter: ")

# Check account
resp = send(proc, {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "list_accounts", "arguments": {}}})
print("\n=== accounts ===")
print(resp["result"]["content"][0]["text"])

# List inbox
print("\n=== inbox guinea (últimos 5) ===")
resp = send(proc, {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                   "params": {"name": "list_messages", "arguments": {
                       "account_id": "guinea", "max_results": 5}}})
data = resp["result"]
print(data["content"][0]["text"][:3000])

# If worked, also auth anta
if not data.get("isError"):
    print("\n\n=== auth anta (m@grupoanta.org) ===")
    resp = send(proc, {"jsonrpc": "2.0", "id": 5, "method": "tools/call",
                       "params": {"name": "auth_start", "arguments": {
                           "provider": "gmail",
                           "account_id": "anta",
                           "client_id": client_id,
                           "client_secret": client_secret,
                       }}})
    print(resp["result"]["content"][0]["text"])
    input("\n>>> Abre la URL con m@grupoanta.org, acepta y presiona Enter: ")

    print("\n=== inbox anta (últimos 5) ===")
    resp = send(proc, {"jsonrpc": "2.0", "id": 6, "method": "tools/call",
                       "params": {"name": "list_messages", "arguments": {
                           "account_id": "anta", "max_results": 5}}})
    print(resp["result"]["content"][0]["text"][:3000])

proc.stdin.close()
proc.wait()
