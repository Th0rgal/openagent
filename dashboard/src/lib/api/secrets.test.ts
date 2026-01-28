import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock fetch globally
const mockFetch = vi.fn();
global.fetch = mockFetch;

// Import after mocking
import { getEncryptionStatus, getSecretsStatus } from '../api';

describe('Secrets API', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset API_URL for tests
    process.env.NEXT_PUBLIC_API_URL = 'http://localhost:8111';
  });

  describe('getEncryptionStatus', () => {
    it('returns encryption status when key is available from environment', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          key_available: true,
          key_source: 'environment',
          key_file_path: null,
        }),
      });

      const status = await getEncryptionStatus();

      expect(status.key_available).toBe(true);
      expect(status.key_source).toBe('environment');
      expect(status.key_file_path).toBeNull();
    });

    it('returns encryption status when key is available from file', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          key_available: true,
          key_source: 'file',
          key_file_path: '/root/.openagent/private_key',
        }),
      });

      const status = await getEncryptionStatus();

      expect(status.key_available).toBe(true);
      expect(status.key_source).toBe('file');
      expect(status.key_file_path).toBe('/root/.openagent/private_key');
    });

    it('returns not available when no key exists', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          key_available: false,
          key_source: null,
          key_file_path: '/root/.openagent/private_key',
        }),
      });

      const status = await getEncryptionStatus();

      expect(status.key_available).toBe(false);
      expect(status.key_source).toBeNull();
    });
  });

  describe('getSecretsStatus', () => {
    it('returns not initialized when secrets store is not set up', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          initialized: false,
          can_decrypt: false,
          registries: [],
          default_key: null,
        }),
      });

      const status = await getSecretsStatus();

      expect(status.initialized).toBe(false);
      expect(status.can_decrypt).toBe(false);
      expect(status.registries).toEqual([]);
    });

    it('returns initialized but locked state', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          initialized: true,
          can_decrypt: false,
          registries: [{ name: 'mcp-tokens', description: null, secret_count: 2, updated_at: '2024-01-01' }],
          default_key: 'default',
        }),
      });

      const status = await getSecretsStatus();

      expect(status.initialized).toBe(true);
      expect(status.can_decrypt).toBe(false);
      expect(status.registries).toHaveLength(1);
    });

    it('returns fully unlocked state', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          initialized: true,
          can_decrypt: true,
          registries: [{ name: 'mcp-tokens', description: null, secret_count: 2, updated_at: '2024-01-01' }],
          default_key: 'default',
        }),
      });

      const status = await getSecretsStatus();

      expect(status.initialized).toBe(true);
      expect(status.can_decrypt).toBe(true);
    });
  });
});
