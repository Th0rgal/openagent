/**
 * Skill content encryption utilities.
 *
 * Handles detection and marking of sensitive values in skill markdown content.
 * Values are wrapped in <encrypted v="1">...</encrypted> tags for highlighting
 * and backend encryption.
 */

/** Pattern to match encrypted tags */
export const ENCRYPTED_TAG_REGEX = /<encrypted(?:\s+v="\d+")?>([^<]*)<\/encrypted>/g;

/** Pattern for unversioned encrypted tags (for editing display) */
export const ENCRYPTED_DISPLAY_REGEX = /<encrypted>([^<]*)<\/encrypted>/g;

/** Pattern to match failed-to-decrypt tags */
export const ENCRYPTED_FAILED_TAG_REGEX = /<encrypted-failed(?:\s+v="\d+")?>([^<]*)<\/encrypted-failed>/g;

/** Check if a value looks like an encrypted tag */
export const isEncryptedTag = (value: string): boolean => {
  const trimmed = value.trim();
  return trimmed.startsWith('<encrypted') && trimmed.endsWith('</encrypted>');
};

/** Check if a value is a failed-to-decrypt tag */
export const isEncryptedFailedTag = (value: string): boolean => {
  const trimmed = value.trim();
  return trimmed.startsWith('<encrypted-failed') && trimmed.endsWith('</encrypted-failed>');
};

/** Check if content contains any failed-to-decrypt tags */
export const hasFailedEncryptedTags = (content: string): boolean => {
  return content.includes('<encrypted-failed');
};

/** Extract the value from an encrypted tag */
export const extractEncryptedValue = (tag: string): string | null => {
  const match = tag.match(/<encrypted(?:\s+v="\d+")?>(.*?)<\/encrypted>/);
  return match ? match[1] : null;
};

/** Wrap a value in an encrypted tag for display/editing */
export const wrapEncrypted = (value: string): string => {
  return `<encrypted>${value}</encrypted>`;
};

/**
 * Common patterns for sensitive values that should be encrypted.
 * These are variable-like patterns found in skill markdown that represent
 * actual secrets, not placeholder patterns like ${OPENAI_API_KEY}.
 */
export const SENSITIVE_PATTERNS = [
  // OpenAI
  /sk-[a-zA-Z0-9]{48,}/g,                           // OpenAI API keys
  /sk-proj-[a-zA-Z0-9_-]{48,}/g,                    // OpenAI project API keys

  // Anthropic
  /sk-ant-[a-zA-Z0-9_-]{40,}/g,                     // Anthropic API keys

  // Google
  /AIza[a-zA-Z0-9_-]{35}/g,                         // Google API keys

  // AWS
  /AKIA[A-Z0-9]{16}/g,                              // AWS access key IDs
  /[a-zA-Z0-9/+=]{40}/g,                            // AWS secret keys (40 char base64)

  // GitHub
  /ghp_[a-zA-Z0-9]{36}/g,                           // GitHub personal access tokens
  /gho_[a-zA-Z0-9]{36}/g,                           // GitHub OAuth tokens
  /ghs_[a-zA-Z0-9]{36}/g,                           // GitHub server tokens
  /github_pat_[a-zA-Z0-9_]{22,}/g,                  // GitHub fine-grained PATs

  // Stripe
  /sk_live_[a-zA-Z0-9]{24,}/g,                      // Stripe live secret keys
  /sk_test_[a-zA-Z0-9]{24,}/g,                      // Stripe test secret keys
  /rk_live_[a-zA-Z0-9]{24,}/g,                      // Stripe live restricted keys
  /rk_test_[a-zA-Z0-9]{24,}/g,                      // Stripe test restricted keys

  // Twilio
  /SK[a-f0-9]{32}/g,                                // Twilio API keys

  // Slack
  /xoxb-[0-9]+-[0-9]+-[a-zA-Z0-9]+/g,              // Slack bot tokens
  /xoxp-[0-9]+-[0-9]+-[0-9]+-[a-f0-9]+/g,          // Slack user tokens

  // Discord
  /[MN][a-zA-Z0-9_-]{23,}\.[a-zA-Z0-9_-]{6}\.[a-zA-Z0-9_-]{27,}/g,  // Discord bot tokens

  // Supabase
  /sbp_[a-f0-9]{40}/g,                              // Supabase service role keys

  // Generic patterns (less specific, use with caution)
  /Bearer\s+[a-zA-Z0-9_-]{20,}/g,                   // Bearer tokens
];

/**
 * Variable name patterns that typically contain sensitive values.
 * Used to detect assignments like `OPENAI_API_KEY=sk-...` in markdown.
 */
