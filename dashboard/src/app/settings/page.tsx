'use client';

import { useEffect } from 'react';
import { useRouter } from 'next/navigation';

export default function SettingsPage() {
  const router = useRouter();

  useEffect(() => {
    router.replace('/settings/system');
  }, [router]);

  return (
    <div className="flex items-center justify-center h-full">
      <div className="text-white/40 text-sm">Redirecting...</div>
    </div>
  );
}
