import fs from 'node:fs/promises';
import path from 'node:path';

const eventName = process.argv[2] || 'UnknownEvent';
const outDir = process.argv[3] || path.resolve('.research/claude-hook-probe/out');
const stdin = await new Promise((resolve, reject) => {
  let data = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', chunk => { data += chunk; });
  process.stdin.on('end', () => resolve(data));
  process.stdin.on('error', reject);
});
await fs.mkdir(outDir, { recursive: true });
const ts = new Date().toISOString().replace(/[:.]/g, '-');
const filename = path.join(outDir, `${ts}-${eventName}.json`);
await fs.writeFile(filename, stdin || '{}');
if (eventName === 'SessionStart') {
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: eventName,
      additionalContext: 'probe-session-start'
    }
  }));
}
