'use client';

import { useEffect, useMemo, useState } from 'react';
import { authHeader, getValidJwt } from '@/lib/auth';
import { getRuntimeApiBase } from '@/lib/settings';

type FsEntry = {
  name: string;
  path: string;
  kind: 'file' | 'dir' | 'link' | 'other' | string;
  size: number;
  mtime: number;
};

function formatBytes(n: number) {
  if (!Number.isFinite(n)) return '-';
  if (n < 1024) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB'] as const;
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(v >= 10 ? 0 : 1)} ${units[i]}`;
}

async function listDir(path: string): Promise<FsEntry[]> {
  const API_BASE = getRuntimeApiBase();
  const res = await fetch(`${API_BASE}/api/fs/list?path=${encodeURIComponent(path)}`, {
    headers: { ...authHeader() },
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

async function mkdir(path: string): Promise<void> {
  const API_BASE = getRuntimeApiBase();
  const res = await fetch(`${API_BASE}/api/fs/mkdir`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...authHeader() },
    body: JSON.stringify({ path }),
  });
  if (!res.ok) throw new Error(await res.text());
}

async function rm(path: string, recursive = false): Promise<void> {
  const API_BASE = getRuntimeApiBase();
  const res = await fetch(`${API_BASE}/api/fs/rm`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...authHeader() },
    body: JSON.stringify({ path, recursive }),
  });
  if (!res.ok) throw new Error(await res.text());
}

async function downloadFile(path: string) {
  const API_BASE = getRuntimeApiBase();
  const res = await fetch(`${API_BASE}/api/fs/download?path=${encodeURIComponent(path)}`, {
    headers: { ...authHeader() },
  });
  if (!res.ok) throw new Error(await res.text());
  const blob = await res.blob();
  const name = path.split('/').filter(Boolean).pop() ?? 'download';
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

async function uploadFiles(dir: string, files: File[], onProgress?: (done: number, total: number) => void) {
  let done = 0;
  for (const f of files) {
    await new Promise<void>((resolve, reject) => {
      const API_BASE = getRuntimeApiBase();
      const form = new FormData();
      form.append('file', f, f.name);
      const xhr = new XMLHttpRequest();
      xhr.open('POST', `${API_BASE}/api/fs/upload?path=${encodeURIComponent(dir)}`, true);
      const jwt = getValidJwt()?.token;
      if (jwt) xhr.setRequestHeader('Authorization', `Bearer ${jwt}`);
      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) resolve();
        else reject(new Error(xhr.responseText || `Upload failed (${xhr.status})`));
      };
      xhr.onerror = () => reject(new Error('Upload failed (network error)'));
      xhr.send(form);
    });
    done += 1;
    onProgress?.(done, files.length);
  }
}

export default function FilesPage() {
  const [cwd, setCwd] = useState('/root');
  const [entries, setEntries] = useState<FsEntry[]>([]);
  const [fsLoading, setFsLoading] = useState(false);
  const [fsError, setFsError] = useState<string | null>(null);
  const [selected, setSelected] = useState<FsEntry | null>(null);
  const [uploading, setUploading] = useState<{ done: number; total: number } | null>(null);

  const sortedEntries = useMemo(() => {
    const dirs = entries.filter((e) => e.kind === 'dir').sort((a, b) => a.name.localeCompare(b.name));
    const files = entries.filter((e) => e.kind !== 'dir').sort((a, b) => a.name.localeCompare(b.name));
    return [...dirs, ...files];
  }, [entries]);

  async function refreshDir(path: string) {
    setFsLoading(true);
    setFsError(null);
    try {
      const data = await listDir(path);
      setEntries(data);
      setSelected(null);
    } catch (e) {
      setFsError(e instanceof Error ? e.message : String(e));
    } finally {
      setFsLoading(false);
    }
  }

  useEffect(() => {
    void refreshDir(cwd);
  }, [cwd]);

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-semibold text-[var(--foreground)]">Files</h1>
        <p className="mt-1 text-sm text-[var(--foreground-muted)]">Remote file explorer (SFTP).</p>
      </div>

      <div className="panel rounded-lg border border-[var(--border)] bg-[var(--background-secondary)]/70 p-3 backdrop-blur-xl">
        <div className="mb-2 flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--foreground)]">Explorer</div>
          <div className="flex items-center gap-2">
            <button
              className="rounded-md border border-[var(--border)] bg-[var(--background-tertiary)] px-2 py-1 text-xs text-[var(--foreground)] hover:bg-[var(--background-tertiary)]/70"
              onClick={() => void refreshDir(cwd)}
            >
              Refresh
            </button>
            <button
              className="rounded-md border border-[var(--border)] bg-[var(--background-tertiary)] px-2 py-1 text-xs text-[var(--foreground)] hover:bg-[var(--background-tertiary)]/70"
              onClick={async () => {
                const name = prompt('New folder name');
                if (!name) return;
                const target = cwd.endsWith('/') ? `${cwd}${name}` : `${cwd}/${name}`;
                await mkdir(target);
                await refreshDir(cwd);
              }}
            >
              New folder
            </button>
          </div>
        </div>

        <div className="mb-3 flex items-center gap-2">
          <button
            className="rounded-md border border-[var(--border)] bg-[var(--background-tertiary)] px-2 py-1 text-xs text-[var(--foreground)] hover:bg-[var(--background-tertiary)]/70"
            onClick={() => {
              const parts = cwd.split('/').filter(Boolean);
              if (parts.length === 0) return;
              parts.pop();
              setCwd('/' + parts.join('/'));
            }}
            disabled={cwd === '/'}
          >
            Up
          </button>
          <input
            className="w-full rounded-md border border-[var(--border)] bg-[var(--background)]/40 px-3 py-2 text-sm text-[var(--foreground)] placeholder:text-[var(--foreground-muted)] focus-visible:!border-[var(--border)]"
            value={cwd}
            onChange={(e) => setCwd(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void refreshDir(cwd);
            }}
          />
        </div>

        <div
          className="mb-3 rounded-md border border-dashed border-[var(--border)] bg-[var(--background)]/20 p-3 text-sm text-[var(--foreground-muted)]"
          onDragOver={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
          onDrop={async (e) => {
            e.preventDefault();
            e.stopPropagation();
            const files = Array.from(e.dataTransfer.files || []);
            if (files.length === 0) return;
            setUploading({ done: 0, total: files.length });
            try {
              await uploadFiles(cwd, files, (done, total) => setUploading({ done, total }));
              await refreshDir(cwd);
            } catch (err) {
              setFsError(err instanceof Error ? err.message : String(err));
            } finally {
              setUploading(null);
            }
          }}
        >
          Drag & drop to upload into <span className="text-[var(--foreground)]">{cwd}</span>
          {uploading ? (
            <span className="ml-2 text-xs">
              ({uploading.done}/{uploading.total})
            </span>
          ) : null}
        </div>

        {fsError ? (
          <div className="mb-3 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-sm text-red-200">
            {fsError}
          </div>
        ) : null}

        <div className="grid gap-3 md:grid-cols-5">
          <div className="md:col-span-3">
            <div className="rounded-md border border-[var(--border)] bg-[var(--background)]/30">
              <div className="grid grid-cols-12 gap-2 border-b border-[var(--border)] px-3 py-2 text-xs text-[var(--foreground-muted)]">
                <div className="col-span-7">Name</div>
                <div className="col-span-3">Size</div>
                <div className="col-span-2">Type</div>
              </div>
              <div className="max-h-[520px] overflow-auto">
                {fsLoading ? (
                  <div className="px-3 py-3 text-sm text-[var(--foreground-muted)]">Loading…</div>
                ) : sortedEntries.length === 0 ? (
                  <div className="px-3 py-3 text-sm text-[var(--foreground-muted)]">Empty</div>
                ) : (
                  sortedEntries.map((e) => (
                    <button
                      key={e.path}
                      className={
                        'grid w-full grid-cols-12 gap-2 px-3 py-2 text-left text-sm hover:bg-[var(--background-tertiary)]/60 ' +
                        (selected?.path === e.path ? 'bg-[var(--accent)]/10' : '')
                      }
                      onClick={() => setSelected(e)}
                      onDoubleClick={() => {
                        if (e.kind === 'dir') setCwd(e.path);
                      }}
                    >
                      <div className="col-span-7 truncate text-[var(--foreground)]">{e.name}</div>
                      <div className="col-span-3 text-[var(--foreground-muted)]">
                        {e.kind === 'file' ? formatBytes(e.size) : '-'}
                      </div>
                      <div className="col-span-2 text-[var(--foreground-muted)]">{e.kind}</div>
                    </button>
                  ))
                )}
              </div>
            </div>
          </div>

          <div className="md:col-span-2">
            <div className="rounded-md border border-[var(--border)] bg-[var(--background)]/30 p-3">
              <div className="text-sm font-medium text-[var(--foreground)]">Selection</div>
              {selected ? (
                <div className="mt-2 space-y-2 text-sm">
                  <div className="break-words text-[var(--foreground)]">{selected.path}</div>
                  <div className="text-[var(--foreground-muted)]">
                    <span className="text-[var(--foreground)]">Type:</span> {selected.kind}
                  </div>
                  {selected.kind === 'file' ? (
                    <div className="text-[var(--foreground-muted)]">
                      <span className="text-[var(--foreground)]">Size:</span> {formatBytes(selected.size)}
                    </div>
                  ) : null}
                  <div className="flex flex-wrap gap-2 pt-1">
                    {selected.kind === 'file' ? (
                      <button
                        className="rounded-md border border-[var(--border)] bg-[var(--background-tertiary)] px-2 py-1 text-xs text-[var(--foreground)] hover:bg-[var(--background-tertiary)]/70"
                        onClick={() => void downloadFile(selected.path)}
                      >
                        Download
                      </button>
                    ) : null}
                    <button
                      className="rounded-md border border-red-500/30 bg-red-500/10 px-2 py-1 text-xs text-red-200 hover:bg-red-500/15"
                      onClick={async () => {
                        if (!confirm(`Delete ${selected.path}?`)) return;
                        await rm(selected.path, selected.kind === 'dir');
                        await refreshDir(cwd);
                      }}
                    >
                      Delete
                    </button>
                  </div>
                </div>
              ) : (
                <div className="mt-2 text-sm text-[var(--foreground-muted)]">Click a file/folder.</div>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}


