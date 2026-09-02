# trmnl-console-relay

An always-on HTTP service that accepts a TRMNL webhook-shaped payload and serves it back to a
TRMNL private plugin configured with the **polling** strategy, instead of that plugin using
TRMNL's own webhook endpoint directly.

## Why

TRMNL's webhook endpoint caps you at 12 pushes/hour (30 on TRMNL+) and 2kb/request (5kb on
TRMNL+) - see <https://docs.trmnl.com/go/private-plugins/webhooks>. If you're pushing from
[`trmnl-console-backend`](../backend/) on a schedule, or just don't want to be bound by that,
point it at this relay instead:

```
trmnl-console-backend (or trmnl-console --url)
    --[webhook payload, unlimited rate]-->  trmnl-console-relay  <--[TRMNL polls on its own schedule]-- TRMNL
```

Nothing changes on the sending side - the relay accepts the exact same
`{"merge_variables": {...}, "merge_strategy": ...}` payload TRMNL's real webhook endpoint
does (see [`../backend/README.md`](../backend/README.md) and the top-level README). TRMNL
instead fetches the current state from the relay whenever it wants (per the plugin's
`refresh_interval`), which isn't subject to the webhook limits above.

## Endpoints

- `POST /` - push a webhook-shaped payload. Requires `Authorization: Bearer <RELAY_TOKEN>`.
  Applies `merge_strategy` (`replace` (default), `deep_merge`, or `stream` with
  `stream_limit`) to the stored state, same semantics as TRMNL's own webhook.
- `GET /` - returns the current stored state as JSON, unwrapped (e.g. `{"data": {...}}`, not
  `{"merge_variables": {"data": {...}}}`) - this is what TRMNL's polling strategy expects:
  top-level response keys become Liquid template variables directly. Restricted to TRMNL's
  published IP ranges unless `RELAY_IP_ALLOWLIST=false`.
- `GET /health` - unauthenticated `200 ok`, for container health checks.

## Quick start

```bash
cp .env.example .env
$EDITOR .env   # set RELAY_TOKEN to a random secret (e.g. `openssl rand -hex 32`)
docker compose up --build -d
```

Then point whatever pushes data at it (see [`../backend/README.md`](../backend/README.md)):

```yaml
# backend.yml
webhook_url: "https://your-relay-host/trmnl-console"
webhook_token: "the RELAY_TOKEN you set above"
```

And configure the plugin recipe for polling (see `plugin/src/settings.yml`):

```yaml
strategy: polling
polling_verb: get
polling_url: "https://your-relay-host/trmnl-console"
```

## Config reference (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `RELAY_BIND` | `0.0.0.0:8080` | Address the HTTP server listens on. |
| `RELAY_TOKEN` | _(none)_ | Bearer token required on `POST /`. Every push is rejected with a startup warning logged if unset - there's no unauthenticated-write mode. |
| `RELAY_STATE_PATH` | `/data/state.json` | Where the current state is persisted, so a restart doesn't lose it before the next push. |
| `RELAY_IP_ALLOWLIST` | `true` | Restrict `GET /` to TRMNL's published IPs (`https://trmnl.com/api/ips`, refreshed periodically). Set `false` only for local testing without network access. |
| `RELAY_IP_REFRESH_HOURS` | `24` | How often the IP allowlist is refreshed. A failed refresh keeps the last known-good list rather than clearing it. |

## Putting it behind Caddy

The relay only listens on plain HTTP and does no TLS itself - put it behind a reverse proxy.
Path-based example, matching this project's own deployment (see the top-level README and
`docker-compose.yml`'s port):

```caddyfile
handle_path /trmnl-console/* {
    reverse_proxy 127.0.0.1:8901
}
```

If your proxy runs on a different host/network than the relay container, use its reachable
address instead of `127.0.0.1` - e.g. Docker's bridge gateway, or the container's published
port on the Docker host's LAN IP, same as sibling services on that proxy.

## Security notes

- **`RELAY_TOKEN` is the only thing gating writes.** Anything with the token can overwrite
  what your TRMNL device shows next poll. Keep it out of version control (`.env` is
  gitignored) and treat it like any other secret.
- **The IP allowlist is the only thing gating reads**, since TRMNL sends no credentials of
  its own when polling. Don't disable it on a public deployment - if you need to test
  without network access to fetch TRMNL's IP list, disable it temporarily, not permanently.
- State is stored **unencrypted, in plain JSON**, and readable by anything reaching
  `GET /` (from an allowlisted IP) or with filesystem access to the volume. Don't push
  anything you wouldn't want visible to whatever else has access to your device's content.
