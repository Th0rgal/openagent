'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { cn } from '@/lib/utils';
import { listTasks, TaskState } from '@/lib/api';
import { ArrowRight, CheckCircle, XCircle, Loader, Clock, Ban } from 'lucide-react';

const statusIcons = {
  pending: Clock,
  running: Loader,
  completed: CheckCircle,
  failed: XCircle,
  cancelled: Ban,
};

const statusColors = {
  pending: 'text-[var(--warning)]',
  running: 'text-[var(--accent)]',
  completed: 'text-[var(--success)]',
  failed: 'text-[var(--error)]',
  cancelled: 'text-[var(--foreground-muted)]',
};

export function RecentTasks() {
  const [tasks, setTasks] = useState<TaskState[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const fetchTasks = async () => {
      try {
        const data = await listTasks();
        setTasks(data.slice(0, 5));
      } catch (error) {
        console.error('Failed to fetch tasks:', error);
      } finally {
        setLoading(false);
      }
    };

    fetchTasks();
    const interval = setInterval(fetchTasks, 3000);
    return () => clearInterval(interval);
  }, []);

  if (loading) {
    return (
      <div className="panel rounded-lg p-4">
        <h3 className="mb-4 text-sm font-semibold text-[var(--foreground)]">Recent Tasks</h3>
        <p className="text-sm text-[var(--foreground-muted)]">Loading...</p>
      </div>
    );
  }

  return (
    <div className="panel rounded-lg p-4">
      <div className="mb-4 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="relative flex h-2 w-2">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-[var(--success)] opacity-75" />
            <span className="relative inline-flex h-2 w-2 rounded-full bg-[var(--success)]" />
          </span>
          <span className="rounded-md bg-[var(--success)]/10 px-2 py-0.5 text-xs font-medium text-[var(--success)]">
            LIVE
          </span>
        </div>
      </div>

      <h3 className="mb-4 text-sm font-semibold text-[var(--foreground)]">Recent Tasks</h3>

      {tasks.length === 0 ? (
        <p className="text-sm text-[var(--foreground-muted)]">No tasks yet</p>
      ) : (
        <div className="space-y-3">
          {tasks.map((task) => {
            const Icon = statusIcons[task.status];
            return (
              <Link
                key={task.id}
                href={`/control?task=${task.id}`}
                className="flex items-center justify-between rounded-md bg-[var(--background-tertiary)]/60 p-3 transition-colors hover:bg-[var(--background-tertiary)]"
              >
                <div className="flex items-center gap-3">
                  <Icon
                    className={cn(
                      'h-4 w-4',
                      statusColors[task.status],
                      task.status === 'running' && 'animate-spin'
                    )}
                  />
                  <span className="max-w-[200px] truncate text-sm text-[var(--foreground)]">
                    {task.task}
                  </span>
                </div>
                <ArrowRight className="h-4 w-4 text-[var(--foreground-muted)]" />
              </Link>
            );
          })}
        </div>
      )}

      <Link
        href="/history"
        className="mt-4 flex items-center gap-1 text-sm text-[var(--accent)] hover:underline"
      >
        View all <ArrowRight className="h-3 w-3" />
      </Link>
    </div>
  );
}

