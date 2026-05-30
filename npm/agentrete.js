#!/usr/bin/env node
// agentrete npm wrapper — locates and runs the native binary
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

function binaryName() {
  return process.platform === 'win32' ? 'agentrete.exe' : 'agentrete';
}

function binaryPath() {
  const bin = binaryName();
  
  // 1. Check npm-installed binary (optional dependencies)
  const pkg = platformPackage();
  if (pkg) {
    try {
      const dir = path.dirname(require.resolve(pkg + '/package.json'));
      const candidate = path.join(dir, bin);
      if (fs.existsSync(candidate)) return candidate;
    } catch (_) {}
  }
  
  // 2. Check local build
  const local = path.join(__dirname, '..', 'target', 'release', bin);
  if (fs.existsSync(local)) return local;
  const debug = path.join(__dirname, '..', 'target', 'debug', bin);
  if (fs.existsSync(debug)) return debug;
  
  return null;
}

function platformPackage() {
  const { platform, arch } = process;
  const map = {
    'darwin-arm64': '@dyrnq/agentrete-darwin-arm64',
    'darwin-x64': '@dyrnq/agentrete-darwin-x64',
    'linux-x64': '@dyrnq/agentrete-linux-x64',
    'linux-arm64': '@dyrnq/agentrete-linux-arm64',
    'win32-x64': '@dyrnq/agentrete-win32-x64',
  };
  return map[`${platform}-${arch}`] || null;
}

function main() {
  const bin = binaryPath();
  if (!bin) {
    console.error('agentrete binary not found. Install with: cargo install agentrete');
    process.exit(1);
  }
  
  const result = spawnSync(bin, process.argv.slice(2), {
    stdio: 'inherit',
    cwd: process.cwd(),
  });
  
  process.exit(result.status ?? 1);
}

main();
