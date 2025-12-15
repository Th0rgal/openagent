import { Suspense } from 'react';
import ControlClient from './control-client';

export default function ControlPage() {
  return (
    <Suspense
      fallback={
        <div className="p-6 text-sm text-[var(--foreground-muted)]">
          Loading…
        </div>
      }
    >
      <ControlClient />
    </Suspense>
  );
}

