'use client';

import { useEffect, useMemo, useState } from 'react';
import { login, getHealth } from '@/lib/api';
import { clearJwt, getValidJwt, setJwt } from '@/lib/auth';

export function AuthGate({ children }: { children: React.ReactNode }) {
  const [ready, setReady] = useState(false);
  const [authRequired, setAuthRequired] = useState(false);
  const [isAuthed, setIsAuthed] = useState(true);
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const needsLogin = useMemo(() => authRequired && !isAuthed, [authRequired, isAuthed]);

  useEffect(() => {
    let mounted = true;
    void (async () => {
      try {
        const health = await getHealth();
        if (!mounted) return;

        setAuthRequired(Boolean(health.auth_required));
        if (!health.auth_required) {
          setIsAuthed(true);
        } else {
          setIsAuthed(Boolean(getValidJwt()));
        }
      } catch {
        // If we can't reach the API, don't hard-block the UI here.
        // The dashboard will show its normal connection errors.
        if (mounted) {
          setAuthRequired(false);
          setIsAuthed(true);
        }
      } finally {
        if (mounted) setReady(true);
      }
    })();
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    const onAuthRequired = () => {
      clearJwt();
      setIsAuthed(false);
      setError(null);
    };
    window.addEventListener('openagent:auth:required', onAuthRequired);
    return () => window.removeEventListener('openagent:auth:required', onAuthRequired);
  }, []);

  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsSubmitting(true);
    setError(null);
    try {
      const res = await login(password);
      setJwt(res.token, res.exp);
      setIsAuthed(true);
      setPassword('');
    } catch {
      setError('Invalid password');
    } finally {
      setIsSubmitting(false);
    }
  };

  if (!ready) {
    return <>{children}</>;
  }

  return (
    <>
      {children}

      {needsLogin && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" />

          <div className="panel relative z-10 w-full max-w-md rounded-lg p-6">
            <h2 className="text-lg font-semibold text-[var(--foreground)]">Authenticate</h2>
            <p className="mt-1 text-sm text-[var(--foreground-muted)]">
              Enter the dashboard password to continue.
            </p>

            <form onSubmit={onSubmit} className="mt-4 space-y-3">
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Password"
                autoFocus
                className="w-full rounded-lg border border-[var(--border)] bg-[var(--background)]/60 px-4 py-2 text-sm text-[var(--foreground)] placeholder-[var(--foreground-muted)] focus:border-[var(--accent)] focus:outline-none focus-visible:!border-[var(--border)]"
              />

              {error && <p className="text-sm text-[var(--error)]">{error}</p>}

              <button
                type="submit"
                disabled={!password || isSubmitting}
                className="w-full rounded-lg bg-[var(--accent)] px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-[var(--accent)]/90 disabled:opacity-50"
              >
                {isSubmitting ? 'Signing in…' : 'Sign in'}
              </button>
            </form>
          </div>
        </div>
      )}
    </>
  );
}



