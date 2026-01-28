'use client';

import { useEffect, useState } from 'react';
import { SystemMonitor } from '@/components/system-monitor';
import { getHealth, type HealthResponse } from '@/lib/api';
import { cn } from '@/lib/utils';
import {
  Server,
  Wifi,
  WifiOff,
  Shield,
  ShieldOff,
  Code,
  Clock,
  Zap,
} from 'lucide-react';

type ConnectionState = 'connected' | 'disconnected' | 'checking';

function ServerInfoCard() {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [connectionState, setConnectionState] = useState<ConnectionState>('checking');
  const [latency, setLatency] = useState<number | null>(null);
  const [lastCheck, setLastCheck] = useState<Date | null>(null);

  useEffect(() => {
    const checkHealth = async () => {
      const start = Date.now();
      try {
        const data = await getHealth();
        setHealth(data);
        setLatency(Date.now() - start);
        setConnectionState('connected');
        setLastCheck(new Date());
      } catch {
        setConnectionState('disconnected');
        setHealth(null);
      }
    };

    checkHealth();
    const interval = setInterval(checkHealth, 5000);
    return () => clearInterval(interval);
  }, []);

  const StatusIcon = connectionState === 'connected' ? Wifi : connectionState === 'checking' ? Wifi : WifiOff;
  const statusColor = connectionState === 'connected' 
    ? 'text-emerald-400' 
    : connectionState === 'checking' 
      ? 'text-amber-400' 
      : 'text-red-400';
  const dotColor = connectionState === 'connected'
    ? 'bg-emerald-400'
    : connectionState === 'checking'
      ? 'bg-amber-400 animate-pulse'
      : 'bg-red-400';

  return (
    <div className="rounded-xl bg-white/[0.02] border border-white/[0.04] p-4 h-full flex flex-col">
      {/* Header with status */}
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <Server className="h-4 w-4 text-white/40" />
          <span className="text-xs font-medium text-white/70">Server</span>
        </div>
        <div className="flex items-center gap-2">
          <span className={cn('h-2 w-2 rounded-full', dotColor)} />
          <span className={cn('text-xs font-medium', statusColor)}>
            {connectionState === 'connected' ? 'Online' : connectionState === 'checking' ? 'Checking' : 'Offline'}
          </span>
        </div>
      </div>

      {/* Stats grid */}
      <div className="space-y-3 flex-1">
        {/* Latency */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Zap className="h-3.5 w-3.5 text-white/30" />
            <span className="text-xs text-white/50">Latency</span>
          </div>
          <span className="text-xs font-medium text-white/80 tabular-nums">
            {latency !== null ? `${latency}ms` : '—'}
          </span>
        </div>

        {/* Version */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Code className="h-3.5 w-3.5 text-white/30" />
            <span className="text-xs text-white/50">Version</span>
          </div>
          <span className="text-xs font-medium text-white/80">
            {health?.version ?? '—'}
          </span>
        </div>

        {/* Auth Mode */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            {health?.auth_required ? (
              <Shield className="h-3.5 w-3.5 text-white/30" />
            ) : (
              <ShieldOff className="h-3.5 w-3.5 text-white/30" />
            )}
            <span className="text-xs text-white/50">Auth</span>
          </div>
          <span className={cn(
            'text-xs font-medium',
            health?.auth_required ? 'text-emerald-400' : 'text-white/50'
          )}>
            {health?.auth_mode === 'disabled' ? 'Disabled' : 
             health?.auth_mode === 'single_tenant' ? 'Single' :
             health?.auth_mode === 'multi_user' ? 'Multi-user' : '—'}
          </span>
        </div>

        {/* Dev Mode */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Code className="h-3.5 w-3.5 text-white/30" />
            <span className="text-xs text-white/50">Mode</span>
          </div>
          <span className={cn(
            'text-xs font-medium px-1.5 py-0.5 rounded',
            health?.dev_mode 
              ? 'bg-amber-500/10 text-amber-400' 
              : 'bg-emerald-500/10 text-emerald-400'
          )}>
            {health?.dev_mode ? 'Dev' : 'Prod'}
          </span>
        </div>

        {/* Max Iterations */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Clock className="h-3.5 w-3.5 text-white/30" />
            <span className="text-xs text-white/50">Max Iter</span>
          </div>
          <span className="text-xs font-medium text-white/80 tabular-nums">
            {health?.max_iterations ?? '—'}
          </span>
        </div>
      </div>

      {/* Last check footer - pinned to bottom */}
      <div className="pt-3 border-t border-white/[0.04] mt-auto">
        <p className="text-[10px] text-white/30 text-center">
          {lastCheck ? `Last check: ${lastCheck.toLocaleTimeString()}` : 'Checking...'}
        </p>
      </div>
    </div>
  );
}

export default function MonitoringPage() {
  return (
    <div className="flex-1 flex gap-4 p-6 min-h-0">
      {/* Server Info - Left column */}
      <div className="w-[200px] shrink-0">
        <ServerInfoCard />
      </div>

      {/* System Monitor - Right column */}
      <div className="flex-1 rounded-xl bg-white/[0.02] border border-white/[0.04] p-4 min-w-0 overflow-hidden">
        <SystemMonitor className="w-full h-full" />
      </div>
    </div>
  );
}
