<script lang="ts">
  import { fetchStatus } from '../lib/api';
  import MetricCard from '../components/MetricCard.svelte';

  let status = $state<any>(null);
  let error = $state('');

  async function refresh() {
    try {
      status = await fetchStatus();
      error = '';
    } catch (e) {
      error = 'Failed to connect to agent';
    }
  }

  $effect(() => {
    refresh();
    const interval = setInterval(refresh, 5000);
    return () => clearInterval(interval);
  });

  let uptime = $derived(status ? formatUptime(status.uptime_secs) : '--');

  function formatUptime(secs: number): string {
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    return h > 0 ? `${h}h ${m}m` : `${m}m`;
  }
</script>

<div class="page">
  <h1 class="page-title">Status</h1>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div class="metrics-grid">
    <MetricCard
      label="Agent"
      value={status ? 'Online' : 'Connecting...'}
      detail={uptime}
      status={status ? 'ok' : 'warning'}
    />
    <MetricCard
      label="Model"
      value={status?.model || '--'}
    />
    <MetricCard
      label="Version"
      value={status?.version || '--'}
    />
  </div>
</div>

<style>
  .page { max-width: 800px; }
  .page-title {
    font-size: 20px;
    font-weight: 600;
    margin-bottom: 24px;
    color: var(--text-primary);
  }
  .metrics-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 12px;
    margin-bottom: 24px;
  }
  .error-banner {
    background: color-mix(in srgb, var(--error) 15%, transparent);
    border: 1px solid var(--error);
    color: var(--error);
    padding: 12px 16px;
    border-radius: var(--radius);
    margin-bottom: 16px;
    font-size: 14px;
  }
</style>
