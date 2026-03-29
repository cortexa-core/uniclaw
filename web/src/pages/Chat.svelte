<script lang="ts">
  import { streamChat } from '../lib/stream';
  import { marked } from 'marked';
  import ToolCall from '../components/ToolCall.svelte';
  import { icons } from '../lib/icons';

  interface Message {
    role: 'user' | 'assistant';
    content: string;
    timestamp: Date;
    tools?: Array<{ name: string; status: string; durationMs?: number }>;
    totalMs?: number;
  }

  let messages = $state<Message[]>([]);
  let inputText = $state('');
  let isThinking = $state(false);
  let sessionId = $state(localStorage.getItem('uniclaw-session') || 'web');
  let messagesEl: HTMLElement;
  let autoScroll = $state(true);

  function scrollToBottom() {
    if (autoScroll && messagesEl) {
      requestAnimationFrame(() => {
        messagesEl.scrollTop = messagesEl.scrollHeight;
      });
    }
  }

  function onScroll() {
    if (!messagesEl) return;
    const { scrollTop, scrollHeight, clientHeight } = messagesEl;
    autoScroll = scrollHeight - scrollTop - clientHeight < 50;
  }

  async function send() {
    const text = inputText.trim();
    if (!text || isThinking) return;

    inputText = '';
    isThinking = true;
    autoScroll = true;

    messages.push({ role: 'user', content: text, timestamp: new Date() });

    const idx = messages.length;
    messages.push({ role: 'assistant', content: '', timestamp: new Date() });

    await streamChat(text, sessionId, {
      onStatus: () => {},
      onTextDelta: (chunk) => {
        messages[idx].content += chunk;
        scrollToBottom();
      },
      onUsage: () => {},
      onDone: () => {
        isThinking = false;
        scrollToBottom();
      },
      onError: (error) => {
        messages[idx].content = `Error: ${error}`;
        isThinking = false;
      },
    });
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  function formatTime(date: Date): string {
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  function renderMarkdown(text: string): string {
    return marked.parse(text, { breaks: true }) as string;
  }

  $effect(() => {
    localStorage.setItem('uniclaw-session', sessionId);
  });
</script>

<div class="chat-page">
  <div class="chat-header">
    <h1 class="page-title">Chat</h1>
    <span class="session-label">session: {sessionId}</span>
  </div>

  <div class="messages" bind:this={messagesEl} onscroll={onScroll}>
    {#if messages.length === 0}
      <div class="empty">
        <p class="empty-title">Start a conversation</p>
        <p class="empty-detail">Messages are processed by the agent on this device.</p>
      </div>
    {/if}

    {#each messages as msg}
      <div class="message">
        <div class="message-header">
          <span class="message-role" class:assistant={msg.role === 'assistant'}>
            {msg.role === 'user' ? 'You' : 'UniClaw'}
          </span>
          <span class="message-time">{formatTime(msg.timestamp)}</span>
        </div>

        {#if msg.tools && msg.tools.length > 0}
          <ToolCall tools={msg.tools} totalMs={msg.totalMs} />
        {/if}

        {#if msg.role === 'assistant' && msg.content}
          <div class="message-content markdown">{@html renderMarkdown(msg.content)}</div>
        {:else if msg.role === 'user'}
          <div class="message-content">{msg.content}</div>
        {/if}
      </div>
    {/each}

    {#if isThinking && messages.length > 0 && messages[messages.length - 1].content === ''}
      <div class="thinking">
        <span class="spinning">{@html icons.spinner}</span>
        Thinking...
      </div>
    {/if}
  </div>

  <div class="input-bar">
    <textarea
      class="input"
      bind:value={inputText}
      onkeydown={onKeydown}
      placeholder={isThinking ? 'Thinking...' : 'Message...'}
      disabled={isThinking}
      rows="1"
    ></textarea>
    <button
      class="send-btn"
      class:active={inputText.trim().length > 0}
      onclick={send}
      disabled={isThinking || !inputText.trim()}
    >
      {@html icons.send}
    </button>
  </div>
</div>

<style>
  .chat-page {
    display: flex;
    flex-direction: column;
    height: 100%;
    max-width: 800px;
    margin: 0 auto;
  }
  .chat-header {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 16px;
    flex-shrink: 0;
  }
  .page-title { font-size: 20px; font-weight: 600; }
  .session-label { font-size: 13px; color: var(--text-secondary); }
  .messages {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding-bottom: 16px;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-secondary);
  }
  .empty-title { font-size: 16px; color: var(--text-primary); }
  .message-header {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin-bottom: 4px;
  }
  .message-role {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  .message-role.assistant { color: var(--accent); }
  .message-time {
    font-size: 12px;
    color: var(--text-secondary);
    margin-left: auto;
  }
  .message-content { font-size: 15px; line-height: 1.6; }
  .message-content :global(pre) { margin: 8px 0; }
  .message-content :global(code) {
    background: var(--surface);
    padding: 2px 5px;
    border-radius: 4px;
    font-size: 13px;
  }
  .message-content :global(pre code) {
    background: none;
    padding: 0;
  }
  .thinking {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent);
    font-size: 14px;
  }
  .spinning {
    display: inline-flex;
    animation: spin 1s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .input-bar {
    display: flex;
    gap: 8px;
    padding: 12px 0;
    flex-shrink: 0;
    border-top: 1px solid var(--border);
  }
  .input {
    flex: 1;
    resize: none;
    min-height: 42px;
    max-height: 120px;
  }
  .send-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 42px;
    height: 42px;
    border-radius: var(--radius);
    color: var(--text-secondary);
    background: var(--surface);
    border: 1px solid var(--border);
    flex-shrink: 0;
  }
  .send-btn.active {
    color: var(--bg);
    background: var(--accent);
    border-color: var(--accent);
  }
  .send-btn:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
