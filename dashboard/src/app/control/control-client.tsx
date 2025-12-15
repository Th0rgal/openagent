'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { cn } from '@/lib/utils';
import {
  cancelControl,
  postControlMessage,
  postControlToolResult,
  streamControl,
  type ControlRunState,
} from '@/lib/api';
import {
  Send,
  Square,
  Bot,
  User,
  Loader,
  CheckCircle,
  XCircle,
  Ban,
  Clock,
} from 'lucide-react';
import {
  OptionList,
  OptionListErrorBoundary,
  parseSerializableOptionList,
  type OptionListSelection,
} from '@/components/tool-ui/option-list';

type ChatItem =
  | {
      kind: 'user';
      id: string;
      content: string;
    }
  | {
      kind: 'assistant';
      id: string;
      content: string;
      success: boolean;
      costCents: number;
      model: string | null;
    }
  | {
      kind: 'tool';
      id: string;
      toolCallId: string;
      name: string;
      args: unknown;
      result?: unknown;
    }
  | {
      kind: 'system';
      id: string;
      content: string;
    };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function statusLabel(state: ControlRunState): {
  label: string;
  Icon: typeof Loader;
  className: string;
} {
  switch (state) {
    case 'idle':
      return { label: 'Idle', Icon: Clock, className: 'text-[var(--foreground-muted)]' };
    case 'running':
      return { label: 'Running', Icon: Loader, className: 'text-[var(--accent)]' };
    case 'waiting_for_tool':
      return { label: 'Waiting', Icon: Loader, className: 'text-[var(--warning)]' };
  }
}

