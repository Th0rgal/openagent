'use client';

import { useEffect, useState } from 'react';
import { cn } from '@/lib/utils';
import { getHealth } from '@/lib/api';

interface ConnectionItem {
  name: string;
  status: 'connected' | 'disconnected' | 'checking';
  latency?: number;
}

export function ConnectionStatus() {
  const [connections, setConnections] = useState<ConnectionItem[]>([
    { name: 'Dashboard → API', status: 'checking' },
    { name: 'API → LLM', status: 'checking' },
  ]);
  const [overallStatus, setOverallStatus] = useState<'all' | 'partial' | 'none'>('partial');

  useEffect(() => {
    const checkConnections = async () => {
      const start = Date.now();
      try {
        await getHealth();
        const latency = Date.now() - start;
        setConnections([
          { name: 'Dashboard → API', status: 'connected', latency },
          { name: 'API → LLM', status: 'connected' },
        ]);
        setOverallStatus('all');
      } catch {
        setConnections([
          { name: 'Dashboard → API', status: 'disconnected' },
          { name: 'API → LLM', status: 'disconnected' },
        ]);
        setOverallStatus('none');
      }
    };

    checkConnections();
    const interval = setInterval(checkConnections, 5000);
    return () => clearInterval(interval);
  }, []);

  const statusColors = {
    connected: 'bg-[var(--success)]',
    disconnected: 'bg-[var(--error)]',
    checking: 'bg-[var(--warning)]',
  };

  return (
    <div className="panel rounded-lg p-4">
      <h3 className="mb-4 text-sm font-semibold text-[var(--foreground)]">
        Connection Status
      </h3>

      <div className="space-y-4">
        {connections.map((conn, i) => (
          <div key={i} className="flex items-center justify-between">
            <div>
              <p className="text-sm text-[var(--foreground-muted)]">{conn.name}</p>
              {conn.latency !== undefined && (
                <p className="text-2xl font-bold text-[var(--foreground)]">
                  {conn.latency}
                  <span className="text-sm font-normal text-[var(--foreground-muted)]">ms</span>
                </p>
              )}
            </div>
            <div
              className={cn(
                'h-2.5 w-2.5 rounded-full',
                statusColors[conn.status]
              )}
            />
          </div>
        ))}
      </div>

      <div className="mt-4 flex items-center justify-between border-t border-[var(--border)] pt-4">
        <span className="text-sm text-[var(--foreground-muted)]">All Systems</span>
        <span
          className={cn(
            'text-sm font-medium',
            overallStatus === 'all' && 'text-[var(--success)]',
            overallStatus === 'partial' && 'text-[var(--warning)]',
            overallStatus === 'none' && 'text-[var(--error)]'
          )}
        >
          {overallStatus === 'all' && 'Operational'}
          {overallStatus === 'partial' && 'Partial'}
          {overallStatus === 'none' && 'Offline'}
        </span>
      </div>
    </div>
  );
}

