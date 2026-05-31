#!/usr/bin/env node
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

function binaryName() {
  return process.platform === 'win32' ? 'agentrete.exe' : 'agentrete';
}

function platformSuffix() {
  const { platform, arch } = process;
  const map = {
    'darwin-arm64': 'aarch64-apple-darwin',
    'darwin-x64': 'x86_64-apple-darwin',
    'linux-x64': 'x86_64-unknown-linux-gnu',
    'linux-arm64': 'aarch64-unknown-linux-gnu',
    'win32-x64': 'x86_64-pc-windows-msvc',
  };
  return map[`${platform}-${arch}`] || null;
}

function binaryPath() {
  const suffix = platformSuffix();
  if (!suffix) return null;

  // Look in bin/ directory (bundled with package)
  const candidates = [
    path.join(__dirname, 'bin', `${binaryName()}-${suffix}`),
    path.join(__dirname, 'bin', `${binaryName()}-${suffix}${process.platform === 'win32' ? '.exe' : ''}`),
  ];

  for (const c of candidates) {
    if (fs.existsSync(c) && fs.statSync(c).size > 0) return c;
  }

  return null;
}

function main() {
  const bin = binaryPath();
  if (!bin) {
    console.error('agentrete binary not found. Platform:', process.platform, process.arch);
    process.exit(1);
  }

  const result = spawnSync(bin, process.argv.slice(2), {
    stdio: 'inherit',
    cwd: process.cwd(),
  });

  process.exit(result.status ?? 1);
}

main();
