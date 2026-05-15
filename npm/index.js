#!/usr/bin/env node
const { execFileSync } = require('child_process');

const PLATFORMS = {
  'darwin-arm64': '@kembec/email-mcp-darwin-arm64',
  'darwin-x64':   '@kembec/email-mcp-darwin-x64',
  'linux-x64':    '@kembec/email-mcp-linux-x64',
  'linux-arm64':  '@kembec/email-mcp-linux-arm64',
  'win32-x64':    '@kembec/email-mcp-win32-x64',
};

const key = `${process.platform}-${process.arch}`;
const pkg = PLATFORMS[key];
if (!pkg) {
  console.error(`email-mcp: unsupported platform ${key}`);
  process.exit(1);
}

const binName = process.platform === 'win32' ? 'email-mcp.exe' : 'email-mcp';

let binPath;
try {
  binPath = require.resolve(`${pkg}/bin/${binName}`);
} catch {
  console.error(`email-mcp: platform package ${pkg} is not installed.`);
  console.error('Reinstall with `npm install @kembec/email-mcp` to pick the right binary.');
  process.exit(1);
}

try {
  execFileSync(binPath, process.argv.slice(2), { stdio: 'inherit' });
} catch (e) {
  process.exit(typeof e.status === 'number' ? e.status : 1);
}