export const SENSITIVE_VAR_NAMES = [
  'OPENAI_API_KEY',
  'ANTHROPIC_API_KEY',
  'CLAUDE_API_KEY',
  'GOOGLE_API_KEY',
  'GOOGLE_CLOUD_KEY',
  'AWS_ACCESS_KEY_ID',
  'AWS_SECRET_ACCESS_KEY',
  'GITHUB_TOKEN',
  'GITHUB_PAT',
  'GH_TOKEN',
  'STRIPE_SECRET_KEY',
  'STRIPE_API_KEY',
  'TWILIO_AUTH_TOKEN',
  'TWILIO_API_KEY',
  'SLACK_TOKEN',
  'SLACK_BOT_TOKEN',
  'DISCORD_TOKEN',
  'DISCORD_BOT_TOKEN',
  'SUPABASE_SERVICE_KEY',
  'SUPABASE_ANON_KEY',
  'DATABASE_URL',
  'DB_PASSWORD',
  'POSTGRES_PASSWORD',
  'MYSQL_PASSWORD',
  'REDIS_PASSWORD',
  'SECRET_KEY',
  'PRIVATE_KEY',
  'API_KEY',
  'API_SECRET',
  'AUTH_TOKEN',
  'ACCESS_TOKEN',
  'REFRESH_TOKEN',
  'JWT_SECRET',
  'ENCRYPTION_KEY',
  'SIGNING_KEY',
  'WEBHOOK_SECRET',
];

/**
 * Find all sensitive values in content that should be encrypted.
 * Returns matches with their positions for highlighting/replacement.
 */
export interface SensitiveMatch {
  value: string;
  start: number;
  end: number;
  pattern: string;
}

export const findSensitiveValues = (content: string): SensitiveMatch[] => {
  const matches: SensitiveMatch[] = [];

  // Skip values already wrapped in <encrypted> tags
  const alreadyEncrypted = new Set<string>();
  let encMatch;
  const encRegex = new RegExp(ENCRYPTED_TAG_REGEX.source, 'g');
  while ((encMatch = encRegex.exec(content)) !== null) {
    alreadyEncrypted.add(encMatch[1]);
  }

  for (const pattern of SENSITIVE_PATTERNS) {
    // Clone regex to reset lastIndex
    const regex = new RegExp(pattern.source, pattern.flags);
    let match;
    while ((match = regex.exec(content)) !== null) {
      const value = match[0];
      // Skip if already encrypted or too short (false positive)
      if (alreadyEncrypted.has(value) || value.length < 20) continue;

      // Check if this position is inside an <encrypted> tag
      const beforeText = content.slice(0, match.index);
      const lastOpenTag = beforeText.lastIndexOf('<encrypted');
      const lastCloseTag = beforeText.lastIndexOf('</encrypted>');
      if (lastOpenTag > lastCloseTag) continue; // Inside encrypted tag

      matches.push({
        value,
        start: match.index,
        end: match.index + value.length,
        pattern: pattern.source,
      });
    }
  }

  // Deduplicate overlapping matches (keep longer ones)
  matches.sort((a, b) => a.start - b.start || b.end - a.end);
  const deduped: SensitiveMatch[] = [];
  for (const match of matches) {
    const last = deduped[deduped.length - 1];
    if (!last || match.start >= last.end) {
      deduped.push(match);
    }
  }

  return deduped;
};

/**
 * Auto-wrap detected sensitive values in <encrypted> tags.
 * Does not encrypt already-wrapped values.
 */
export const autoWrapSensitiveValues = (content: string): string => {
  const matches = findSensitiveValues(content);
  if (matches.length === 0) return content;

  // Replace from end to start to preserve indices
  let result = content;
  for (let i = matches.length - 1; i >= 0; i--) {
    const match = matches[i];
    result =
      result.slice(0, match.start) +
      wrapEncrypted(match.value) +
      result.slice(match.end);
  }

  return result;
};

/**
 * Count encrypted tags in content.
 */
export const countEncryptedTags = (content: string): number => {
  const regex = new RegExp(ENCRYPTED_TAG_REGEX.source, 'g');
  let count = 0;
  while (regex.exec(content) !== null) count++;
  return count;
};

/**
 * List all encrypted values in content (for display).
 */
export const listEncryptedValues = (content: string): string[] => {
  const values: string[] = [];
  const regex = new RegExp(ENCRYPTED_TAG_REGEX.source, 'g');
  let match;
  while ((match = regex.exec(content)) !== null) {
    values.push(match[1]);
  }
  return values;
};
