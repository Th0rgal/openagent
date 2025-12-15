'use client';

import { cn } from '@/lib/utils';
import { LucideIcon } from 'lucide-react';

interface StatsCardProps {
  title: string;
  value: string | number;
  subtitle?: string;
  icon?: LucideIcon;
  trend?: {
    value: number;
    isPositive: boolean;
  };
  className?: string;
  color?: 'default' | 'success' | 'warning' | 'error' | 'info';
}

const colorClasses = {
  default: 'text-[var(--foreground)]',
  success: 'text-[var(--success)]',
  warning: 'text-[var(--warning)]',
  error: 'text-[var(--error)]',
  info: 'text-[var(--info)]',
};

export function StatsCard({
  title,
  value,
  subtitle,
  icon: Icon,
  trend,
  className,
  color = 'default',
}: StatsCardProps) {
  return (
    <div
      className={cn(
        'panel rounded-lg p-5',
        className
      )}
    >
      <div className="flex items-start justify-between">
        <div>
          <p className="text-xs font-medium uppercase tracking-wider text-[var(--foreground-muted)]">
            {title}
          </p>
          <div className="mt-2 flex items-baseline gap-2">
            <p className={cn('text-3xl font-bold', colorClasses[color])}>{value}</p>
            {subtitle && (
              <span className="text-sm text-[var(--foreground-muted)]">{subtitle}</span>
            )}
          </div>
          {trend && (
            <p
              className={cn(
                'mt-1 text-xs',
                trend.isPositive ? 'text-[var(--success)]' : 'text-[var(--error)]'
              )}
            >
              {trend.isPositive ? '↑' : '↓'} {Math.abs(trend.value)}%
            </p>
          )}
        </div>
        {Icon && (
          <div className="rounded-md bg-[var(--background-tertiary)]/70 p-2">
            <Icon className="h-5 w-5 text-[var(--foreground-muted)]" />
          </div>
        )}
      </div>
    </div>
  );
}

