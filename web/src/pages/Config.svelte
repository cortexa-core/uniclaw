<script lang="ts">
  import { fetchConfig, saveConfig } from '../lib/api';
  import Toggle from '../components/Toggle.svelte';
  import Toast from '../components/Toast.svelte';

  let config = $state<any>(null);
  let original = $state<string>('');
  let saving = $state(false);
  let toast = $state<{ message: string; type: 'success' | 'error' } | null>(null);

  let dirty = $derived(config && JSON.stringify(config) !== original);

  async function load() {
    try {
      config = await fetchConfig();
      original = JSON.stringify(config);
    } catch (e) {
      toast = { message: 'Failed to load config', type: 'error' };
    }
  }

  async function save() {
    if (!config || saving) return;
    saving = true;
    try {
      const result = await saveConfig(config);
      if (result.error) {
        toast = { message: result.error, type: 'error' };
      } else {
        toast = { message: 'Config saved', type: 'success' };
        original = JSON.stringify(config);
      }
    } catch (e) {
      toast = { message: 'Failed to save config', type: 'error' };
    }
    saving = false;
  }

  $effect(() => { load(); });
</script>

{#if toast}
  <Toast message={toast.message} type={toast.type} />
{/if}

<div class="page">
  <h1 class="page-title">Configuration</h1>

  {#if !config}
    <p class="loading">Loading config...</p>
  {:else}
    <section class="section">
      <h2 class="section-title">LLM Provider</h2>
      <div class="field">
        <label>Provider</label>
        <select bind:value={config.llm.provider}>
          <option value="anthropic">Anthropic</option>
          <option value="openai_compatible">OpenAI Compatible</option>
        </select>
      </div>
      <div class="field">
        <label>Model</label>
        <input type="text" bind:value={config.llm.model} />
      </div>
      <div class="field">
        <label>API Key Env Variable</label>
        <input type="text" bind:value={config.llm.api_key_env} />
      </div>
      <div class="field">
        <label>Base URL</label>
        <input type="text" bind:value={config.llm.base_url} />
      </div>
      <div class="field">
        <label>Max Tokens</label>
        <input type="number" bind:value={config.llm.max_tokens} />
      </div>
      <div class="field">
        <label>Temperature ({config.llm.temperature})</label>
        <input type="range" min="0" max="2" step="0.1" bind:value={config.llm.temperature} />
      </div>
    </section>

    <section class="section">
      <h2 class="section-title">Server</h2>
      {#if config.server}
        <div class="field row">
          <label>HTTP Port</label>
          <input type="number" bind:value={config.server.http_port} style="width:100px" />
        </div>
        <div class="field row">
          <label>MQTT</label>
          <Toggle checked={config.server.mqtt_enabled} onchange={(v) => config.server.mqtt_enabled = v} />
        </div>
      {/if}
      {#if config.cron}
        <div class="field row">
          <label>Cron Scheduler</label>
          <Toggle checked={config.cron.enabled} onchange={(v) => config.cron.enabled = v} />
        </div>
      {/if}
      {#if config.heartbeat}
        <div class="field row">
          <label>Heartbeat</label>
          <Toggle checked={config.heartbeat.enabled} onchange={(v) => config.heartbeat.enabled = v} />
        </div>
      {/if}
    </section>

    <section class="section">
      <h2 class="section-title">Tools</h2>
      <div class="field row">
        <label>Shell Commands</label>
        <Toggle checked={config.tools.shell_enabled} onchange={(v) => config.tools.shell_enabled = v} />
      </div>
      <div class="field row">
        <label>HTTP Fetch</label>
        <Toggle checked={config.tools.http_fetch_enabled} onchange={(v) => config.tools.http_fetch_enabled = v} />
      </div>
    </section>

    <div class="save-bar">
      <button class="save-btn" class:dirty disabled={!dirty || saving} onclick={save}>
        {saving ? 'Saving...' : 'Save & Apply'}
      </button>
      {#if dirty}
        <span class="dirty-label">Unsaved changes</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .page { max-width: 600px; }
  .page-title { font-size: 20px; font-weight: 600; margin-bottom: 24px; }
  .loading { color: var(--text-secondary); }
  .section { margin-bottom: 32px; }
  .section-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 16px;
  }
  .field { margin-bottom: 16px; }
  .field label {
    display: block;
    font-size: 13px;
    color: var(--text-secondary);
    margin-bottom: 6px;
  }
  .field.row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .field.row label { margin-bottom: 0; }
  .save-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px 0;
    border-top: 1px solid var(--border);
    position: sticky;
    bottom: 0;
    background: var(--bg);
  }
  .save-btn {
    padding: 10px 24px;
    border-radius: var(--radius);
    font-weight: 600;
    background: var(--surface);
    color: var(--text-secondary);
    border: 1px solid var(--border);
  }
  .save-btn.dirty {
    background: var(--accent);
    color: var(--bg);
    border-color: var(--accent);
  }
  .save-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .dirty-label { font-size: 13px; color: var(--accent); }
</style>
