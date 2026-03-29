<script lang="ts">
  import Sidebar from './components/Sidebar.svelte';
  import Dashboard from './pages/Dashboard.svelte';
  import Chat from './pages/Chat.svelte';
  import Config from './pages/Config.svelte';
  import Skills from './pages/Skills.svelte';

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
    <Dashboard />
  {:else if currentPage === '/chat'}
    <Chat />
  {:else if currentPage === '/config'}
    <Config />
  {:else if currentPage === '/skills'}
    <Skills />
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
