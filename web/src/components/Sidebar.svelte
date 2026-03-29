<script lang="ts">
  import { icons } from '../lib/icons';
  import { getTheme, toggleTheme } from '../lib/theme';

  let { currentPage, navigate }: { currentPage: string; navigate: (path: string) => void } = $props();

  const navItems = [
    { path: '/', label: 'Status', icon: icons.dashboard },
    { path: '/chat', label: 'Chat', icon: icons.chat },
    { path: '/config', label: 'Config', icon: icons.config },
    { path: '/skills', label: 'Skills', icon: icons.skills },
  ];

  let theme = $state(getTheme());

  function onToggleTheme() {
    toggleTheme();
    theme = getTheme();
  }
</script>

<!-- Desktop sidebar -->
<nav class="sidebar desktop">
  <div class="logo">UC</div>
  {#each navItems as item}
    <button
      class="nav-item"
      class:active={currentPage === item.path}
      onclick={() => navigate(item.path)}
      title={item.label}
    >
      <span class="icon">{@html item.icon}</span>
      <span class="label">{item.label}</span>
    </button>
  {/each}
  <div class="spacer"></div>
  <button class="nav-item" onclick={onToggleTheme} title="Toggle theme">
    <span class="icon">{@html theme === 'dark' ? icons.sun : icons.moon}</span>
    <span class="label">Theme</span>
  </button>
</nav>

<!-- Mobile bottom tabs -->
<nav class="tabs mobile">
  {#each navItems as item}
    <button
      class="tab"
      class:active={currentPage === item.path}
      onclick={() => navigate(item.path)}
    >
      <span class="icon">{@html item.icon}</span>
      <span class="label">{item.label}</span>
    </button>
  {/each}
</nav>

<style>
  .sidebar {
    width: 48px;
    background: var(--surface);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 12px 0;
    gap: 4px;
    overflow: hidden;
    transition: width var(--transition);
    flex-shrink: 0;
  }
  .sidebar:hover { width: 180px; }
  .logo {
    font-size: 16px;
    font-weight: 700;
    color: var(--accent);
    padding: 8px 0 16px;
    white-space: nowrap;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 10px 14px;
    color: var(--text-secondary);
    white-space: nowrap;
    border-left: 3px solid transparent;
  }
  .nav-item:hover {
    color: var(--text-primary);
    background: var(--surface-hover);
  }
  .nav-item.active {
    color: var(--accent);
    border-left-color: var(--accent);
  }
  .icon { flex-shrink: 0; display: flex; }
  .label { opacity: 0; transition: opacity var(--transition); }
  .sidebar:hover .label { opacity: 1; }
  .spacer { flex: 1; }
  .tabs {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    background: var(--surface);
    border-top: 1px solid var(--border);
    display: flex;
    justify-content: space-around;
    padding: 8px 0;
    padding-bottom: max(8px, env(safe-area-inset-bottom));
    z-index: 100;
  }
  .tab {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 4px 12px;
    color: var(--text-secondary);
    font-size: 11px;
  }
  .tab.active { color: var(--accent); }
  .desktop { display: flex; }
  .mobile { display: none; }
  @media (max-width: 768px) {
    .desktop { display: none; }
    .mobile { display: flex; }
  }
</style>
