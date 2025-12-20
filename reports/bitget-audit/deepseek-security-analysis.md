# Static Security Analysis Report: web3.bitget.com

## Executive Summary

A comprehensive security analysis of `web3.bitget.com` was conducted, focusing on Content Security Policy (CSP), JavaScript vulnerabilities, and configuration files. The website appears to be a legitimate cryptocurrency wallet service (Bitget Wallet) with reasonable security measures but some notable vulnerabilities.

## 1. Content Security Policy (CSP) Analysis

### CSP Header (Report-Only Mode)
The site uses a **Content-Security-Policy-Report-Only** header, which means violations are reported but not blocked.

### Key CSP Directives:

#### **script-src** (Critical Findings):
- ✅ **Good**: Includes 'self' directive
- ⚠️ **High Risk**: Allows `'unsafe-eval'` and `'unsafe-inline'` 
- ⚠️ **Moderate Risk**: Allows `blob:` and `data:` protocols
- ⚠️ **Moderate Risk**: Extensive allowlist of 60+ external domains including:
  - `*.youtube.com`
  - `firebase.googleapis.com`
  - `*.bitkeep.fun`
  - `*.bitget.cloud`
  - `keepshare.xyz`
  - `gasutopia.com`
  - `bitkeep.com`
  - `*.facebook.net`
  - `api.nileex.io`
  - `keepshare.info`
  - `*.google.com`
  - `share.bitkeep.shop`
  - `*.bitkeep.com`
  - `ta.bitkeep.buzz:8993` (non-standard port)
  - `bitkeep.io`
  - `*.bitkeep.io`
  - `www.google-analytics.com`
  - `fp-constantid.bitkeep.vip`
  - `*.bitkeep.page`
  - `*.bjxnyj.com`
  - `bitkeep.org`
  - `*.bitgetstatic.com`
  - `share.bwb.live`
  - `*.bitkeep.vip`
  - `*.bitget.site`
  - `*.bitgetpro.site`
  - `api.shasta.trongrid.io`
  - `s3.infcrypto.com`
  - `*.bitkeep.me`
  - `*.jjdsn.vip`
  - `*.mytokenpocket.vip`
  - `sun.tronex.io`
  - `goldshare.me`
  - `*.bitget.com`
  - `firebaseinstallations.googleapis.com`
  - `www.googletagmanager.com`
  - `*.googleapis.com`
  - `share.bwb.site`
  - `stats.g.doubleclick.net`
  - `rpc-wallet.broearn.com`
  - `api.trongrid.io`
  - `*.bknode.vip`
  - `cdn.bootcdn.net`
  - `search.imtt.qq.com`
  - `api-web.wwmxd.info`
  - `api-web.wwmxd.site`
  - `www.recaptcha.net`
  - `ordinals.com`
  - `www.gstatic.cn`
  - `www.gstatic.com`
  - `log.noxiaohao.com`
  - `*.geetest.com`
  - `*.geevisit.com`
  - `*.gsensebot.com`
  - `share.bwb.online`
  - `share.bwb.global`
  - `share.bwb.win`
  - `share.bwb.inc`
  - `share.bwb.space`
  - `*.walletconnect.org`
  - `wss://*.walletconnect.org`
  - `https://*.walletconnect.com`

#### **connect-src**: Similar extensive allowlist with additional blockchain RPC endpoints

#### **frame-src**: Allows framing from multiple domains

#### **frame-ancestors**: Allows embedding from specific domains

#### **report-uri**: `https://log.noxiaohao.com/v1/buried/log/cspSecurity`

### CSP Security Issues:
1. **`unsafe-eval` and `unsafe-inline`**: These directives significantly reduce CSP effectiveness against XSS attacks.
2. **Extensive Domain Allowlist**: Overly permissive policy increases attack surface.
3. **Mixed Content**: Allows both HTTP and HTTPS sources (though most are HTTPS).
4. **Non-standard Ports**: Allows `ta.bitkeep.buzz:8993` (non-standard port).
5. **Report-Only Mode**: Policy violations are only reported, not blocked.

## 2. HTTP Security Headers

### Present Headers:
- ✅ `Strict-Transport-Security: max-age=15768000;includeSubDomains;preload`
  - Good: Enforces HTTPS with long max-age and includes subdomains
- ✅ `X-Content-Type-Options: nosniff`
  - Good: Prevents MIME type sniffing
- ✅ `Referrer-Policy: unsafe-url`
  - Moderate: Sends full URL in Referer header (privacy concern)

### Missing Headers:
- ❌ **X-Frame-Options**: Not present (CSP frame-ancestors provides some protection)
- ❌ **X-XSS-Protection**: Not present (modern browsers ignore this in favor of CSP)
- ❌ **Permissions-Policy**: Not present (could restrict sensitive browser features)
- ❌ **Cache-Control**: Present but `no-cache` (could be more restrictive for sensitive content)

## 3. JavaScript Analysis

