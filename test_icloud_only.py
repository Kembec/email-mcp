#!/usr/bin/env python3
"""Test iCloud only (non-interactive)."""
import json
import os
import subprocess

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

print("=== Add iCloud account ===")
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
print(json.dumps(resp, indent=2, ensure_ascii=False)[:4000])

print("\n=== iCloud: search 'hello' ===")
resp = send(proc, {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                   "params": {"name": "search_messages", "arguments": {
                       "account_id": "icloud", "query": "hello", "max_results": 3}}})
print(json.dumps(resp, indent=2, ensure_ascii=False)[:2000])

proc.stdin.close()
proc.wait()
print("\nDone.")
