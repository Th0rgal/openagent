'use client';

import { useState, useEffect } from 'react';
import useSWR from 'swr';
import {
  getSecretsStatus,
  getEncryptionStatus,
  initializeSecrets,
  unlockSecrets,
  lockSecrets,
  listSecrets,
  setSecret,
  deleteSecret,
  revealSecret,
  type SecretsStatus,
  type EncryptionStatus,
  type SecretInfo,
} from '@/lib/api';
import {
  Key,
  Lock,
  Unlock,
  Plus,
  Trash2,
  Eye,
  EyeOff,
  Loader,
  Shield,
  Copy,
  Check,
  FileKey,
  Server,
  CheckCircle,
  AlertCircle,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useToast } from '@/components/toast';

export default function SecretsPage() {
  const { showError } = useToast();

  // Fetch encryption status (skill content encryption)
  const { data: encryptionStatus, isLoading: encryptionLoading } = useSWR(
    'encryption-status',
    getEncryptionStatus,
    { revalidateOnFocus: false }
  );

  // Fetch secrets store status
  const { data: secretsStatus, isLoading: secretsLoading, mutate: mutateSecrets } = useSWR(
    'secrets-status',
    getSecretsStatus,
    { revalidateOnFocus: false }
  );

  // Unlock dialog
  const [showUnlockDialog, setShowUnlockDialog] = useState(false);
  const [passphrase, setPassphrase] = useState('');
  const [unlocking, setUnlocking] = useState(false);

  // Initialize dialog
  const [showInitDialog, setShowInitDialog] = useState(false);
  const [initializing, setInitializing] = useState(false);

  // Selected registry and secrets
  const [selectedRegistry, setSelectedRegistry] = useState<string | null>(null);
  const [secrets, setSecrets] = useState<SecretInfo[]>([]);
  const [loadingSecrets, setLoadingSecrets] = useState(false);

  // Add secret dialog
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [newSecretRegistry, setNewSecretRegistry] = useState('');
  const [newSecretKey, setNewSecretKey] = useState('');
  const [newSecretValue, setNewSecretValue] = useState('');
  const [newSecretType, setNewSecretType] = useState<string>('generic');
  const [addingSecret, setAddingSecret] = useState(false);

  // Reveal secret
  const [revealedSecrets, setRevealedSecrets] = useState<Record<string, string>>({});
  const [revealingSecret, setRevealingSecret] = useState<string | null>(null);

  // Copy feedback
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  // Load secrets when registry changes
  useEffect(() => {
    if (selectedRegistry && secretsStatus?.can_decrypt) {
      loadSecrets(selectedRegistry);
    }
  }, [selectedRegistry, secretsStatus?.can_decrypt]);

  // Auto-select first registry
  useEffect(() => {
    if (secretsStatus?.registries.length && !selectedRegistry) {
      setSelectedRegistry(secretsStatus.registries[0].name);
    }
  }, [secretsStatus?.registries, selectedRegistry]);

  // Handle ESC key to close modals
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (showInitDialog) setShowInitDialog(false);
        if (showUnlockDialog) setShowUnlockDialog(false);
        if (showAddDialog) setShowAddDialog(false);
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [showInitDialog, showUnlockDialog, showAddDialog]);

  const loadSecrets = async (registry: string) => {
    try {
      setLoadingSecrets(true);
      const s = await listSecrets(registry);
      setSecrets(s);
      setRevealedSecrets({});
    } catch (err) {
      console.error('Failed to load secrets:', err);
      setSecrets([]);
    } finally {
      setLoadingSecrets(false);
    }
  };

  const handleInitialize = async () => {
    try {
      setInitializing(true);
      const result = await initializeSecrets('default');
      setShowInitDialog(false);
      await mutateSecrets();
      alert(result.message);
    } catch (err) {
      showError(err instanceof Error ? err.message : 'Failed to initialize');
    } finally {
      setInitializing(false);
    }
  };

  const handleUnlock = async () => {
    try {
      setUnlocking(true);
      await unlockSecrets(passphrase);
      setShowUnlockDialog(false);
      setPassphrase('');
      await mutateSecrets();
    } catch (err) {
      showError(err instanceof Error ? err.message : 'Invalid passphrase');
    } finally {
      setUnlocking(false);
    }
  };

  const handleLock = async () => {
    try {
      await lockSecrets();
      setRevealedSecrets({});
      await mutateSecrets();
    } catch (err) {
      showError(err instanceof Error ? err.message : 'Failed to lock');
    }
  };

  const handleAddSecret = async () => {
    if (!newSecretKey.trim() || !newSecretValue.trim() || !newSecretRegistry.trim()) return;
    try {
      setAddingSecret(true);
      await setSecret(newSecretRegistry, newSecretKey, newSecretValue, {
        type: newSecretType as 'api_key' | 'password' | 'generic',
      });
      setShowAddDialog(false);
      setNewSecretKey('');
      setNewSecretValue('');
      setNewSecretRegistry('');
      setNewSecretType('generic');
      await mutateSecrets();
      if (selectedRegistry === newSecretRegistry) {
        await loadSecrets(selectedRegistry);
      } else {
        setSelectedRegistry(newSecretRegistry);
      }
    } catch (err) {
      showError(err instanceof Error ? err.message : 'Failed to add secret');
    } finally {
      setAddingSecret(false);
    }
  };

  const handleDeleteSecret = async (registry: string, key: string) => {
    if (!confirm(`Delete secret "${key}"?`)) return;
    try {
      await deleteSecret(registry, key);
      await mutateSecrets();
      if (selectedRegistry === registry) {
        await loadSecrets(registry);
      }
    } catch (err) {
      showError(err instanceof Error ? err.message : 'Failed to delete secret');
    }
  };

  const handleReveal = async (registry: string, key: string) => {
    const fullKey = `${registry}/${key}`;
    if (revealedSecrets[fullKey]) {
      setRevealedSecrets((prev) => {
        const next = { ...prev };
        delete next[fullKey];
        return next;
      });
      return;
    }

    try {
      setRevealingSecret(fullKey);
      const value = await revealSecret(registry, key);
      setRevealedSecrets((prev) => ({ ...prev, [fullKey]: value }));
    } catch (err) {
      showError(err instanceof Error ? err.message : 'Failed to reveal secret');
    } finally {
      setRevealingSecret(null);
    }
  };

  const handleCopy = async (registry: string, key: string) => {
    const fullKey = `${registry}/${key}`;
    try {
      let value = revealedSecrets[fullKey];
      if (!value) {
        value = await revealSecret(registry, key);
      }
      await navigator.clipboard.writeText(value);
      setCopiedKey(fullKey);
      setTimeout(() => setCopiedKey(null), 2000);
    } catch (err) {
      showError(err instanceof Error ? err.message : 'Failed to copy');
    }
  };

  const formatSecretType = (type: string | null) => {
    if (!type) return 'Generic';
    return type.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
  };

  const loading = encryptionLoading || secretsLoading;

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[calc(100vh-4rem)]">
        <Loader className="h-8 w-8 animate-spin text-white/40" />
      </div>
    );
  }

  const hasSecrets = (secretsStatus?.registries ?? []).some(r => r.secret_count > 0);

  return (
    <div className="flex-1 p-6 overflow-auto">
      <div className="max-w-4xl mx-auto space-y-8">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-semibold text-white mb-2">Encryption & Secrets</h1>
        <p className="text-white/50">
          Manage encryption for skill content and optional secret storage.
        </p>
      </div>

      {/* Encryption Status Card */}
      <div className="rounded-xl bg-white/[0.02] border border-white/[0.06] p-6">
        <div className="flex items-start gap-4">
          <div className={cn(
            'w-12 h-12 rounded-xl flex items-center justify-center',
            encryptionStatus?.key_available ? 'bg-emerald-500/10' : 'bg-amber-500/10'
          )}>
            <FileKey className={cn(
              'h-6 w-6',
              encryptionStatus?.key_available ? 'text-emerald-400' : 'text-amber-400'
            )} />
          </div>
          <div className="flex-1">
            <div className="flex items-center gap-2 mb-1">
              <h2 className="text-lg font-medium text-white">Skill Content Encryption</h2>
              {encryptionStatus?.key_available ? (
                <span className="flex items-center gap-1 text-xs text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full">
                  <CheckCircle className="h-3 w-3" />
                  Active
                </span>
              ) : (
                <span className="flex items-center gap-1 text-xs text-amber-400 bg-amber-500/10 px-2 py-0.5 rounded-full">
                  <AlertCircle className="h-3 w-3" />
                  Not Configured
                </span>
              )}
            </div>
            <p className="text-sm text-white/50 mb-3">
              Encrypts <code className="text-xs bg-white/[0.06] px-1 py-0.5 rounded">&lt;encrypted&gt;...&lt;/encrypted&gt;</code> tags in skill markdown files.
            </p>
            {encryptionStatus?.key_available ? (
              <div className="flex items-center gap-4 text-sm">
                <div className="flex items-center gap-2 text-white/60">
                  {encryptionStatus.key_source === 'environment' ? (
                    <>
                      <Server className="h-4 w-4" />
                      <span>Key from environment variable</span>
                    </>
                  ) : (
                    <>
                      <FileKey className="h-4 w-4" />
                      <span>Key from file</span>
                    </>
                  )}
                </div>
                {encryptionStatus.key_file_path && encryptionStatus.key_source === 'file' && (
                  <code className="text-xs text-white/40 bg-white/[0.04] px-2 py-1 rounded">
                    {encryptionStatus.key_file_path}
                  </code>
                )}
              </div>
            ) : (
              <p className="text-sm text-white/40">
                Set <code className="text-xs bg-white/[0.06] px-1 py-0.5 rounded">PRIVATE_KEY</code> environment variable or the key will be auto-generated on first use.
              </p>
            )}
          </div>
        </div>
      </div>

      {/* Secrets Store Section */}
      <div className="rounded-xl bg-white/[0.02] border border-white/[0.06] overflow-hidden">
        <div className="p-4 border-b border-white/[0.06] flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-white/[0.04] flex items-center justify-center">
              <Shield className="h-5 w-5 text-white/60" />
            </div>
            <div>
              <h2 className="text-base font-medium text-white">Secrets Store</h2>
              <p className="text-xs text-white/40">Optional key-value storage for credentials</p>
            </div>
          </div>
          {secretsStatus?.initialized && (
            <div className="flex items-center gap-2">
              {secretsStatus.can_decrypt ? (
                <button
                  onClick={handleLock}
                  className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-amber-400 bg-amber-500/10 hover:bg-amber-500/20 rounded-lg transition-colors"
                >
                  <Lock className="h-3.5 w-3.5" />
                  Lock
                </button>
              ) : (
                <button
                  onClick={() => setShowUnlockDialog(true)}
                  className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-emerald-400 bg-emerald-500/10 hover:bg-emerald-500/20 rounded-lg transition-colors"
                >
                  <Unlock className="h-3.5 w-3.5" />
                  Unlock
                </button>
              )}
              <button
                onClick={() => {
                  setNewSecretRegistry(selectedRegistry || 'mcp-tokens');
                  setShowAddDialog(true);
                }}
                disabled={!secretsStatus.can_decrypt}
                className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-white bg-indigo-500 hover:bg-indigo-600 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Plus className="h-3.5 w-3.5" />
                Add Secret
              </button>
            </div>
          )}
        </div>

        {!secretsStatus?.initialized ? (
          <div className="p-8 text-center">
            <p className="text-sm text-white/50 mb-4">
              The secrets store is not initialized. This is optional and separate from skill encryption.
            </p>
            <button
              onClick={() => setShowInitDialog(true)}
              className="px-4 py-2 text-sm font-medium text-white/70 border border-white/[0.08] hover:bg-white/[0.04] rounded-lg transition-colors"
            >
              Initialize Secrets Store
            </button>
          </div>
        ) : !secretsStatus.can_decrypt ? (
          <div className="p-8 text-center">
            <Lock className="h-8 w-8 text-white/20 mx-auto mb-3" />
            <p className="text-sm text-white/50 mb-4">
              Secrets store is locked. Enter passphrase to access.
            </p>
            <button
              onClick={() => setShowUnlockDialog(true)}
              className="px-4 py-2 text-sm font-medium text-emerald-400 bg-emerald-500/10 hover:bg-emerald-500/20 rounded-lg transition-colors"
            >
              Unlock
            </button>
          </div>
        ) : !hasSecrets ? (
          <div className="p-8 text-center">
            <p className="text-sm text-white/50">
              No secrets stored. Click "Add Secret" to store credentials.
            </p>
          </div>
        ) : (
          <div className="flex">
            {/* Registries sidebar */}
            <div className="w-48 border-r border-white/[0.06] p-2">
              {secretsStatus.registries.map((registry) => (
                <button
                  key={registry.name}
                  onClick={() => setSelectedRegistry(registry.name)}
                  className={cn(
                    'w-full text-left p-2 rounded-lg transition-colors mb-1 text-sm',
                    selectedRegistry === registry.name
                      ? 'bg-white/[0.08] text-white'
                      : 'text-white/60 hover:bg-white/[0.04] hover:text-white'
                  )}
                >
                  <div className="flex items-center gap-2">
                    <Key className="h-3.5 w-3.5" />
                    <span className="truncate">{registry.name}</span>
                  </div>
                  <p className="text-xs text-white/40 mt-0.5 ml-5">
                    {registry.secret_count} secret{registry.secret_count !== 1 ? 's' : ''}
                  </p>
                </button>
              ))}
            </div>

            {/* Secrets list */}
            <div className="flex-1">
              <div className="divide-y divide-white/[0.06]">
                {loadingSecrets ? (
                  <div className="p-8 flex items-center justify-center">
                    <Loader className="h-5 w-5 animate-spin text-white/40" />
                  </div>
                ) : secrets.length === 0 ? (
                  <div className="p-8 text-center text-white/40 text-sm">
                    No secrets in this registry
                  </div>
                ) : (
                  secrets.map((secret) => {
                    const fullKey = `${selectedRegistry}/${secret.key}`;
                    const isRevealed = !!revealedSecrets[fullKey];
                    const isRevealing = revealingSecret === fullKey;
                    const isCopied = copiedKey === fullKey;

                    return (
                      <div key={secret.key} className="p-4 flex items-center gap-4">
                        <div className="flex-1 min-w-0">
                          <p className="text-sm font-medium text-white truncate">{secret.key}</p>
                          <div className="flex items-center gap-2 mt-1">
                            <span
                              className={cn(
                                'text-xs px-2 py-0.5 rounded',
                                secret.secret_type === 'api_key'
                                  ? 'bg-blue-500/10 text-blue-400'
                                  : secret.secret_type === 'oauth_access_token'
                                    ? 'bg-green-500/10 text-green-400'
                                    : secret.secret_type === 'password'
                                      ? 'bg-red-500/10 text-red-400'
                                      : 'bg-white/[0.06] text-white/50'
                              )}
                            >
                              {formatSecretType(secret.secret_type)}
                            </span>
                            {secret.is_expired && (
                              <span className="text-xs px-2 py-0.5 rounded bg-red-500/10 text-red-400">
                                Expired
                              </span>
                            )}
                          </div>
                          {isRevealed && (
                            <div className="mt-2 p-2 rounded bg-black/40 font-mono text-xs text-white/80 break-all">
                              {revealedSecrets[fullKey]}
                            </div>
                          )}
                        </div>
                        <div className="flex items-center gap-1">
                          <button
                            onClick={() => handleReveal(selectedRegistry!, secret.key)}
                            disabled={isRevealing}
                            className="p-2 rounded-lg text-white/40 hover:text-white hover:bg-white/[0.06] transition-colors"
                            title={isRevealed ? 'Hide' : 'Reveal'}
                          >
                            {isRevealing ? (
                              <Loader className="h-4 w-4 animate-spin" />
                            ) : isRevealed ? (
                              <EyeOff className="h-4 w-4" />
                            ) : (
                              <Eye className="h-4 w-4" />
                            )}
                          </button>
                          <button
                            onClick={() => handleCopy(selectedRegistry!, secret.key)}
                            className="p-2 rounded-lg text-white/40 hover:text-white hover:bg-white/[0.06] transition-colors"
                            title="Copy"
                          >
                            {isCopied ? (
                              <Check className="h-4 w-4 text-emerald-400" />
                            ) : (
                              <Copy className="h-4 w-4" />
                            )}
                          </button>
                          <button
                            onClick={() => handleDeleteSecret(selectedRegistry!, secret.key)}
                            className="p-2 rounded-lg text-red-400/60 hover:text-red-400 hover:bg-red-500/10 transition-colors"
                            title="Delete"
                          >
                            <Trash2 className="h-4 w-4" />
                          </button>
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </div>
          </div>
        )}
      </div>
      </div>

      {/* Initialize Dialog */}
      {showInitDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-md p-6 rounded-xl bg-[#1a1a1c] border border-white/[0.06]">
            <h3 className="text-lg font-medium text-white mb-4">Initialize Secrets Store</h3>
            <p className="text-sm text-white/60 mb-4">
              This creates a separate encrypted key-value store. After initialization, set{' '}
              <code className="px-1 py-0.5 rounded bg-white/[0.06] text-amber-400">
                OPENAGENT_SECRET_PASSPHRASE
              </code>{' '}
              to enable encryption.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowInitDialog(false)}
                className="px-4 py-2 text-sm text-white/60 hover:text-white"
              >
                Cancel
              </button>
              <button
                onClick={handleInitialize}
                disabled={initializing}
                className="px-4 py-2 text-sm font-medium text-white bg-indigo-500 hover:bg-indigo-600 rounded-lg disabled:opacity-50"
              >
                {initializing ? 'Initializing...' : 'Initialize'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Unlock Dialog */}
      {showUnlockDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-md p-6 rounded-xl bg-[#1a1a1c] border border-white/[0.06]">
            <h3 className="text-lg font-medium text-white mb-4">Unlock Secrets</h3>
            <input
              type="password"
              placeholder="Enter passphrase..."
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleUnlock()}
              className="w-full px-4 py-2 rounded-lg bg-white/[0.04] border border-white/[0.08] text-white placeholder:text-white/30 focus:outline-none focus:border-indigo-500/50 mb-4"
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => {
                  setShowUnlockDialog(false);
                  setPassphrase('');
                }}
                className="px-4 py-2 text-sm text-white/60 hover:text-white"
              >
                Cancel
              </button>
              <button
                onClick={handleUnlock}
                disabled={!passphrase.trim() || unlocking}
                className="px-4 py-2 text-sm font-medium text-white bg-emerald-500 hover:bg-emerald-600 rounded-lg disabled:opacity-50"
              >
                {unlocking ? 'Unlocking...' : 'Unlock'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Add Secret Dialog */}
      {showAddDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-md p-6 rounded-xl bg-[#1a1a1c] border border-white/[0.06]">
            <h3 className="text-lg font-medium text-white mb-4">Add Secret</h3>
            <div className="space-y-4">
              <div>
                <label className="block text-sm text-white/60 mb-1">Registry</label>
                <input
                  type="text"
                  placeholder="e.g., mcp-tokens"
                  value={newSecretRegistry}
                  onChange={(e) => setNewSecretRegistry(e.target.value)}
                  className="w-full px-4 py-2 rounded-lg bg-white/[0.04] border border-white/[0.08] text-white placeholder:text-white/30 focus:outline-none focus:border-indigo-500/50"
                />
              </div>
              <div>
                <label className="block text-sm text-white/60 mb-1">Key</label>
                <input
                  type="text"
                  placeholder="e.g., service/api_key"
                  value={newSecretKey}
                  onChange={(e) => setNewSecretKey(e.target.value)}
                  className="w-full px-4 py-2 rounded-lg bg-white/[0.04] border border-white/[0.08] text-white placeholder:text-white/30 focus:outline-none focus:border-indigo-500/50"
                />
              </div>
              <div>
                <label className="block text-sm text-white/60 mb-1">Value</label>
                <textarea
                  placeholder="Secret value..."
                  value={newSecretValue}
                  onChange={(e) => setNewSecretValue(e.target.value)}
                  rows={3}
                  className="w-full px-4 py-2 rounded-lg bg-white/[0.04] border border-white/[0.08] text-white placeholder:text-white/30 focus:outline-none focus:border-indigo-500/50 resize-none font-mono text-sm"
                />
              </div>
              <div>
                <label className="block text-sm text-white/60 mb-1">Type</label>
                <select
                  value={newSecretType}
                  onChange={(e) => setNewSecretType(e.target.value)}
                  className="w-full px-4 py-2 rounded-lg bg-white/[0.04] border border-white/[0.08] text-white focus:outline-none focus:border-indigo-500/50"
                >
                  <option value="generic">Generic</option>
                  <option value="api_key">API Key</option>
                  <option value="oauth_access_token">OAuth Access Token</option>
                  <option value="oauth_refresh_token">OAuth Refresh Token</option>
                  <option value="password">Password</option>
                </select>
              </div>
            </div>
            <div className="flex justify-end gap-2 mt-6">
              <button
                onClick={() => {
                  setShowAddDialog(false);
                  setNewSecretKey('');
                  setNewSecretValue('');
                  setNewSecretType('generic');
                }}
                className="px-4 py-2 text-sm text-white/60 hover:text-white"
              >
                Cancel
              </button>
              <button
                onClick={handleAddSecret}
                disabled={!newSecretKey.trim() || !newSecretValue.trim() || !newSecretRegistry.trim() || addingSecret}
                className="px-4 py-2 text-sm font-medium text-white bg-indigo-500 hover:bg-indigo-600 rounded-lg disabled:opacity-50"
              >
                {addingSecret ? 'Adding...' : 'Add Secret'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
