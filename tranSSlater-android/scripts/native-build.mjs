import { spawn } from 'node:child_process';
import { createWriteStream, mkdirSync } from 'node:fs';
mkdirSync('test-results', { recursive: true });
const log = createWriteStream('test-results/tauri-build.log');
const child = spawn(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', 'android', 'build', '--apk', ...process.argv.slice(2)], { stdio: ['inherit', 'pipe', 'pipe'] });
for (const stream of [child.stdout, child.stderr]) stream.on('data', data => { process.stdout.write(data); log.write(data); });
child.on('error', error => { console.error(error); log.end(String(error)); process.exitCode = 1; });
child.on('close', code => { log.end(); process.exitCode = code ?? 1; });
