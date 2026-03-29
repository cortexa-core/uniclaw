export interface StreamCallbacks {
  onStatus: (type: string) => void;
  onTextDelta: (text: string) => void;
  onUsage: (usage: { input_tokens: number; output_tokens: number }) => void;
  onDone: (data: any) => void;
  onError: (error: string) => void;
}

export async function streamChat(
  message: string,
  sessionId: string,
  callbacks: StreamCallbacks,
): Promise<void> {
  const response = await fetch('/api/chat/stream', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ message, session_id: sessionId }),
  });

  if (!response.ok) {
    callbacks.onError(`HTTP ${response.status}`);
    return;
  }

  const reader = response.body?.getReader();
  if (!reader) {
    callbacks.onError('No response body');
    return;
  }

  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() || '';

    let eventType = '';
    for (const line of lines) {
      if (line.startsWith('event: ')) {
        eventType = line.slice(7).trim();
      } else if (line.startsWith('data: ')) {
        const data = line.slice(6);
        try {
          const parsed = JSON.parse(data);
          switch (eventType) {
            case 'status': callbacks.onStatus(parsed.type); break;
            case 'text_delta': callbacks.onTextDelta(parsed.text); break;
            case 'usage': callbacks.onUsage(parsed); break;
            case 'done': callbacks.onDone(parsed); break;
            case 'error': callbacks.onError(parsed.error); break;
          }
        } catch {}
      }
    }
  }
}
