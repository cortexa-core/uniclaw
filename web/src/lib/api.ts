const BASE = '';

export async function fetchStatus() {
  const res = await fetch(`${BASE}/api/status`);
  if (!res.ok) throw new Error(`Status ${res.status}`);
  return res.json();
}

export async function fetchConfig() {
  const res = await fetch(`${BASE}/api/config`);
  if (!res.ok) throw new Error(`Status ${res.status}`);
  return res.json();
}

export async function saveConfig(config: any) {
  const res = await fetch(`${BASE}/api/config`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
  return res.json();
}

export async function fetchSkills() {
  const res = await fetch(`${BASE}/api/skills`);
  if (!res.ok) throw new Error(`Status ${res.status}`);
  return res.json();
}

export async function sendChat(message: string, sessionId: string) {
  const res = await fetch(`${BASE}/api/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ message, session_id: sessionId }),
  });
  return res.json();
}
