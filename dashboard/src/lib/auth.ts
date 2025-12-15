const TOKEN_KEY = 'openagent.jwt';
const EXP_KEY = 'openagent.jwt_exp';

export function getStoredJwt(): { token: string; exp: number } | null {
  if (typeof window === 'undefined') return null;
  const token = sessionStorage.getItem(TOKEN_KEY);
  const expRaw = sessionStorage.getItem(EXP_KEY);
  if (!token || !expRaw) return null;
  const exp = Number(expRaw);
  if (!Number.isFinite(exp)) return null;
  return { token, exp };
}

export function isJwtValid(exp: number, skewSeconds = 15): boolean {
  const now = Math.floor(Date.now() / 1000);
  return exp > now + skewSeconds;
}

export function getValidJwt(): { token: string; exp: number } | null {
  const stored = getStoredJwt();
  if (!stored) return null;
  if (!isJwtValid(stored.exp)) {
    clearJwt();
    return null;
  }
  return stored;
}

export function setJwt(token: string, exp: number): void {
  if (typeof window === 'undefined') return;
  sessionStorage.setItem(TOKEN_KEY, token);
  sessionStorage.setItem(EXP_KEY, String(exp));
}

export function clearJwt(): void {
  if (typeof window === 'undefined') return;
  sessionStorage.removeItem(TOKEN_KEY);
  sessionStorage.removeItem(EXP_KEY);
}

export function authHeader(): Record<string, string> {
  const jwt = getValidJwt();
  if (!jwt) return {};
  return { Authorization: `Bearer ${jwt.token}` };
}

export function signalAuthRequired(): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent('openagent:auth:required'));
}



