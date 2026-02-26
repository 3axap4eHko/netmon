# NetMon: Auth + Cloud Sync + Web Dashboard

## Context

NetMon is a local-first desktop network monitor (Tauri/Rust + React). Users want to optionally sign in to sync monitoring data to the cloud and view charts from a web browser. The app should work fully offline — sign-in unlocks cloud sync. Free tier gets 1h cloud retention, Pro ($3/mo) gets 30 days.

## Architecture Overview

```
Desktop App (Tauri)          Cloudflare Worker API          Turso (Cloud SQLite)
┌─────────────────┐   HTTP   ┌──────────────────┐          ┌────────────────┐
│ Local SQLite    │────────>│ JWT validation   │────────>│ System DB      │
│ AuthManager     │  push    │ Rate limiting    │          │ (users, subs)  │
│ SyncEngine      │  summaries│ OAuth flows     │          ├────────────────┤
│ MtrEngine       │<────────│ Stripe webhooks  │────────>│ Per-user DB    │
└─────────────────┘  response│ Data queries     │          │ (summaries)    │
                             └──────────────────┘          └────────────────┘
                                      ↑
                             ┌────────┴────────┐
                             │ Web Dashboard   │
                             │ (CF Pages)      │
                             │ React + shared  │
                             │ chart components│
                             └─────────────────┘
```

## JWT Structure

```json
{
  "sub": "usr_a1b2c3",
  "email": "user@example.com",
  "plan": "free",
  "max_devices": 1,
  "device_id": "dev_x9y8z7",
  "write_rate": 300,
  "retention_days": 1,
  "exp": 1708786400,
  "iss": "netmon-api"
}
```

- Access token: 24h lifetime, Ed25519 signed
- Refresh token: 30d, opaque, stored hashed in system DB

## Phase 1: Desktop Auth Foundation

### Rust changes

**New deps** in `src-tauri/Cargo.toml`:
```toml
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
sha2 = "0.10"
base64 = "0.22"
rand = "0.9"
uuid = { version = "1", features = ["v4"] }
tauri-plugin-deep-link = "2"
tauri-plugin-shell = "2"
```

**New file: `src-tauri/src/auth.rs`** — `AuthManager` struct:
- Device ID generation (UUID v4, persisted in local DB)
- PKCE code_verifier/code_challenge generation
- `start_oauth(provider)` → returns URL to open in browser
- `handle_callback(code, state)` → exchanges code for JWT via Worker
- `login_email(email, password)` → direct login
- `refresh_token()` → auto-refresh before expiry
- `get_access_token()` → returns valid token, refreshing if needed
- Token storage/retrieval in local SQLite

**New file: `src-tauri/src/sync.rs`** — `SyncEngine` struct:
- Background thread pushes 1-min summaries to `POST /data/push`
- Tracks watermark (`last_push_timestamp`) in local DB
- Respects `write_rate` from JWT claims
- Exponential backoff on failure (5s → 15s → 60s → 300s)
- Auto-stops on logout, auto-starts on login

**New file: `src-tauri/src/cloud_commands.rs`** — Tauri commands:
- `get_auth_state` / `start_oauth` / `login_email` / `register_email`
- `logout` / `get_sync_status` / `get_account_info`

**Modified: `src-tauri/src/db.rs`** — new tables:
```sql
CREATE TABLE IF NOT EXISTS device_info (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    device_id TEXT NOT NULL,
    device_name TEXT NOT NULL,
    platform TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS auth_tokens (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    user_id TEXT NOT NULL, email TEXT NOT NULL, plan TEXT NOT NULL,
    access_token TEXT NOT NULL, refresh_token TEXT NOT NULL, expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_push_timestamp INTEGER NOT NULL DEFAULT 0
);
```
New methods: `get_device_info`, `set_device_info`, `get_auth_tokens`, `set_auth_tokens`, `clear_auth_tokens`, `get_last_push_timestamp`, `set_last_push_timestamp`, `get_unsynced_summaries(since, limit)`

**Modified: `src-tauri/src/main.rs`**:
- Register `tauri_plugin_deep_link` and `tauri_plugin_shell`
- Create `AuthManager` and `SyncEngine`, manage as Tauri state
- Handle `netmon://auth/callback` deep link
- Register new commands in `invoke_handler`

**Modified: `src-tauri/tauri.conf.json`**:
- CSP: add `connect-src https://api.netmon.app`
- Plugins: add deep-link with `netmon://` scheme

### React changes

**New: `src/hooks/useAuth.ts`** — auth state hook
**New: `src/components/LoginModal.tsx`** — Google/Apple/email sign-in
**New: `src/components/AccountPage.tsx`** — plan info, upgrade, sign out
**New: `src/components/SyncIndicator.tsx`** — cloud icon in header
**Modified: `src/components/Dashboard.tsx`** — sign-in button + sync indicator in header
**Modified: `src/types.ts`** — add `AuthState`, `SyncStatus`, `AccountInfo`
**Modified: `src/api.ts`** — add auth/sync API functions