### Script Sources:
1. **Google Analytics**: `https://www.googletagmanager.com/gtag/js?id=G-BW4GVE68H3`
2. **Multiple Static Assets**: From `static-web.jjdsn.vip` (20+ JavaScript files)
3. **Inline JavaScript**: Large `window.__NUXT__` object with application state

### JavaScript Vulnerabilities:
1. **`unsafe-eval` Usage**: The CSP allows `unsafe-eval`, suggesting eval() or similar functions may be used.
2. **Inline Scripts**: Despite CSP allowing `unsafe-inline`, most scripts are external.
3. **Base64 Data URLs**: Multiple images embedded as base64 data URLs.

### Third-Party Dependencies:
- Google Analytics
- Firebase services
- Multiple Chinese CDNs (`jjdsn.vip`, `bitkeep.vip`)
- Various blockchain/crypto services
- reCAPTCHA
- WalletConnect services

## 4. Configuration Files Analysis

### robots.txt Analysis:
✅ Well-configured robots.txt allowing legitimate crawlers including:
- AI/LLM crawlers (Anthropic, Claude, GPTBot, etc.)
- Search engines (Googlebot, Baiduspider, etc.)
- Disallows sensitive paths (`/dapp/browsinghistory`, `/dapp/collect`, `/dapp/searchresults`)

### Missing/Protected Files:
- `.git/config`: 404 Not Found (good)
- `phpinfo.php`: 404 Not Found (good)
- `README.md`: 404 Not Found (good)
- `package.json`: 404 Not Found (good)
- `.env`: 404 Not Found (good)

## 5. Source Code Analysis

### HTML Structure:
- Modern Vue.js/Nuxt.js application (server-side rendered)
- Uses `data-v-*` attributes for Vue.js scoped styling
- Multiple language support (20+ languages)
- Responsive design with mobile support

### External Resource Domains:
1. **Primary CDN**: `static-web.jjdsn.vip`
2. **Asset CDN**: `cdn.bitkeep.vip`
3. **Analytics**: Google services (Tag Manager, Analytics, DoubleClick)
4. **Blockchain**: Multiple blockchain RPC endpoints
5. **Third-party Services**: Various Chinese domains for tracking/analytics

### Potential Issues:
1. **Multiple Unfamiliar Domains**: Several domains like `*.jjdsn.vip`, `*.bitkeep.*`, `*.bwb.*` raise questions about ownership and security
2. **Mixed Chinese/English Services**: Chinese CDNs mixed with international services
3. **Extensive Third-party Dependencies**: Increases supply chain attack surface

## 6. Security Recommendations

### Critical (High Priority):
1. **Remove `unsafe-eval` from CSP**: Refactor code to avoid eval(), Function(), setTimeout/setInterval with strings
2. **Remove `unsafe-inline` from CSP**: Move inline scripts to external files
3. **Reduce Domain Allowlist**: Restrict to essential domains only
4. **Enable CSP Enforcement**: Switch from `report-only` to enforced mode

### Important (Medium Priority):
5. **Add X-Frame-Options Header**: As additional protection against clickjacking
6. **Add Permissions-Policy Header**: Restrict access to sensitive browser features
7. **Implement Subresource Integrity (SRI)**: Add integrity attributes to external scripts
8. **Audit Third-party Scripts**: Review all external JavaScript for security implications

### Recommended (Low Priority):
9. **Implement Certificate Transparency**: Monitor for fraudulent certificates
10. **Regular Security Audits**: Ongoing monitoring of dependencies
11. **Content Security Policy Testing**: Regular testing with tools like CSP Evaluator
12. **Reduce Cookie Scope**: Review SameSite cookie settings

## 7. Technical Details

### Server Information:
- **Server**: Cloudflare
- **Framework**: Vue.js/Nuxt.js
- **CDN**: Multiple Chinese CDNs
- **Analytics**: Google Analytics, Firebase
- **Blockchain Integration**: Multiple chains (Ethereum, Polygon, BSC, etc.)

### SSL/TLS Configuration:
- HTTPS enforced via HSTS
- Cloudflare-managed SSL
- Includes subdomains in HSTS

## 8. Risk Assessment

### Overall Risk Score: 6/10 (Moderate-High)

**Factors Contributing to Risk:**
1. **CSP Weaknesses**: `unsafe-eval` and `unsafe-inline` directives
2. **Extensive Third-party Dependencies**: Increases attack surface
3. **Mixed Content Sources**: Chinese and international CDNs
4. **Report-Only CSP**: Violations not blocked

**Mitigating Factors:**
1. **HTTPS Enforcement**: Strong HSTS policy
2. **Modern Framework**: Vue.js/Nuxt.js with SSR
3. **Cloudflare Protection**: DDoS and WAF protection likely
4. **No Exposed Config Files**: Common sensitive files return 404

## Conclusion

The website implements several security measures but has significant CSP weaknesses that could expose users to XSS attacks. The extensive allowlist of third-party domains and permissive CSP directives (`unsafe-eval`, `unsafe-inline`) represent the most critical vulnerabilities. 

**Recommendation**: Implement a stricter CSP policy, reduce third-party dependencies, and enable CSP enforcement mode to provide actual protection rather than just reporting.