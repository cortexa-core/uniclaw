<script lang="ts">
  let { checked = false, onchange }: { checked: boolean; onchange: (val: boolean) => void } = $props();
</script>

<button class="toggle" class:on={checked} onclick={() => onchange(!checked)} type="button">
  <span class="knob"></span>
</button>

<style>
  .toggle {
    width: 40px;
    height: 22px;
    border-radius: 11px;
    background: var(--border);
    position: relative;
    flex-shrink: 0;
    transition: background var(--transition);
  }
  .toggle.on { background: var(--accent); }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: white;
    transition: transform var(--transition);
  }
  .toggle.on .knob { transform: translateX(18px); }
</style>
