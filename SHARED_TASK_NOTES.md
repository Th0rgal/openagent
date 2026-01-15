# Encrypted Environment Variables - Task Notes

## Current Status

**Iteration 1 complete**. The encryption utilities are implemented and tested. Next iteration should integrate them into the Library template load/save.

## What's Done

1. **Design document** - Architecture decisions documented below
2. **Crypto module** - `src/template_crypto.rs` with full test coverage (10 tests)
   - `is_encrypted(value)` - detect encrypted format
   - `encrypt_string(key, plaintext)` - encrypt with nonce
   - `decrypt_string(key, value)` - decrypt or passthrough plaintext
   - `load_or_create_private_key(env_path)` - auto-generate key if missing
3. **Dependencies** - Added `hex = "0.4"` to Cargo.toml
4. **Documentation** - Updated `.env.example` with PRIVATE_KEY info

## Next Iteration: Integration

**Goal**: Wire the crypto into `src/library/mod.rs` template functions.

### Files to modify

1. **`src/library/mod.rs`** lines 1164-1220:
   - `get_workspace_template()` - decrypt env_vars after loading JSON
   - `save_workspace_template()` - encrypt env_vars before writing JSON

2. **`src/config.rs`** or startup code:
   - Call `load_or_create_private_key()` at server startup
   - Store key in `Config` or a static/global

### Integration pattern

```rust
// In get_workspace_template() after parsing JSON:
let key = get_template_key()?; // however you access it
let mut decrypted_env = HashMap::new();
for (k, v) in config.env_vars {
    decrypted_env.insert(k, decrypt_string(&key, &v)?);
}

// In save_workspace_template() before serializing:
let key = get_template_key()?;
let mut encrypted_env = HashMap::new();
for (k, v) in template.env_vars {
    if !is_encrypted(&v) {
        encrypted_env.insert(k, encrypt_string(&key, &v)?);
    } else {
        encrypted_env.insert(k, v); // already encrypted
    }
}
```

### Key access pattern options

1. **Config field** - Add `template_key: Option<[u8; 32]>` to `Config`
2. **OnceCell/LazyLock** - Module-level static initialized at startup
3. **Thread-local** - Less preferred

### Testing the integration

1. Create a template with env vars via UI
2. Check stored JSON file - values should be wrapped in `<encrypted v="1">...</encrypted>`
3. Reload template via UI - plaintext should display correctly
4. Legacy test: manually create JSON with plaintext env vars, load it, verify it works

## Design Decisions

### Encrypted value format
```
<encrypted v="1">BASE64(nonce:ciphertext)</encrypted>
```
- XML-like wrapper enables autodetection
- Version attribute for future algorithm changes
- Base64 payload: 12-byte nonce || AES-GCM ciphertext

### Key management
- `PRIVATE_KEY` env var: 64 hex chars (32 bytes)
- Auto-generated at startup if missing, appended to `.env`
- Single key per installation; per-value random nonce

### Backward compatibility
- `decrypt_string()` passes through non-wrapped values unchanged
- Existing plaintext templates continue to work
- Values get encrypted on next save

## Files Changed

- `Cargo.toml` - added `hex = "0.4"`
- `src/lib.rs` - added `pub mod template_crypto`
- `src/template_crypto.rs` - new file, crypto utilities + tests
- `.env.example` - documented PRIVATE_KEY

## How to Test Crypto Module

```bash
cargo test template_crypto
```

All 10 tests should pass:
- `test_is_encrypted`
- `test_encrypt_decrypt_roundtrip`
- `test_plaintext_passthrough`
- `test_prevent_double_encryption`
- `test_wrong_key_fails`
- `test_different_encryptions_different_ciphertext`
- `test_parse_hex_key`
- `test_load_or_create_key_generates_new`
- `test_empty_string_encryption`
- `test_unicode_encryption`
