#!/usr/bin/env node
// postinstall: download platform-specific binary from GitHub release
const fs = require('fs');
const path = require('path');
const https = require('https');

const PKG_VERSION = process.env.npm_package_version || '0.1.0';
const BINARY_NAME = process.platform === 'win32' ? 'agentrete.exe' : 'agentrete';

function platformAsset() {
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

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    https.get(url, (res) => {
      if (res.statusCode !== 200) {
        reject(new Error(`Download failed: ${res.statusCode}`));
        return;
      }
      res.pipe(file);
      file.on('finish', () => { file.close(); fs.chmodSync(dest, 0o755); resolve(); });
    }).on('error', reject);
  });
}

async function main() {
  const target = platformAsset();
  if (!target) {
    console.warn(`Unsupported platform: ${process.platform}-${process.arch}`);
    return;
  }

  const binDir = path.join(__dirname, 'bin');
  fs.mkdirSync(binDir, { recursive: true });

  const dest = path.join(binDir, BINARY_NAME);
  if (fs.existsSync(dest)) {
    console.log(`agentrete binary already exists at ${dest}`);
    return;
  }

  const tag = `v${PKG_VERSION}`;
  const url = `https://github.com/dyrnq/agentrete/releases/download/${tag}/agentrete-${tag}-${target}`;
  
  console.log(`Downloading agentrete ${PKG_VERSION} for ${target}...`);
  try {
    await download(url, dest);
    console.log(`Installed to ${dest}`);
  } catch (e) {
    console.warn(`Failed to download binary: ${e.message}`);
    console.warn('You can install manually with: cargo install agentrete');
  }
}

main();
