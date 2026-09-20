import { createHash } from "node:crypto";
import { access, chmod, copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { execFileSync, spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { run } from "./build-nexus.mjs";

const target = process.argv[2];
if (!['x86_64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin'].includes(target) || process.platform === 'win32') {
  throw new Error('Build the verifier on its native Linux or macOS runner');
}
const root = process.cwd();
const work = path.join(root, 'target', 'native-verifier', target);
const source = path.join(work, 'source');
const build = path.join(work, 'build');
const resources = path.join(work, 'resources');
await mkdir(source, { recursive: true });
await mkdir(resources, { recursive: true });
const archive = path.join(resources, 'osslsigncode-source.tar.gz');
const commit = 'beec94e308d1a1e03ca17b05fe089d93c6303e90';
const expected = '1cd8ad26c9ed34e5b96d0a0bef0309437e5e71227a67560da03c33797371ccbd';
const response = await fetch(`https://codeload.github.com/mtrojnar/osslsigncode/tar.gz/${commit}`);
if (!response.ok) throw new Error(`Verifier source download failed: ${response.status}`);
const bytes = Buffer.from(await response.arrayBuffer());
if (createHash('sha256').update(bytes).digest('hex') !== expected) throw new Error('Verifier source checksum mismatch');
await writeFile(archive, bytes);
await run('tar', ['-xzf', archive, '--strip-components', '1', '-C', source], { cwd: root, env: process.env });
const configure = ['-S', source, '-B', build, '-DCMAKE_BUILD_TYPE=Release', '-DOPENSSL_USE_STATIC_LIBS=ON'];
let opensslLicense;
if (process.platform === 'darwin') {
  const prefix = execFileSync('brew', ['--prefix', 'openssl@3'], { encoding: 'utf8' }).trim();
  configure.push(`-DOPENSSL_ROOT_DIR=${prefix}`, '-DCMAKE_OSX_DEPLOYMENT_TARGET=15.0');
  const candidates = [path.join(prefix, 'LICENSE.txt'), path.join(prefix, 'share/doc/openssl@3/LICENSE.txt')];
  for (const file of candidates) { try { await access(file); opensslLicense = file; break; } catch { /* Check the other installation layout. */ } }
} else {
  opensslLicense = '/usr/share/doc/libssl-dev/copyright';
}
if (!opensslLicense) throw new Error('OpenSSL license file was not found');
await run('cmake', configure, { cwd: root, env: process.env });
await run('cmake', ['--build', build, '--config', 'Release', '--parallel', '2'], { cwd: root, env: process.env });
const binaries = path.join(root, 'src-tauri', 'binaries');
await mkdir(binaries, { recursive: true });
const destination = path.join(binaries, `dlssync-authenticode-${target}`);
await copyFile(path.join(build, 'osslsigncode'), destination);
await chmod(destination, 0o755);
await run(destination, ['--version'], { cwd: root, env: process.env });
// Isolated test CA: never installed into the machine trust store or distributed.
const fixture = path.join(work, 'verification-fixture');
await mkdir(fixture, { recursive: true });
const certificate = path.join(fixture, 'test-ca.pem');
const privateKey = path.join(fixture, 'test-key.pem');
const signed = path.join(fixture, 'signed.exe');
await run('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', privateKey, '-out', certificate, '-days', '1', '-subj', '/CN=DLSSync Verifier Fixture', '-addext', 'extendedKeyUsage=codeSigning'], { cwd: root, env: process.env });
await run(destination, ['sign', '-certs', certificate, '-key', privateKey, '-in', path.join(source, 'tests/files/unsigned.exe'), '-out', signed], { cwd: root, env: process.env });
function verify(args, expectedSuccess) {
  const result = spawnSync(destination, ['verify', '-index', '0', ...args], { encoding: 'utf8', timeout: 30000 });
  if (result.error || result.signal || (result.status === 0) !== expectedSuccess) {
    throw new Error(`Verifier self-test failed: ${result.error ?? result.stderr ?? result.status}`);
  }
}
verify(['-CAfile', certificate, '-in', signed], true);
verify(['-in', signed], false);
const damaged = Buffer.from(await readFile(signed));
const peOffset = damaged.readUInt32LE(0x3c);
const sectionOffset = peOffset + 24 + damaged.readUInt16LE(peOffset + 20);
const rawOffset = damaged.readUInt32LE(sectionOffset + 20);
if (!damaged.readUInt32LE(sectionOffset + 16) || rawOffset >= damaged.length) throw new Error('Verifier fixture has no section payload');
damaged[rawOffset] ^= 1;
const tampered = path.join(fixture, 'tampered.exe');
await writeFile(tampered, damaged);
verify(['-CAfile', certificate, '-in', tampered], false);
console.log('Verifier checks passed: explicit test CA, untrusted CA rejection, tampered bytes rejection');
await copyFile(path.join(source, 'LICENSE.txt'), path.join(resources, 'osslsigncode-LICENSE.txt'));
await copyFile(path.join(source, 'COPYING.txt'), path.join(resources, 'osslsigncode-COPYING.txt'));
await copyFile(opensslLicense, path.join(resources, 'OpenSSL-LICENSE.txt'));
await writeFile(path.join(resources, 'BUILD.txt'), `osslsigncode 2.14\nSource commit: ${commit}\nSource archive SHA-256: ${expected}\nBuilt with CMake Release and OPENSSL_USE_STATIC_LIBS=ON.\nExtract osslsigncode-source.tar.gz, install CMake and OpenSSL 3 development files, then run:\ncmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DOPENSSL_USE_STATIC_LIBS=ON\ncmake --build build --config Release\nThe program is a separate executable, unmodified from upstream.\nFull packaging instructions are in scripts/build-native-verifier.mjs in the corresponding DLSSync source release.\n`);
const resourceMap = {};
for (const name of ['osslsigncode-source.tar.gz', 'osslsigncode-LICENSE.txt', 'osslsigncode-COPYING.txt', 'OpenSSL-LICENSE.txt', 'BUILD.txt']) {
  resourceMap[path.join(resources, name)] = `native-verifier/${name}`;
}
const overlay = { bundle: { externalBin: [path.join(binaries, 'dlssync-authenticode')], resources: resourceMap, macOS: { minimumSystemVersion: '15.0' } } };
await writeFile(path.join(work, 'tauri.conf.json'), JSON.stringify(overlay, null, 2));
console.log(`Native verifier prepared for ${target}`);
