'use client';

import { useEffect, useState } from 'react';
import { StatsCard } from '@/components/stats-card';
import { ConnectionStatus } from '@/components/connection-status';
import { RecentTasks } from '@/components/recent-tasks';
import { getStats, StatsResponse } from '@/lib/api';
import { Activity, CheckCircle, DollarSign, Zap } from 'lucide-react';
import { formatCents } from '@/lib/utils';

export default function OverviewPage() {
  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [isActive, setIsActive] = useState(false);

  useEffect(() => {
    const fetchStats = async () => {
      try {
        const data = await getStats();
        setStats(data);
        setIsActive(data.active_tasks > 0);
      } catch (error) {
        console.error('Failed to fetch stats:', error);
      }
    };

    fetchStats();
    const interval = setInterval(fetchStats, 3000);
    return () => clearInterval(interval);
  }, []);

  return (
    <div className="flex min-h-screen">
      {/* Main content */}
      <div className="flex-1 p-8">
        <div className="mx-auto max-w-6xl">
          {/* Header */}
          <div className="mb-6 flex items-start justify-between gap-6">
            <div>
              <div className="flex flex-wrap items-center gap-3">
                <h1 className="text-2xl font-semibold tracking-tight text-[var(--foreground)]">
                  Global Monitor
                </h1>
                <span
                  className={[
                    'inline-flex items-center gap-2 rounded-md border px-2 py-1 text-xs font-medium',
                    isActive
                      ? 'border-[var(--accent)]/30 bg-[var(--accent)]/10 text-[var(--accent)]'
                      : 'border-[var(--border)] bg-[var(--background-secondary)] text-[var(--foreground-muted)]',
                  ].join(' ')}
                >
                  <span
                    className={[
                      'h-1.5 w-1.5 rounded-full',
                      isActive ? 'bg-[var(--accent)] animate-pulse' : 'bg-[var(--foreground-muted)]',
                    ].join(' ')}
                  />
                  {isActive ? 'Active' : 'Idle'}
                </span>
              </div>
              <p className="mt-1 text-sm text-[var(--foreground-muted)]">
                Real-time agent activity (refreshes every 3s)
              </p>
            </div>
          </div>

          {/* Stats grid */}
          <div className="grid grid-cols-4 gap-4">
            <StatsCard
              title="Total Tasks"
              value={stats?.total_tasks ?? 0}
              icon={Activity}
            />
            <StatsCard
              title="Active"
              value={stats?.active_tasks ?? 0}
              subtitle="running"
              icon={Zap}
              color={stats?.active_tasks ? 'info' : 'default'}
            />
            <StatsCard
              title="Success Rate"
              value={`${((stats?.success_rate ?? 1) * 100).toFixed(0)}%`}
              icon={CheckCircle}
              color="success"
            />
            <StatsCard
              title="Total Cost"
              value={formatCents(stats?.total_cost_cents ?? 0)}
              icon={DollarSign}
            />
          </div>
        </div>
      </div>

      {/* Right sidebar */}
      <div className="w-80 border-l border-[var(--border)] bg-[var(--background-secondary)]/70 backdrop-blur p-4">
        <RecentTasks />
        <div className="mt-4">
          <ConnectionStatus />
        </div>
      </div>
    </div>
  );
}
