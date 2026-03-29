<script lang="ts">
  import { icons } from '../lib/icons';

  let { tools, totalMs }: {
    tools: Array<{ name: string; status: string; durationMs?: number }>;
    totalMs?: number;
  } = $props();

  let expanded = $state(false);
</script>

{#if tools.length > 0}
  <button class="tool-summary" onclick={() => expanded = !expanded}>
    <span class="icon">{@html expanded ? icons.chevronDown : icons.chevronRight}</span>
    <span class="text">
      {tools.length} tool{tools.length > 1 ? 's' : ''} used
      {#if totalMs}({totalMs}ms){/if}
    </span>
  </button>

  {#if expanded}
    <div class="tool-details">
      {#each tools as tool}
        <div class="tool-item">
          <span class="tool-status">
            {#if tool.status === 'running'}
              <span class="spinning">{@html icons.spinner}</span>
            {:else}
              <span class="done">{@html icons.check}</span>
            {/if}
          </span>
          <span class="tool-name">{tool.name}</span>
          {#if tool.durationMs}
            <span class="tool-duration">{tool.durationMs}ms</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .tool-summary {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    color: var(--text-secondary);
    font-size: 13px;
  }
  .tool-summary:hover { color: var(--text-primary); }
  .icon { display: flex; }
  .tool-details {
    padding: 4px 0 4px 26px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .tool-item {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-secondary);
  }
  .tool-name { font-family: var(--font-mono); }
  .tool-duration { margin-left: auto; }
  .done { color: var(--success); display: flex; }
  .spinning {
    display: flex;
    animation: spin 1s linear infinite;
    color: var(--accent);
  }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
