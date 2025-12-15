import { Suspense } from 'react';
import { ConsoleWrapper } from './console-wrapper';

export default function ConsolePage() {
  return (
    <Suspense
      fallback={
        <div className="panel rounded-lg border border-[var(--border)] bg-[var(--background-secondary)]/70 p-4 backdrop-blur-xl">
          <div className="text-sm text-[var(--foreground-muted)]">Loading console…</div>
        </div>
      }
    >
      <ConsoleWrapper />
    </Suspense>
  );
}