export default function ControlClient() {
  const [items, setItems] = useState<ChatItem[]>([]);
  const [input, setInput] = useState('');

  const [runState, setRunState] = useState<ControlRunState>('idle');
  const [queueLen, setQueueLen] = useState(0);

  const isBusy = runState !== 'idle';

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const streamCleanupRef = useRef<null | (() => void)>(null);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [items]);

  useEffect(() => {
    streamCleanupRef.current?.();

    const cleanup = streamControl((event) => {
      const data: unknown = event.data;

      if (event.type === 'status' && isRecord(data)) {
        const st = data['state'];
        setRunState(typeof st === 'string' ? (st as ControlRunState) : 'idle');
        const q = data['queue_len'];
        setQueueLen(typeof q === 'number' ? q : 0);
        return;
      }

      if (event.type === 'user_message' && isRecord(data)) {
        setItems((prev) => [
          ...prev,
          {
            kind: 'user',
            id: String(data['id'] ?? Date.now()),
            content: String(data['content'] ?? ''),
          },
        ]);
        return;
      }

      if (event.type === 'assistant_message' && isRecord(data)) {
        setItems((prev) => [
          ...prev,
          {
            kind: 'assistant',
            id: String(data['id'] ?? Date.now()),
            content: String(data['content'] ?? ''),
            success: Boolean(data['success']),
            costCents: Number(data['cost_cents'] ?? 0),
            model: data['model'] ? String(data['model']) : null,
          },
        ]);
        return;
      }

      if (event.type === 'tool_call' && isRecord(data)) {
        const name = String(data['name'] ?? '');
        if (!name.startsWith('ui_')) return;

        setItems((prev) => [
          ...prev,
          {
            kind: 'tool',
            id: `tool-${String(data['tool_call_id'] ?? Date.now())}`,
            toolCallId: String(data['tool_call_id'] ?? ''),
            name,
            args: data['args'],
          },
        ]);
        return;
      }

      if (event.type === 'tool_result' && isRecord(data)) {
        const name = String(data['name'] ?? '');
        if (!name.startsWith('ui_')) return;

        const toolCallId = String(data['tool_call_id'] ?? '');
        setItems((prev) =>
          prev.map((it) =>
            it.kind === 'tool' && it.toolCallId === toolCallId
              ? { ...it, result: data['result'] }
              : it,
          ),
        );
        return;
      }

      if (event.type === 'error') {
        const msg =
          (isRecord(data) && data['message'] ? String(data['message']) : null) ??
          'An error occurred.';
        setItems((prev) => [
          ...prev,
          { kind: 'system', id: `err-${Date.now()}`, content: msg },
        ]);
      }
    });

    streamCleanupRef.current = cleanup;

    return () => {
      streamCleanupRef.current?.();
      streamCleanupRef.current = null;
    };
  }, []);

  const status = useMemo(() => statusLabel(runState), [runState]);
  const StatusIcon = status.Icon;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const content = input.trim();
    if (!content) return;

    setInput('');

    try {
      await postControlMessage(content);
    } catch (err) {
      console.error(err);
      setItems((prev) => [
        ...prev,
        {
          kind: 'system',
          id: `err-${Date.now()}`,
          content: 'Failed to send message to control session.',
        },
      ]);
    }
  };

  const handleStop = async () => {
    try {
      await cancelControl();
    } catch (err) {
      console.error(err);
      setItems((prev) => [
        ...prev,
        {
          kind: 'system',
          id: `err-${Date.now()}`,
          content: 'Failed to cancel control session.',
        },
      ]);
    }
  };

  return (
    <div className="flex min-h-screen flex-col p-8">
      <div className="mb-6 flex items-start justify-between gap-6">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight text-[var(--foreground)]">
            Agent Control
          </h1>
          <p className="mt-1 text-sm text-[var(--foreground-muted)]">
            Talk to the global RootAgent session (messages queue while busy)
          </p>
        </div>

        <div className="flex items-center gap-3">
          <div className={cn('flex items-center gap-2 text-sm', status.className)}>
            <StatusIcon className={cn('h-4 w-4', runState !== 'idle' && 'animate-spin')} />
            <span>{status.label}</span>
            <span className="text-[var(--foreground-muted)]">•</span>
            <span className="text-[var(--foreground-muted)]">Queue: {queueLen}</span>
          </div>
        </div>
      </div>

      <div className="panel flex-1 min-h-0 overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--background-secondary)]/70 backdrop-blur-xl">
        <div className="flex-1 overflow-y-auto p-6">
          {items.length === 0 ? (
            <div className="flex h-full items-center justify-center">
              <div className="text-center">
                <Bot className="mx-auto h-12 w-12 text-[var(--foreground-muted)]" />
                <h2 className="mt-4 text-lg font-medium text-[var(--foreground)]">
                  Start a conversation
                </h2>
                <p className="mt-2 text-sm text-[var(--foreground-muted)]">
                  Ask the agent to do something (it will queue if already busy)
                </p>
              </div>
            </div>
          ) : (
            <div className="mx-auto max-w-3xl space-y-6">
              {items.map((item) => {
                if (item.kind === 'user') {
                  return (
                    <div key={item.id} className="flex justify-end gap-4">
                      <div className="max-w-[80%] rounded-lg bg-[var(--accent)] px-4 py-3 text-white">
                        <p className="whitespace-pre-wrap text-sm">{item.content}</p>
                      </div>
                      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-[var(--background-tertiary)]">
                        <User className="h-4 w-4 text-[var(--foreground-muted)]" />
                      </div>
                    </div>
                  );
                }

                if (item.kind === 'assistant') {
                  const statusIcon = item.success ? CheckCircle : XCircle;
                  const StatusIcon = statusIcon;
                  return (
                    <div key={item.id} className="flex justify-start gap-4">
                      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-[var(--accent)] to-[var(--accent-secondary)]">
                        <Bot className="h-4 w-4 text-white" />
                      </div>
                      <div className="max-w-[80%] rounded-lg bg-[var(--background-secondary)] px-4 py-3 text-[var(--foreground)]">
                        <div className="mb-2 flex items-center gap-2 text-xs text-[var(--foreground-muted)]">
                          <StatusIcon
                            className={cn(
                              'h-3 w-3',
                              item.success ? 'text-[var(--success)]' : 'text-[var(--error)]',
                            )}
                          />
                          <span>{item.success ? 'Completed' : 'Failed'}</span>
                          {item.model && (
                            <>
                              <span>•</span>
                              <span className="font-mono">{item.model}</span>
                            </>
                          )}
                        </div>
                        <p className="whitespace-pre-wrap text-sm">{item.content}</p>
                      </div>
                    </div>
                  );
                }

                if (item.kind === 'tool') {
                  if (item.name === 'ui_optionList') {
                    const toolCallId = item.toolCallId;
                    const rawArgs: Record<string, unknown> = isRecord(item.args) ? item.args : {};

                    let optionList: ReturnType<typeof parseSerializableOptionList> | null = null;
                    let parseErr: string | null = null;
                    try {
                      optionList = parseSerializableOptionList({
                        ...rawArgs,
                        id:
                          typeof rawArgs['id'] === 'string' && rawArgs['id']
                            ? (rawArgs['id'] as string)
                            : `option-list-${toolCallId}`,
                      });
                    } catch (e) {
                      parseErr = e instanceof Error ? e.message : 'Invalid option list payload';
                    }

                    const confirmed = item.result as OptionListSelection | undefined;

                    return (
                      <div key={item.id} className="flex justify-start gap-4">
                        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-[var(--accent)] to-[var(--accent-secondary)]">
                          <Bot className="h-4 w-4 text-white" />
                        </div>
                        <div className="max-w-[80%] rounded-lg bg-[var(--background-secondary)] px-4 py-3 text-[var(--foreground)]">
                          <div className="mb-2 text-xs text-[var(--foreground-muted)]">
                            Tool UI: <span className="font-mono">{item.name}</span>
                          </div>

                          {parseErr || !optionList ? (
                            <div className="rounded-lg border border-[var(--border)] bg-[var(--background-tertiary)] p-3 text-sm text-[var(--error)]">
                              {parseErr ?? 'Failed to render OptionList'}
                            </div>
                          ) : (
                            <OptionListErrorBoundary>
                              <OptionList
                                {...optionList}
                                value={undefined}
                                confirmed={confirmed}
                                onConfirm={async (selection) => {
                                  // Optimistic receipt state.
                                  setItems((prev) =>
                                    prev.map((it) =>
                                      it.kind === 'tool' && it.toolCallId === toolCallId
                                        ? { ...it, result: selection }
                                        : it,
                                    ),
                                  );
                                  await postControlToolResult({
                                    tool_call_id: toolCallId,
                                    name: item.name,
                                    result: selection,
                                  });
                                }}
                                onCancel={async () => {
                                  setItems((prev) =>
                                    prev.map((it) =>
                                      it.kind === 'tool' && it.toolCallId === toolCallId
                                        ? { ...it, result: null }
                                        : it,
                                    ),
                                  );
                                  await postControlToolResult({
                                    tool_call_id: toolCallId,
                                    name: item.name,
                                    result: null,
                                  });
                                }}
                              />
                            </OptionListErrorBoundary>
                          )}
                        </div>
                      </div>
                    );
                  }

                  // Unknown UI tool.
                  return (
                    <div key={item.id} className="flex justify-start gap-4">
                      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-[var(--accent)] to-[var(--accent-secondary)]">
                        <Bot className="h-4 w-4 text-white" />
                      </div>
                      <div className="max-w-[80%] rounded-lg bg-[var(--background-secondary)] px-4 py-3 text-[var(--foreground)]">
                        <p className="text-sm">
                          Unsupported Tool UI: <span className="font-mono">{item.name}</span>
                        </p>
                      </div>
                    </div>
                  );
                }

                // system
                return (
                  <div key={item.id} className="flex justify-start gap-4">
                    <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-[var(--background-tertiary)]">
                      <Ban className="h-4 w-4 text-[var(--foreground-muted)]" />
                    </div>
                    <div className="max-w-[80%] rounded-lg bg-[var(--background-tertiary)] px-4 py-3 text-[var(--foreground)]">
                      <p className="whitespace-pre-wrap text-sm">{item.content}</p>
                    </div>
                  </div>
                );
              })}
              <div ref={messagesEndRef} />
            </div>
          )}
        </div>

        <div className="border-t border-[var(--border)] bg-[var(--background-secondary)]/40 backdrop-blur p-4">
          <form onSubmit={handleSubmit} className="mx-auto flex max-w-3xl gap-3">
            <input
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder="Message the root agent…"
              className="flex-1 rounded-lg border border-[var(--border)] bg-[var(--background)]/60 px-4 py-3 text-sm text-[var(--foreground)] placeholder-[var(--foreground-muted)] focus:border-[var(--accent)] focus:outline-none"
            />

            {isBusy ? (
              <button
                type="button"
                onClick={handleStop}
                className="flex items-center gap-2 rounded-lg bg-[var(--error)] px-4 py-3 text-sm font-medium text-white transition-colors hover:bg-[var(--error)]/90"
              >
                <Square className="h-4 w-4" />
                Stop
              </button>
            ) : (
              <button
                type="submit"
                disabled={!input.trim()}
                className="flex items-center gap-2 rounded-lg bg-[var(--accent)] px-4 py-3 text-sm font-medium text-white transition-colors hover:bg-[var(--accent)]/90 disabled:opacity-50"
              >
                <Send className="h-4 w-4" />
                Send
              </button>
            )}
          </form>
        </div>
      </div>
    </div>
  );
}
