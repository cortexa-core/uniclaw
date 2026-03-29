<script lang="ts">
  import { fetchSkills } from '../lib/api';
  import { marked } from 'marked';
  import { icons } from '../lib/icons';

  let data = $state<any>(null);
  let expandedSkill = $state<string | null>(null);

  $effect(() => {
    fetchSkills().then((d) => { data = d; });
  });

  function toggle(name: string) {
    expandedSkill = expandedSkill === name ? null : name;
  }
</script>

<div class="page">
  <div class="page-header">
    <h1 class="page-title">Skills</h1>
    {#if data}
      <span class="count">{data.count} loaded</span>
    {/if}
  </div>

  {#if !data}
    <p class="loading">Loading skills...</p>
  {:else if data.skills.length === 0}
    <p class="empty">No skills loaded.</p>
  {:else}
    {#each data.skills as skill}
      <button class="skill-card" onclick={() => toggle(skill.name)}>
        <div class="skill-header">
          <div class="skill-info">
            <span class="skill-name">{skill.name}</span>
            <span class="skill-desc">{skill.description}</span>
          </div>
          <span class="chevron">{@html expandedSkill === skill.name ? icons.chevronDown : icons.chevronRight}</span>
        </div>

        {#if expandedSkill === skill.name}
          <div class="skill-content">
            {@html marked.parse(skill.content)}
          </div>
        {/if}
      </button>
    {/each}
  {/if}

  <p class="hint">Skills are markdown files in data/skills/. Drop a .md file to add a new skill.</p>
</div>

<style>
  .page { max-width: 700px; }
  .page-header { display: flex; align-items: baseline; gap: 12px; margin-bottom: 24px; }
  .page-title { font-size: 20px; font-weight: 600; }
  .count { font-size: 13px; color: var(--text-secondary); }
  .loading, .empty { color: var(--text-secondary); }
  .skill-card {
    display: block;
    width: 100%;
    text-align: left;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 8px;
    transition: background var(--transition);
  }
  .skill-card:hover { background: var(--surface-hover); }
  .skill-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .skill-info { display: flex; flex-direction: column; gap: 4px; }
  .skill-name { font-weight: 600; font-size: 15px; }
  .skill-desc { font-size: 13px; color: var(--text-secondary); }
  .chevron { color: var(--text-secondary); display: flex; }
  .skill-content {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
    font-size: 14px;
    line-height: 1.6;
  }
  .skill-content :global(ul) { padding-left: 20px; }
  .skill-content :global(code) {
    background: var(--bg);
    padding: 2px 5px;
    border-radius: 4px;
    font-size: 13px;
  }
  .hint {
    margin-top: 24px;
    font-size: 13px;
    color: var(--text-secondary);
    font-style: italic;
  }
</style>
