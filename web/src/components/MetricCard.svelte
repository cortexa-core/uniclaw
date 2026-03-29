<script lang="ts">
  let { label, value, detail, status }: {
    label: string;
    value: string;
    detail?: string;
    status?: 'ok' | 'warning' | 'error';
  } = $props();
</script>

<div class="card">
  <div class="card-header">
    {#if status}
      <span class="dot dot-{status}"></span>
    {/if}
    <span class="label">{label}</span>
  </div>
  <div class="value">{value}</div>
  {#if detail}
    <div class="detail">{detail}</div>
  {/if}
</div>

<style>
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    transition: background var(--transition);
  }
  .card:hover { background: var(--surface-hover); }
  .card-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .label {
    font-size: 13px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .value {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .detail {
    font-size: 13px;
    color: var(--text-secondary);
    margin-top: 4px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .dot-ok { background: var(--success); }
  .dot-warning { background: var(--accent); }
  .dot-error { background: var(--error); }
</style>
