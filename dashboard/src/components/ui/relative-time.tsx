'use client';

import { useState, useEffect, useMemo } from 'react';

interface RelativeTimeProps {
  date: string | Date;
  className?: string;
}

function getRelativeTime(date: Date): string {
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHour = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHour / 24);
  const diffWeek = Math.floor(diffDay / 7);
  const diffMonth = Math.floor(diffDay / 30);

  if (diffSec < 60) return 'just now';
  if (diffMin < 60) return `${diffMin}m ago`;
  if (diffHour < 24) return `${diffHour}h ago`;
  if (diffDay < 7) return `${diffDay}d ago`;
  if (diffWeek < 4) return `${diffWeek}w ago`;
  if (diffMonth < 12) return `${diffMonth}mo ago`;
  return date.toLocaleDateString();
}

export function RelativeTime({ date, className }: RelativeTimeProps) {
  // Memoize timestamp to avoid recreating Date object on every render
  const timestamp = useMemo(() => {
    const d = typeof date === 'string' ? new Date(date) : date;
    return d.getTime();
  }, [date]);

  const dateObj = useMemo(() => new Date(timestamp), [timestamp]);
  const [relativeTime, setRelativeTime] = useState(() => getRelativeTime(dateObj));

  // Update relative time periodically
  useEffect(() => {
    // Update immediately in case timestamp changed
    setRelativeTime(getRelativeTime(dateObj));

    const interval = setInterval(() => {
      setRelativeTime(getRelativeTime(dateObj));
    }, 60000); // Update every minute

    return () => clearInterval(interval);
  }, [dateObj, timestamp]);

  return (
    <span
      className={className}
      title={dateObj.toLocaleString()}
    >
      {relativeTime}
    </span>
  );
}






