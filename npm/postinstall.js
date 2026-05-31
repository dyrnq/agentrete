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

function humanSize(bytes) {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1048576) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / 1048576).toFixed(1) + ' MB';
}

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    https.get(url, (res) => {
      if (res.statusCode === 302 || res.statusCode === 301) {
        https.get(res.headers.location, (redirectRes) => {
          handleDownload(redirectRes, file, dest, resolve, reject);
        }).on('error', reject);
        return;
      }
      if (res.statusCode !== 200) {
        reject(new Error(`Download failed: ${res.statusCode}`));
        return;
      }
      handleDownload(res, file, dest, resolve, reject);
    }).on('error', reject);
  });
}

function handleDownload(res, file, dest, resolve, reject) {
  const total = parseInt(res.headers['content-length'] || '0', 10);
  let downloaded = 0;
  let lastLog = Date.now();

  res.on('data', (chunk) => {
    downloaded += chunk.length;
    const now = Date.now();
    if (total > 0 && now - lastLog > 500) {
      const pct = ((downloaded / total) * 100).toFixed(0);
      process.stdout.write(`\r  ${pct}%  ${humanSize(downloaded)} / ${humanSize(total)}`);
      lastLog = now;
    }
  });

  res.pipe(file);
  file.on('finish', () => {
    process.stdout.write('\n');
    file.close();
    fs.chmodSync(dest, 0o755);
    resolve();
  });
  res.on('error', reject);
}

async function main() {
  const target = platformAsset();
  if (!target) {
    console.warn(`Unsupported platform: ${process.platform}-${process.arch}`);
    return;
  }

  const binDir = path.join(__dirname, 'bin');
  fs.mkdirSync(binDir, { recursive: true });

  const suffix = process.platform === 'win32' ? '.exe' : '';
  const dest = path.join(binDir, BINARY_NAME + '-' + target + suffix);
  if (fs.existsSync(dest) && fs.statSync(dest).size > 0) {
    console.log(`agentrete ${PKG_VERSION} already installed (${humanSize(fs.statSync(dest).size)})`);
    return;
  }

  const tag = `v${PKG_VERSION}`;
  const url = `https://github.com/dyrnq/agentrete/releases/download/${tag}/agentrete-${tag}-${target}${suffix}`;

  console.log(`Downloading agentrete ${PKG_VERSION} (${target})...`);
  console.log(`  ${url}`);
  try {
    await download(url, dest);
    console.log(`\nInstalled ${humanSize(fs.statSync(dest).size)} to ${dest}`);
  } catch (e) {
    console.warn(`\nFailed to download binary: ${e.message}`);
    console.warn('You can install manually with: cargo install agentrete');
  }
}

main();
