"use client";

import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import { Terminal as XTerm } from "xterm";
import { FitAddon } from "xterm-addon-fit";
import "xterm/css/xterm.css";

import { authHeader, getValidJwt } from "@/lib/auth";
import { getRuntimeApiBase } from "@/lib/settings";

type FsEntry = {
  name: string;
  path: string;
  kind: "file" | "dir" | "link" | "other" | string;
  size: number;
  mtime: number;
};

type TabType = "terminal" | "files";

type Tab = {
  id: string;
  type: TabType;
  title: string;
};

function formatBytes(n: number) {
  if (!Number.isFinite(n)) return "-";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"] as const;
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
  const res = await fetch(
    `${API_BASE}/api/fs/list?path=${encodeURIComponent(path)}`,
    {
      headers: { ...authHeader() },
    }
  );
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

async function mkdir(path: string): Promise<void> {
  const API_BASE = getRuntimeApiBase();
  const res = await fetch(`${API_BASE}/api/fs/mkdir`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeader() },
    body: JSON.stringify({ path }),
  });
  if (!res.ok) throw new Error(await res.text());
}

async function rm(path: string, recursive = false): Promise<void> {
  const API_BASE = getRuntimeApiBase();
  const res = await fetch(`${API_BASE}/api/fs/rm`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeader() },
    body: JSON.stringify({ path, recursive }),
  });
  if (!res.ok) throw new Error(await res.text());
}

async function downloadFile(path: string) {
  const API_BASE = getRuntimeApiBase();
  const res = await fetch(
    `${API_BASE}/api/fs/download?path=${encodeURIComponent(path)}`,
    {
      headers: { ...authHeader() },
    }
  );
  if (!res.ok) throw new Error(await res.text());
  const blob = await res.blob();
  const name = path.split("/").filter(Boolean).pop() ?? "download";
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

async function uploadFiles(
  dir: string,
  files: File[],
  onProgress?: (done: number, total: number) => void
) {
  let done = 0;
  for (const f of files) {
    await new Promise<void>((resolve, reject) => {
      const API_BASE = getRuntimeApiBase();
      const form = new FormData();
      form.append("file", f, f.name);
      const xhr = new XMLHttpRequest();
      xhr.open(
        "POST",
        `${API_BASE}/api/fs/upload?path=${encodeURIComponent(dir)}`,
        true
      );
      const jwt = getValidJwt()?.token;
      if (jwt) xhr.setRequestHeader("Authorization", `Bearer ${jwt}`);
      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) resolve();
        else
          reject(
            new Error(xhr.responseText || `Upload failed (${xhr.status})`)
          );
      };
      xhr.onerror = () => reject(new Error("Upload failed (network error)"));
      xhr.send(form);
    });
    done += 1;
    onProgress?.(done, files.length);
  }
}

// Generate unique IDs
let tabIdCounter = 0;
function generateTabId(): string {
  return `tab-${++tabIdCounter}-${Date.now()}`;
}

