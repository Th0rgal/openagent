/**
 * AI Providers API - Provider management and OAuth flows.
 */

import { apiGet, apiPost, apiPut, apiDel, apiFetch } from "./core";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type AIProviderType =
  | "anthropic"
  | "openai"
  | "google"
  | "amazon-bedrock"
  | "azure"
  | "open-router"
  | "mistral"
  | "groq"
  | "xai"
  | "deep-infra"
  | "cerebras"
  | "cohere"
  | "together-ai"
  | "perplexity"
  | "github-copilot"
  | "zai"
  | "custom";

export interface AIProviderTypeInfo {
  id: string;
  name: string;
  uses_oauth: boolean;
  env_var: string | null;
}

export interface AIProviderStatus {
  type: "unknown" | "connected" | "needs_auth" | "error";
  auth_url?: string;
  message?: string;
}

export interface AIProviderAuthMethod {
  label: string;
  type: "oauth" | "api";
  description?: string;
}

/** Custom model definition for custom providers */
export interface CustomModel {
  id: string;
  name?: string;
  context_limit?: number;
  output_limit?: number;
}

export interface AIProvider {
  id: string;
  provider_type: AIProviderType;
  provider_type_name: string;
  name: string;
  google_project_id?: string | null;
  has_api_key: boolean;
  has_oauth: boolean;
  base_url: string | null;
  /** Custom models for custom providers */
  custom_models?: CustomModel[] | null;
  /** Custom environment variable name for API key */
  custom_env_var?: string | null;
  /** NPM package for custom provider */
  npm_package?: string | null;
  enabled: boolean;
  is_default: boolean;
  uses_oauth: boolean;
  auth_methods: AIProviderAuthMethod[];
  status: AIProviderStatus;
  use_for_backends: string[];
  created_at: string;
  updated_at: string;
}

export interface AIProviderAuthResponse {
  success: boolean;
  message: string;
  auth_url: string | null;
}

export interface OAuthAuthorizeResponse {
  url: string;
  instructions: string;
  method: "code" | "auto";
}

export interface BackendProviderResponse {
  configured: boolean;
  provider_type: string | null;
  provider_name: string | null;
  api_key: string | null;
  oauth: {
    access_token: string;
    refresh_token: string;
    expires_at: number;
  } | null;
  has_credentials: boolean;
}

// ---------------------------------------------------------------------------
// Provider Model Types
// ---------------------------------------------------------------------------

export interface ProviderModel {
  id: string;
  name: string;
  description?: string;
}

export interface Provider {
  id: string;
  name: string;
  billing: "subscription" | "pay-per-token";
  description: string;
  models: ProviderModel[];
}

export interface ProvidersResponse {
  providers: Provider[];
}

// ---------------------------------------------------------------------------
// API Functions
// ---------------------------------------------------------------------------

export async function listAIProviders(): Promise<AIProvider[]> {
  return apiGet("/api/ai/providers", "Failed to list AI providers");
}

export async function listAIProviderTypes(): Promise<AIProviderTypeInfo[]> {
  return apiGet("/api/ai/providers/types", "Failed to list AI provider types");
}

export async function getAIProvider(id: string): Promise<AIProvider> {
  return apiGet(`/api/ai/providers/${id}`, "Failed to get AI provider");
}

export async function createAIProvider(data: {
  provider_type: AIProviderType;
  name: string;
  google_project_id?: string;
  api_key?: string;
  base_url?: string;
  enabled?: boolean;
  use_for_backends?: string[];
  /** Custom models for custom providers */
  custom_models?: CustomModel[];
  /** Custom environment variable name for API key */
  custom_env_var?: string;
  /** NPM package for custom provider */
  npm_package?: string;
}): Promise<AIProvider> {
  return apiPost("/api/ai/providers", data, "Failed to create AI provider");
}

export async function updateAIProvider(
  id: string,
  data: {
    name?: string;
    google_project_id?: string | null;
    api_key?: string | null;
    base_url?: string | null;
    enabled?: boolean;
    use_for_backends?: string[];
  }
): Promise<AIProvider> {
  return apiPut(`/api/ai/providers/${id}`, data, "Failed to update AI provider");
}

export async function deleteAIProvider(id: string): Promise<void> {
  return apiDel(`/api/ai/providers/${id}`, "Failed to delete AI provider");
}

export async function getProviderForBackend(backendId: string): Promise<BackendProviderResponse> {
  return apiGet(`/api/ai/providers/for-backend/${backendId}`, "Failed to get provider for backend");
}

export async function authenticateAIProvider(id: string): Promise<AIProviderAuthResponse> {
  return apiPost(`/api/ai/providers/${id}/auth`, undefined, "Failed to authenticate AI provider");
}

export async function setDefaultAIProvider(id: string): Promise<AIProvider> {
  return apiPost(`/api/ai/providers/${id}/default`, undefined, "Failed to set default AI provider");
}

export async function getAuthMethods(id: string): Promise<AIProviderAuthMethod[]> {
  return apiGet(`/api/ai/providers/${id}/auth/methods`, "Failed to get auth methods");
}

export async function oauthAuthorize(id: string, methodIndex: number): Promise<OAuthAuthorizeResponse> {
  const res = await apiFetch(`/api/ai/providers/${id}/oauth/authorize`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ method_index: methodIndex }),
  });
  if (!res.ok) {
    const error = await res.text();
    throw new Error(error || "Failed to start OAuth authorization");
  }
  return res.json();
}

export async function oauthCallback(
  id: string,
  methodIndex: number,
  code: string,
  useForBackends?: string[]
): Promise<AIProvider> {
  const res = await apiFetch(`/api/ai/providers/${id}/oauth/callback`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      method_index: methodIndex,
      code,
      use_for_backends: useForBackends,
    }),
  });
  if (!res.ok) {
    const error = await res.text();
    throw new Error(error || "Failed to complete OAuth");
  }
  return res.json();
}

export async function listProviders(options?: { includeAll?: boolean }): Promise<ProvidersResponse> {
  const params = new URLSearchParams();
  if (options?.includeAll) {
    params.set("include_all", "true");
  }
  const query = params.toString();
  const res = await apiFetch(`/api/providers${query ? `?${query}` : ""}`);
  if (!res.ok) throw new Error("Failed to fetch providers");
  return res.json();
}
