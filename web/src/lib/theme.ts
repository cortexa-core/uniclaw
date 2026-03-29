export function getTheme(): 'dark' | 'light' {
  return (document.documentElement.getAttribute('data-theme') as 'dark' | 'light') || 'dark';
}

export function toggleTheme(): void {
  const current = getTheme();
  const next = current === 'dark' ? 'light' : 'dark';
  document.documentElement.setAttribute('data-theme', next);
  localStorage.setItem('uniclaw-theme', next);
}

// Apply saved theme on load
const saved = localStorage.getItem('uniclaw-theme') as 'dark' | 'light' | null;
if (saved) {
  document.documentElement.setAttribute('data-theme', saved);
}