## Phase 2: Cloudflare Worker API

**New directory: `worker/`**

```
worker/
  src/
    index.ts              # Hono router entry
    middleware/auth.ts     # JWT validation
    middleware/cors.ts
    routes/auth.ts        # OAuth + email login
    routes/data.ts        # POST /data/push, GET /data/dashboard
    routes/devices.ts     # Device CRUD
    routes/account.ts     # Stripe checkout/portal
    routes/webhooks.ts    # Stripe webhook handler
    lib/jwt.ts            # Ed25519 sign/verify (jose)
    lib/turso.ts          # @libsql/client helpers
    lib/stripe.ts
    lib/oauth/google.ts
    lib/oauth/apple.ts
  wrangler.toml
  package.json
```

**API routes:**

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/auth/google` | - | Initiate Google OAuth PKCE |
| GET | `/auth/google/callback` | - | Google callback → auth code |
| POST | `/auth/login` | - | Email/password → tokens |
| POST | `/auth/register` | - | Create account |
| POST | `/auth/token` | - | Exchange auth code + PKCE for JWT |
| POST | `/auth/refresh` | refresh | Refresh access token |
| POST | `/data/push` | JWT | Push summaries (validates device, rate, batch size) |
| GET | `/data/dashboard` | JWT/cookie | Query dashboard data for web |
| GET | `/data/targets` | JWT/cookie | Get user's targets |
| GET | `/devices` | JWT | List devices |
| DELETE | `/devices/:id` | JWT | Remove device |
| POST | `/account/subscribe` | JWT | Create Stripe checkout |
| POST | `/account/portal` | JWT | Stripe billing portal |
| POST | `/webhooks/stripe` | sig | Stripe event handler |

**Turso system DB schema:**
- `users` (id, email, password_hash, plan, stripe_customer_id, turso_db_name)
- `oauth_identities` (user_id, provider, provider_user_id)
- `devices` (id, user_id, name, platform, last_push_at)
- `refresh_tokens` (user_id, device_id, token_hash, expires_at)
- `auth_codes` (code, user_id, code_challenge, device_id, expires_at)
- `subscriptions` (user_id, stripe_subscription_id, status, current_period_end)

**Per-user Turso DB** (`netmon-data-{user_id}`): same schema as local `ping_summaries` + `ping_summaries_hourly` + `targets`, with added `device_id` column on summaries.

## Phase 3: Web Dashboard

**New directory: `web/`** — Vite + React, deployed to Cloudflare Pages

- Reuses chart components from `src/components/` via shared directory or TS path aliases
- `web/src/api.ts` uses `fetch()` with cookies instead of Tauri invoke
- Same `Dashboard`, `HopTable`, `LatencyChart`, `LossChart`, `TimeSelector` components
- Auth via cookie-based sessions (Worker sets HttpOnly cookie)

**Repo structure:**
```
netmon-rs/
  src/              # Desktop frontend (unchanged)
  src-tauri/        # Desktop backend (modified)
  web/              # Web dashboard (new)
  worker/           # Cloudflare Worker (new)
  shared/           # Shared types + chart components (extracted)
  package.json      # Workspaces: [".", "web", "worker"]
```

## Phase 4: Stripe Billing

- Product: "NetMon Pro", $3/mo
- `POST /account/subscribe` creates Stripe Checkout Session
- `POST /webhooks/stripe` handles: `checkout.session.completed`, `invoice.paid`, `customer.subscription.updated`, `customer.subscription.deleted`
- Plan changes update `users.plan` in system DB; next JWT refresh picks up new claims

## Phase 5: Retention & Cleanup

- Cloudflare Cron Trigger (daily): prune per-user DBs based on plan retention, aggregate old summaries to hourly, deregister stale devices (90d inactive)

## Key Decisions

1. **`reqwest::blocking` in sync thread** — matches existing `std::thread::spawn` pattern, avoids async refactor
2. **Database-per-user on Turso** — perfect tenant isolation, same SQLite schema, no RLS needed
3. **Push only 1-min summaries** — raw pings stay local, ~100x less cloud data
4. **Cookie auth for web, Bearer for desktop** — standard for each platform
5. **Deep link (`netmon://`) for OAuth callback** — Tauri plugin handles URI scheme registration

## Verification

1. Build desktop app: `npx tauri build` — no compile errors
2. Auth flow: click Sign In → browser opens → complete OAuth → app receives token → `get_auth_state` returns user info
3. Sync: sign in → wait for sync interval → check Turso DB has summaries
4. Web: visit `app.netmon.app` → sign in → see same charts as desktop
5. Billing: click Upgrade → complete Stripe checkout → plan changes to pro → retention extends
6. Rate limiting: push data faster than `write_rate` → receive 429
7. Device limit: sign in on 2nd device with free plan → receive 403
