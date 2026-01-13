import { Suspense } from 'react';
import { Loader } from 'lucide-react';
import ControlClient from './control-client';

export default function ControlPage() {
  return (
    <Suspense
      fallback={
        <div className="flex h-full items-center justify-center">
          <Loader className="h-6 w-6 animate-spin text-indigo-400" />
        </div>
      }
    >
      <ControlClient />
    </Suspense>
  );
}

