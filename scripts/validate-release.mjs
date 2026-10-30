import assert from 'node:assert/strict';
import { appendFileSync, readFileSync } from 'node:fs';

const tag = process.argv[2];
assert.match(tag ?? '', /^v\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/, 'Use a version tag such as v0.6.0 or v0.7.0-beta.1');

const version = tag.slice(1);
const json = (path) => JSON.parse(readFileSync(new URL(path, import.meta.url), 'utf8'));
const cargo = readFileSync(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
const cargoLock = readFileSync(new URL('../src-tauri/Cargo.lock', import.meta.url), 'utf8');
const cargoPackage = cargoLock.split('[[package]]').find((entry) => /^name = "token-speed"$/m.test(entry));
const versions = {
  'package.json': json('../package.json').version,
  'package-lock.json': json('../package-lock.json').version,
  'package-lock.json packages[""]': json('../package-lock.json').packages[''].version,
  'src-tauri/tauri.conf.json': json('../src-tauri/tauri.conf.json').version,
  'src-tauri/Cargo.toml': cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1],
  'src-tauri/Cargo.lock': cargoPackage?.match(/^version\s*=\s*"([^"]+)"/m)?.[1],
};

for (const [path, actual] of Object.entries(versions)) {
  assert.equal(actual, version, `${path}: expected ${version} for ${tag}, got ${actual}`);
}

if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\nprerelease=${version.includes('-')}\n`);
}
console.log(`Release ${tag}: all project versions match.`);
