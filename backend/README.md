# trmnl-console-backend

A self-hosted stand-in for TRMNL's own private-plugin webhook endpoint.

## Why

TRMNL's real webhook endpoint caps you at 12 pushes/hour (30 on TRMNL+) and 2kb/request (5kb
on TRMNL+) - see <https://docs.trmnl.com/go/private-plugins/webhooks>. If you're pushing on a
schedule (see [`../pusher/`](../pusher/)) or just don't want to be bound by that, run this
instead and push to it - it isn't rate/size-limited, and a **polling**-strategy plugin fetches
the current state from it whenever it wants.

## The contract

This mirrors TRMNL's real webhook endpoint exactly - same URL for both directions, same
payload:

- `POST /<id>` - push. Body: `{"merge_variables": {...}, "merge_strategy": ...}`, exactly
  what `trmnl-console --url` or `trmnl-console-pusher` already send. `merge_strategy` is
  `replace` (default), `deep_merge`, or `stream` (with `stream_limit`) - same semantics as
  TRMNL's own.
- `GET /<id>` - poll. Returns the current merged state as JSON, unwrapped (e.g.
  `{"data": {...}}`) - this is what a plugin's `polling_url` expects: top-level response
  keys become Liquid template variables directly.
- `GET /health` - unauthenticated `200 ok`, for container health checks.

`<id>` is the only credential, same as a real TRMNL webhook URL - anyone who knows it can
push and poll. Pick something unguessable (a UUID: `uuidgen`, or `openssl rand -hex 16`).
Different ids are completely independent, so one deployment can back several plugin
instances/screens at once.

## Quick start

```bash
docker compose up --build -d
```

Then use the *same* URL, `https://your-host/<id>`, in two places:

1. Whatever pushes to it - `trmnl-console --url https://your-host/<id>` or
   [`../pusher/`](../pusher/)'s `webhook_url`.
2. The plugin instance's **Backend URL** field, in the TRMNL dashboard (its settings page,
   the same place a webhook-strategy plugin shows its webhook URL). The recipe
   (`plugin/src/settings.yml`) sets `polling_url: "{{ backend_url }}"` (custom fields are
   referenced bare inside `polling_url`, unlike `custom_fields_values.*` in markup templates)
   and declares that field as `password`-masked - the id-bearing URL is a credential, so it
   never lives in the plugin recipe's source, only in your own TRMNL account.

## Config reference (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `BACKEND_BIND` | `0.0.0.0:8080` | Address the HTTP server listens on. |
| `BACKEND_STATE_DIR` | `/data` | Directory holding one `<id>.json` file per id pushed to, so a restart doesn't lose state before the next push. |

## Putting it behind Caddy

Only listens on plain HTTP, no TLS - put it behind a reverse proxy. Path-based example,
matching this project's own deployment (see the top-level README and
`docker-compose.yml`'s port):

```caddyfile
handle_path /trmnl-console/* {
    reverse_proxy 127.0.0.1:8901
}
```

A request to `/trmnl-console/<id>` then reaches the backend as `/<id>`, matching the
contract above. If your proxy runs on a different host/network than the backend container,
use its reachable address instead of `127.0.0.1`.

## Security notes

- **The id is the only thing gating both push and poll**, exactly like a real TRMNL webhook
  URL. Keep it as secret as you would that URL - anyone who has it can overwrite what your
  device shows next poll.
- State is stored **unencrypted, in plain JSON**, and readable by anything reaching the URL
  or with filesystem access to the volume. Don't push anything you wouldn't want visible to
  whatever else has access to your device's content.
