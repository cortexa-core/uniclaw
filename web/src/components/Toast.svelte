<script lang="ts">
  let { message, type = 'info' }: { message: string; type?: 'info' | 'error' | 'success' } = $props();
  let visible = $state(true);

  $effect(() => {
    const timer = setTimeout(() => { visible = false; }, 3000);
    return () => clearTimeout(timer);
  });
</script>

{#if visible}
  <div class="toast toast-{type}">
    {message}
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    top: 16px;
    right: 16px;
    padding: 12px 20px;
    border-radius: var(--radius);
    font-size: 14px;
    z-index: 1000;
    animation: slideIn 200ms ease-out;
  }
  .toast-success {
    background: color-mix(in srgb, var(--success) 15%, var(--surface));
    border: 1px solid var(--success);
    color: var(--success);
  }
  .toast-error {
    background: color-mix(in srgb, var(--error) 15%, var(--surface));
    border: 1px solid var(--error);
    color: var(--error);
  }
  .toast-info {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text-primary);
  }
  @keyframes slideIn {
    from { opacity: 0; transform: translateX(20px); }
    to { opacity: 1; transform: translateX(0); }
  }
</style>
