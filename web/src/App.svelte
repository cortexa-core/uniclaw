<script lang="ts">
  import Sidebar from './components/Sidebar.svelte';

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

<Sidebar {currentPage} {navigate} />

<main class="main">
  {#if currentPage === '/'}
    <h1>Status</h1>
    <p style="color: var(--text-secondary)">Dashboard coming next...</p>
  {:else if currentPage === '/chat'}
    <h1>Chat</h1>
    <p style="color: var(--text-secondary)">Chat page coming soon...</p>
  {:else if currentPage === '/config'}
    <h1>Config</h1>
    <p style="color: var(--text-secondary)">Config page coming soon...</p>
  {:else if currentPage === '/skills'}
    <h1>Skills</h1>
    <p style="color: var(--text-secondary)">Skills page coming soon...</p>
  {:else}
    <h1>Status</h1>
  {/if}
</main>

<style>
  .main {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    max-width: 960px;
  }
  h1 {
    font-size: 20px;
    font-weight: 600;
    margin-bottom: 16px;
  }
  @media (max-width: 768px) {
    .main {
      padding: 16px;
      padding-bottom: 72px;
    }
  }
</style>