// Terminal Tab Component
function TerminalTab({ tabId, isActive }: { tabId: string; isActive: boolean }) {
  const termElRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<XTerm | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const mountedRef = useRef(true);
  const initAttemptedRef = useRef(false);
  const [wsStatus, setWsStatus] = useState<
    "disconnected" | "connecting" | "connected" | "error"
  >("disconnected");

  // Initialize terminal and WebSocket
  useEffect(() => {
    // Track mount state for async operations
    mountedRef.current = true;
    
    // Only init once per tab instance, and only when active
    if (!isActive || initAttemptedRef.current) return;
    initAttemptedRef.current = true;

    const container = termElRef.current;
    if (!container) return;

    // Create terminal
    const term = new XTerm({
      fontFamily:
        'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
      fontSize: 13,
      lineHeight: 1.25,
      cursorBlink: true,
      convertEol: true,
      allowProposedApi: true,
      theme: {
        background: "transparent",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);

    termRef.current = term;
    fitRef.current = fit;

    // Defer opening to next frame to ensure container has dimensions
    const openFrame = requestAnimationFrame(() => {
      if (!mountedRef.current) return;
      try {
        term.open(container);
        // Another frame for fit after open
        requestAnimationFrame(() => {
          if (!mountedRef.current) return;
          try {
            fit.fit();
          } catch {
            // Ignore fit errors
          }
        });
      } catch {
        // Ignore open errors
      }
    });

    // Resize handler
    const onResize = () => {
      if (!mountedRef.current) return;
      try {
        fit.fit();
        const ws = wsRef.current;
        if (ws?.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ t: "r", c: term.cols, r: term.rows }));
        }
      } catch {
        // Ignore
      }
    };
    window.addEventListener("resize", onResize);

    // Forward terminal input to WebSocket
    const onDataDisposable = term.onData((d) => {
      const ws = wsRef.current;
      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ t: "i", d }));
      }
    });

    // Connect WebSocket
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setWsStatus("connecting");
    const jwt = getValidJwt()?.token ?? null;
    const proto = jwt
      ? (["openagent", `jwt.${jwt}`] as string[])
      : (["openagent"] as string[]);
    const API_BASE = getRuntimeApiBase();
    const u = new URL(`${API_BASE}/api/console/ws`);
    u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(u.toString(), proto);
    wsRef.current = ws;

    ws.onopen = () => {
      if (!mountedRef.current) return;
      setWsStatus("connected");
      term.writeln("\x1b[1;34mConnected.\x1b[0m");
      // Fit and send dimensions after connection
      setTimeout(() => {
        if (!mountedRef.current) return;
        try {
          fit.fit();
          ws.send(JSON.stringify({ t: "r", c: term.cols, r: term.rows }));
        } catch {
          // Ignore
        }
      }, 50);
    };
    ws.onmessage = (evt) => {
      if (!mountedRef.current) return;
      term.write(typeof evt.data === "string" ? evt.data : "");
    };
    ws.onerror = () => {
      if (mountedRef.current) setWsStatus("error");
    };
    ws.onclose = () => {
      if (mountedRef.current) setWsStatus("disconnected");
    };

    // Cleanup on unmount
    return () => {
      mountedRef.current = false;
      cancelAnimationFrame(openFrame);
      window.removeEventListener("resize", onResize);
      try { onDataDisposable.dispose(); } catch { /* ignore */ }
      try { ws.close(); } catch { /* ignore */ }
      try { term.dispose(); } catch { /* ignore */ }
      wsRef.current = null;
      termRef.current = null;
      fitRef.current = null;
    };
  }, [isActive]);

  // Reconnect function
  const reconnect = useCallback(() => {
    // Close existing
    try { wsRef.current?.close(); } catch { /* ignore */ }
    
    const term = termRef.current;
    const fit = fitRef.current;
    if (!term) return;

    setWsStatus("connecting");
    const jwt = getValidJwt()?.token ?? null;
    const proto = jwt
      ? (["openagent", `jwt.${jwt}`] as string[])
      : (["openagent"] as string[]);
    const API_BASE = getRuntimeApiBase();
    const u = new URL(`${API_BASE}/api/console/ws`);
    u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(u.toString(), proto);
    wsRef.current = ws;

    ws.onopen = () => {
      if (!mountedRef.current) return;
      setWsStatus("connected");
      term.writeln("\x1b[1;34mReconnected.\x1b[0m");
      setTimeout(() => {
        if (!mountedRef.current) return;
        try {
          fit?.fit();
          ws.send(JSON.stringify({ t: "r", c: term.cols, r: term.rows }));
        } catch { /* ignore */ }
      }, 50);
    };
    ws.onmessage = (evt) => {
      if (!mountedRef.current) return;
      term.write(typeof evt.data === "string" ? evt.data : "");
    };
    ws.onerror = () => { if (mountedRef.current) setWsStatus("error"); };
    ws.onclose = () => { if (mountedRef.current) setWsStatus("disconnected"); };
  }, []);

  // Fit terminal when tab becomes active
  useEffect(() => {
    if (isActive && fitRef.current) {
      // Delay fit to allow layout to settle
      const timer = setTimeout(() => {
        try { fitRef.current?.fit(); } catch { /* ignore */ }
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [isActive]);

  return (
    <div
      className={[
        "absolute inset-0 flex h-full min-h-0 flex-col p-4",
        isActive ? "opacity-100" : "pointer-events-none opacity-0",
      ].join(" ")}
      aria-label={`terminal-tab-${tabId}`}
    >
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span
            className={
              wsStatus === "connected"
                ? "h-2 w-2 rounded-full bg-emerald-500"
                : wsStatus === "connecting"
                ? "h-2 w-2 rounded-full bg-yellow-500 animate-pulse"
                : wsStatus === "error"
                ? "h-2 w-2 rounded-full bg-red-500"
                : "h-2 w-2 rounded-full bg-gray-500"
            }
          />
          <span className="text-xs text-[var(--foreground-muted)]">
            {wsStatus}
          </span>
        </div>
        <button
          className="rounded-md border border-[var(--border)] bg-[var(--background-tertiary)] px-2 py-1 text-xs text-[var(--foreground)] hover:bg-[var(--background-tertiary)]/70"
          onClick={reconnect}
        >
          Reconnect
        </button>
      </div>
      <div
        className="flex-1 min-h-0 rounded-md border border-[var(--border)] bg-[var(--background)]/40 overflow-hidden"
        ref={termElRef}
      />
    </div>
  );
}

// Files Tab Component
function FilesTab({ isActive }: { tabId: string; isActive: boolean }) {
  const [cwd, setCwd] = useState("/root");
  const [entries, setEntries] = useState<FsEntry[]>([]);
  const [fsLoading, setFsLoading] = useState(false);
  const [fsError, setFsError] = useState<string | null>(null);
  const [selected, setSelected] = useState<FsEntry | null>(null);
  const [uploading, setUploading] = useState<{
    done: number;
    total: number;
  } | null>(null);

  const sortedEntries = useMemo(() => {
    const dirs = entries
      .filter((e) => e.kind === "dir")
      .sort((a, b) => a.name.localeCompare(b.name));
    const files = entries
      .filter((e) => e.kind !== "dir")
      .sort((a, b) => a.name.localeCompare(b.name));
    return [...dirs, ...files];
  }, [entries]);

  const refreshDir = useCallback(async (path: string) => {
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
  }, []);

  useEffect(() => {
    if (isActive) {
      void refreshDir(cwd);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwd, isActive]);

  return (
    <div
      className={[
        "absolute inset-0 flex h-full min-h-0 flex-col p-4",
        isActive ? "opacity-100" : "pointer-events-none opacity-0",
      ].join(" ")}
    >
      <div className="mb-3 flex items-center justify-between">
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
              const name = prompt("New folder name");
              if (!name) return;
              const target = cwd.endsWith("/")
                ? `${cwd}${name}`
                : `${cwd}/${name}`;
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
            const parts = cwd.split("/").filter(Boolean);
            if (parts.length === 0) return;
            parts.pop();
            setCwd("/" + parts.join("/"));
          }}
          disabled={cwd === "/"}
        >
          Up
        </button>
        <input
          className="w-full rounded-md border border-[var(--border)] bg-[var(--background)]/40 px-3 py-2 text-sm text-[var(--foreground)] placeholder:text-[var(--foreground-muted)] focus-visible:!border-[var(--border)]"
          value={cwd}
          onChange={(e) => setCwd(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void refreshDir(cwd);
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
            await uploadFiles(cwd, files, (done, total) =>
              setUploading({ done, total })
            );
            await refreshDir(cwd);
          } catch (err) {
            setFsError(err instanceof Error ? err.message : String(err));
          } finally {
            setUploading(null);
          }
        }}
      >
        Drag & drop to upload into{" "}
        <span className="text-[var(--foreground)]">{cwd}</span>
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

      <div className="flex-1 grid gap-3 md:grid-cols-5 min-h-0">
        <div className="md:col-span-3 overflow-hidden">
          <div className="h-full rounded-md border border-[var(--border)] bg-[var(--background)]/30 flex flex-col">
            <div className="grid grid-cols-12 gap-2 border-b border-[var(--border)] px-3 py-2 text-xs text-[var(--foreground-muted)]">
              <div className="col-span-7">Name</div>
              <div className="col-span-3">Size</div>
              <div className="col-span-2">Type</div>
            </div>
            <div className="flex-1 overflow-auto">
              {fsLoading ? (
                <div className="px-3 py-3 text-sm text-[var(--foreground-muted)]">
                  Loading…
                </div>
              ) : sortedEntries.length === 0 ? (
                <div className="px-3 py-3 text-sm text-[var(--foreground-muted)]">
                  Empty
                </div>
              ) : (
                sortedEntries.map((e) => (
                  <button
                    key={e.path}
                    className={
                      "grid w-full grid-cols-12 gap-2 px-3 py-2 text-left text-sm hover:bg-[var(--background-tertiary)]/60 " +
                      (selected?.path === e.path
                        ? "bg-[var(--accent)]/10"
                        : "")
                    }
                    onClick={() => setSelected(e)}
                    onDoubleClick={() => {
                      if (e.kind === "dir") setCwd(e.path);
                    }}
                  >
                    <div className="col-span-7 truncate text-[var(--foreground)]">
                      {e.kind === "dir" ? "📁 " : "📄 "}{e.name}
                    </div>
                    <div className="col-span-3 text-[var(--foreground-muted)]">
                      {e.kind === "file" ? formatBytes(e.size) : "-"}
                    </div>
                    <div className="col-span-2 text-[var(--foreground-muted)]">
                      {e.kind}
                    </div>
                  </button>
                ))
              )}
            </div>
          </div>
        </div>

        <div className="md:col-span-2">
          <div className="rounded-md border border-[var(--border)] bg-[var(--background)]/30 p-3">
            <div className="text-sm font-medium text-[var(--foreground)]">
              Selection
            </div>
            {selected ? (
              <div className="mt-2 space-y-2 text-sm">
                <div className="break-words text-[var(--foreground)]">
                  {selected.path}
                </div>
                <div className="text-[var(--foreground-muted)]">
                  <span className="text-[var(--foreground)]">Type:</span>{" "}
                  {selected.kind}
                </div>
                {selected.kind === "file" ? (
                  <div className="text-[var(--foreground-muted)]">
                    <span className="text-[var(--foreground)]">Size:</span>{" "}
                    {formatBytes(selected.size)}
                  </div>
                ) : null}
                <div className="flex flex-wrap gap-2 pt-1">
                  {selected.kind === "file" ? (
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
                      await rm(selected.path, selected.kind === "dir");
                      await refreshDir(cwd);
                    }}
                  >
                    Delete
                  </button>
                </div>
              </div>
            ) : (
              <div className="mt-2 text-sm text-[var(--foreground-muted)]">
                Click a file/folder.
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export default function ConsoleClient() {
  const [tabs, setTabs] = useState<Tab[]>([
    { id: generateTabId(), type: "terminal", title: "Terminal 1" },
    { id: generateTabId(), type: "files", title: "Files 1" },
  ]);
  const [activeTabId, setActiveTabId] = useState<string>(tabs[0].id);
  const [showNewTabMenu, setShowNewTabMenu] = useState(false);

  const addTab = (type: TabType) => {
    const newTabId = generateTabId();
    setTabs((prev) => {
      const terminalCount = prev.filter((t) => t.type === "terminal").length;
      const filesCount = prev.filter((t) => t.type === "files").length;
      const count = type === "terminal" ? terminalCount + 1 : filesCount + 1;
      const title = type === "terminal" ? `Terminal ${count}` : `Files ${count}`;
      return [...prev, { id: newTabId, type, title }];
    });
    setActiveTabId(newTabId);
    setShowNewTabMenu(false);
  };

  const closeTab = (tabId: string) => {
    setTabs((prev) => {
      if (prev.length <= 1) return prev;
      const idx = prev.findIndex((t) => t.id === tabId);
      const next = prev.filter((t) => t.id !== tabId);
      if (activeTabId === tabId) {
        const newIdx = Math.min(idx, next.length - 1);
        setActiveTabId(next[newIdx].id);
      }
      return next;
    });
  };

  return (
    <div className="flex min-h-screen flex-col p-8">
      <div className="mb-6 flex items-start justify-between gap-6">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight text-[var(--foreground)]">
            Console
          </h1>
          <p className="mt-1 text-sm text-[var(--foreground-muted)]">
            Root shell + remote file explorer (SFTP). Keep this behind your dashboard password.
          </p>
        </div>
      </div>

      {/* Tab bar */}
      <div className="flex items-center gap-1 border-b border-[var(--border)] mb-4">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={`group flex items-center gap-2 px-3 py-2 text-sm cursor-pointer border-b-2 transition-colors ${
              activeTabId === tab.id
                ? "border-[var(--accent)] text-[var(--foreground)] bg-[var(--background-secondary)]/50"
                : "border-transparent text-[var(--foreground-muted)] hover:text-[var(--foreground)] hover:bg-[var(--background-tertiary)]/30"
            }`}
            onClick={() => setActiveTabId(tab.id)}
          >
            <span className="text-base">
              {tab.type === "terminal" ? "⌨️" : "📁"}
            </span>
            <span>{tab.title}</span>
            {tabs.length > 1 && (
              <button
                className="ml-1 opacity-0 group-hover:opacity-100 hover:bg-[var(--background-tertiary)] rounded p-0.5 transition-opacity"
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(tab.id);
                }}
              >
                <svg
                  className="w-3 h-3"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M6 18L18 6M6 6l12 12"
                  />
                </svg>
              </button>
            )}
          </div>
        ))}

        {/* Add tab button */}
        <div className="relative">
          <button
            className="flex items-center justify-center w-8 h-8 text-[var(--foreground-muted)] hover:text-[var(--foreground)] hover:bg-[var(--background-tertiary)]/30 rounded transition-colors"
            onClick={() => setShowNewTabMenu(!showNewTabMenu)}
          >
            <svg
              className="w-4 h-4"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M12 4v16m8-8H4"
              />
            </svg>
          </button>

          {showNewTabMenu && (
            <>
              <div
                className="fixed inset-0 z-10"
                onClick={() => setShowNewTabMenu(false)}
              />
              <div className="absolute left-0 top-full mt-1 z-20 rounded-md border border-[var(--border)] bg-[var(--background-secondary)] shadow-lg py-1 min-w-[140px]">
                <button
                  className="w-full px-3 py-2 text-left text-sm text-[var(--foreground)] hover:bg-[var(--background-tertiary)] flex items-center gap-2"
                  onClick={() => addTab("terminal")}
                >
                  <span>⌨️</span> New Terminal
                </button>
                <button
                  className="w-full px-3 py-2 text-left text-sm text-[var(--foreground)] hover:bg-[var(--background-tertiary)] flex items-center gap-2"
                  onClick={() => addTab("files")}
                >
                  <span>📁</span> New Files
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      {/* Tab content */}
      <div className="relative flex-1 min-h-0 panel rounded-lg border border-[var(--border)] bg-[var(--background-secondary)]/70 p-0 backdrop-blur-xl overflow-hidden">
        {tabs.map((tab) =>
          tab.type === "terminal" ? (
            <TerminalTab
              key={tab.id}
              tabId={tab.id}
              isActive={activeTabId === tab.id}
            />
          ) : (
            <FilesTab
              key={tab.id}
              tabId={tab.id}
              isActive={activeTabId === tab.id}
            />
          )
        )}
      </div>
    </div>
  );
}
