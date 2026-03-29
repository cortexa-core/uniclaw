<script lang="ts">
  let currentPage = $state(window.location.hash.slice(1) || '/');

  $effect(() => {
    const onHash = () => { currentPage = window.location.hash.slice(1) || '/'; };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  });

  function navigate(path: string) {
    window.location.hash = path;
  }
</script>

<main class="main">
  <h1>UniClaw</h1>
  <p>Current page: {currentPage}</p>
  <nav>
    <button onclick={() => navigate('/')}>Status</button>
    <button onclick={() => navigate('/chat')}>Chat</button>
    <button onclick={() => navigate('/config')}>Config</button>
    <button onclick={() => navigate('/skills')}>Skills</button>
  </nav>
</main>

<style>
  .main {
    flex: 1;
    padding: 24px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
  }

  h1 { color: var(--accent); }

  nav {
    display: flex;
    gap: 8px;
  }

  nav button {
    padding: 8px 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
  }

  nav button:hover {
    background: var(--surface-hover);
    border-color: var(--accent);
  }
</style>
