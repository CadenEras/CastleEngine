#!/usr/bin/env node
// Measures the idle memory floor of the release desktop app: shell, worker
// and WebView processes (the whole process tree), over several cold starts.
// Usage: node scripts/measure-memory.mjs [runs=10] [settleSeconds=5]
// Requires `npm --prefix apps/desktop run tauri -- build --no-bundle` first.
// Reports medians and ranges; it does not judge them against a budget.
import { execFileSync, spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const exe = path.join(root, 'target', 'release',
  `castle-engine-desktop${process.platform === 'win32' ? '.exe' : ''}`);
const runs = Number.parseInt(process.argv[2] ?? '10', 10);
const settleSeconds = Number.parseInt(process.argv[3] ?? '5', 10);
const MiB = 1024 * 1024;

if (!existsSync(exe)) {
  console.error(`missing ${exe}: build the release app first`);
  process.exit(1);
}
if (!(runs > 0 && settleSeconds > 0)) {
  console.error('runs and settleSeconds must be positive integers');
  process.exit(1);
}

// Returns [{ pid, ppid, name, rss, priv }] in bytes; priv is null when unknown.
function processTable() {
  if (process.platform === 'win32') {
    const script = 'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,'
      + 'Name,WorkingSetSize,PrivatePageCount | ConvertTo-Json -Compress';
    const out = execFileSync('powershell.exe', ['-NoProfile', '-Command', script],
      { encoding: 'utf8', maxBuffer: 64 * MiB });
    return JSON.parse(out).map(p => ({
      pid: p.ProcessId, ppid: p.ParentProcessId, name: p.Name,
      rss: Number(p.WorkingSetSize), priv: Number(p.PrivatePageCount),
    }));
  }
  const out = execFileSync('ps', ['-A', '-o', 'pid=,ppid=,rss=,comm='], { encoding: 'utf8' });
  return out.trim().split('\n').map(line => {
    const [pid, ppid, rss, ...name] = line.trim().split(/\s+/);
    return { pid: Number(pid), ppid: Number(ppid), name: path.basename(name.join(' ')),
      rss: Number(rss) * 1024, priv: null };
  });
}

// Descendants of the shell only. WebView2 (Windows) and WebKitGTK (Linux)
// processes are descendants; macOS WebKit XPC services are not and are
// therefore NOT counted there. Matching them by name would include other apps.
function tree(table, rootPid) {
  const ids = new Set([rootPid]);
  for (let grew = true; grew;) {
    grew = false;
    for (const p of table) {
      if (!ids.has(p.pid) && ids.has(p.ppid)) { ids.add(p.pid); grew = true; }
    }
  }
  return table.filter(p => ids.has(p.pid));
}

function stop(child) {
  if (process.platform === 'win32') {
    // Without /F and /T: WM_CLOSE to the main window only, so the app shuts
    // its worker and WebView down itself.
    try { execFileSync('taskkill', ['/PID', String(child.pid)], { stdio: 'ignore' }); }
    catch { /* already gone */ }
  } else {
    child.kill('SIGTERM');
  }
}

const median = values => {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
};
const fmt = bytes => (bytes / MiB).toFixed(1);

const samples = [];
for (let i = 0; i < runs; i++) {
  const child = spawn(exe, [], { stdio: 'ignore' });
  await sleep(settleSeconds * 1000);
  const procs = tree(processTable(), child.pid);
  const sum = (filter, key) => procs.filter(filter).reduce((n, p) => n + (p[key] ?? 0), 0);
  const isShell = p => p.pid === child.pid;
  const isWorker = p => /^pdf-worker/.test(p.name);
  const isWebView = p => !isShell(p) && !isWorker(p);
  samples.push({
    processes: procs.length,
    shell: sum(isShell, 'rss'), worker: sum(isWorker, 'rss'), webview: sum(isWebView, 'rss'),
    total: sum(() => true, 'rss'),
    privateTotal: process.platform === 'win32' ? sum(() => true, 'priv') : null,
    workerFound: procs.some(isWorker),
  });
  const workerPids = new Set(procs.filter(isWorker).map(p => p.pid));
  stop(child);
  const exited = await Promise.race([
    new Promise(resolve => child.once('exit', () => resolve(true))),
    sleep(10_000).then(() => false),
  ]);
  if (!exited) {
    console.error(`run ${i + 1}: app did not exit after close request`);
    process.exit(1);
  }
  await sleep(1000);
  if (processTable().some(p => workerPids.has(p.pid) && /^pdf-worker/.test(p.name))) {
    console.error(`run ${i + 1}: pdf-worker survived the application`);
    process.exit(1);
  }
}

if (samples.some(s => !s.workerFound)) {
  console.error('pdf-worker was not found in at least one run');
  process.exit(1);
}

const rows = [
  ['Shell (working set / RSS)', 'shell'],
  ['Worker (working set / RSS)', 'worker'],
  ['WebView (working set / RSS)', 'webview'],
  ['Total (working set / RSS)', 'total'],
  ...(process.platform === 'win32' ? [['Total private bytes', 'privateTotal']] : []),
];
console.log(`Platform: ${os.type()} ${os.release()} ${os.arch()}, ${os.cpus()[0]?.model ?? 'CPU?'}, `
  + `${fmt(os.totalmem())} MiB RAM, Node ${process.version}`);
console.log(`Runs: ${runs}, settle: ${settleSeconds} s after start, processes per run: `
  + `${median(samples.map(s => s.processes))}`);
console.log('| Measure | Median (MiB) | Min | Max |');
console.log('| --- | --- | --- | --- |');
for (const [label, key] of rows) {
  const values = samples.map(s => s[key]);
  console.log(`| ${label} | ${fmt(median(values))} | ${fmt(Math.min(...values))} | `
    + `${fmt(Math.max(...values))} |`);
}
