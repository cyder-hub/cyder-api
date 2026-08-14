# Ollama through OpenAI-Compatible Source

R3.20 retires Cyder's unfinished native Ollama wire implementation while
keeping a documented connection recipe for Ollama deployments that expose the
OpenAI-compatible API. The recipe uses the existing `OPENAI_COMPATIBLE` Source
contract. It does not create an Ollama Profile, a native transport, or a
special no-credential mode.

## Current boundary

Cyder currently supports four downstream protocol families and four upstream
wire families: OpenAI, Responses, Anthropic, and Gemini. Ollama is a provider
deployment behind an ordinary OpenAI-compatible Source. The supported recipe
proves OpenAI Chat Completions, OpenAI SSE streaming, and OpenAI Embeddings.

This document does not promise:

- Ollama native `/api/chat`, `/api/embed`, `/api/generate`, or `/api/tags` as
  Cyder endpoints;
- NDJSON framing or native Ollama stream/error semantics;
- a complete OpenAI API surface for every Ollama model;
- a Cyder `/v1/responses` compatibility claim for Ollama; or
- Rerank, remote model discovery, or automatic model import.

The official references are [OpenAI compatibility](https://docs.ollama.com/api/openai-compatibility),
[streaming](https://docs.ollama.com/api/streaming),
[errors](https://docs.ollama.com/api/errors), and
[embeddings](https://docs.ollama.com/api/embed). The native streaming and
error behavior described by Ollama is intentionally outside Cyder's current
wire contract.

## Configure a new Source

Use the Provider management page and complete every item below. The ordinary
Provider credential contract still applies: save a non-empty Provider Key.
For a local Ollama deployment, `ollama-local` is a suitable placeholder. Cyder
sends it as `Authorization: Bearer ollama-local`; the local server may ignore
the value, but Cyder does not support an empty credential.

1. Create or select a Provider. If it already has a non-deleted OpenAI-family
   Source (`OPENAI`, `OPENAI_COMPATIBLE`, or `GEMINI_OPENAI`), the Source
   uniqueness contract prevents adding a second OpenAI-family Source to that
   Provider. Use the existing compatible Source where appropriate or create a
   separate Provider. Source Profile is immutable after creation.
2. Create a Source with Profile `OPENAI_COMPATIBLE`.
3. Set `base_url` to the exact OpenAI-compatible API root, normally:
   `http://<ollama-host>:11434/v1`. A reverse proxy prefix belongs in this
   value, for example `https://models.example.test/ollama/v1`.
4. Enable Chat Completions. Leave its path override empty unless the reverse
   proxy deliberately exposes a different relative operation path. The
   default operation path is `chat/completions`.
5. Leave Embeddings disabled unless an embedding model is configured. When it
   is needed, enable Embeddings explicitly; its default operation path is
   `embeddings`.
6. Leave Rerank disabled for this recipe. Do not map a native Ollama endpoint
   to the Rerank operation.
7. Create or replace the ordinary Provider Key with a non-empty value such as
   `ollama-local`. Do not add a no-credential flag or special key type.
8. Configure each Model explicitly:
   - `model_name` is the logical name callers send to Cyder;
   - `real_model_name` is the exact Ollama model name/tag sent upstream, such
     as `llama3.2` or `nomic-embed-text`; and
   - set `ModelKind` to `CHAT` for Chat Completions or `EMBEDDING` for
     Embeddings.
9. Rebuild the model's Source selection. Choose `INHERIT_ALL` when the model
   should use all enabled Sources, or `EXPLICIT` and add the compatible Source
   binding. Recreate a model default when one is needed.
10. Recreate Source-bound Request Patch variants/rules that are still needed.
    The migration removes variants/rules bound to deleted Ollama Sources;
    Patch values are never inferred from a native endpoint configuration.

The operation URL is formed as `base_url + "/" + relative operation path`.
Therefore, `http://host:11434/v1` targets
`http://host:11434/v1/chat/completions` and
`http://host:11434/v1/embeddings`. Do not set the base URL to a complete
operation URL, do not rely on Cyder to add `/v1`, and do not use an absolute
path override. A reverse proxy must preserve the corresponding `/v1/...`
routes; a proxy that exposes only `/api/chat` is not compatible with this
recipe.

## Destructive upgrade boundary

Back up the database before applying the R3.20 migration and keep the backup
as the rollback snapshot. The migration has no `down.sql` and there is no
automatic rollback or Source conversion.

The migration deletes exactly:

- every `upstream_source` with the retired profile, including soft-deleted
  rows;
- `model_source_binding` rows referencing those Sources;
- `request_patch_variant` rows referencing those Sources and their
  `request_patch_rule` rows; and
- all rows from `request_log`,
  `metric_ingested_request_log`, `metric_request_rollup_minute`,
  `metric_http_status_rollup_minute`, and `metric_cost_rollup_minute`.

It retains exactly the other business domains covered by the migration:

- Providers and all Provider Keys;
- Models, including their current selection mode even when a deleted Source
  leaves an `EXPLICIT` model with no bindings;
- non-Ollama Sources;
- downstream API Keys and their daily/monthly governance rollups;
- Cost Catalogs and their versions/components/templates; and
- Manager identity and session data.

The migration does not rewrite a base URL, create a new Source, choose a
default, repair a model binding, or translate a Request Patch. A Provider can
legitimately remain with zero Sources, and an `EXPLICIT` Model can legitimately
remain with zero bindings, but neither state can execute a request. Rebuild
the missing Source, Provider Key, default, binding, and Patch configuration
explicitly, then run Provider Check before serving traffic.

## Manual Cyder smoke

This is an operator check, not an automated release gate. It must go through
Cyder's OpenAI downstream route; calling an Ollama native endpoint directly is
not evidence for the gateway contract.

Set `CYDER_URL` to the server origin and `CYDER_API_KEY` to an issued
downstream API key. With the default `base_path: /ai`:

```bash
curl -sS "$CYDER_URL/ai/openai/v1/chat/completions" \
  -H "Authorization: Bearer $CYDER_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"<chat-logical-model>","messages":[{"role":"user","content":"reply with smoke-ok"}],"stream":false}'
```

Then verify the OpenAI SSE path and normal termination:

```bash
curl -N "$CYDER_URL/ai/openai/v1/chat/completions" \
  -H "Authorization: Bearer $CYDER_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"<chat-logical-model>","messages":[{"role":"user","content":"reply with smoke-ok"}],"stream":true}'
```

For an explicitly enabled embedding model:

```bash
curl -sS "$CYDER_URL/ai/openai/v1/embeddings" \
  -H "Authorization: Bearer $CYDER_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"<embedding-logical-model>","input":"smoke embedding"}'
```

In the Manager Record view, confirm the request reached the configured
Source, the source profile is `OPENAI_COMPATIBLE`, the upstream protocol is
`OPENAI`, the model snapshot uses the configured real model name, and the
placeholder Provider Key is not present in ordinary logs or diagnostics. The
expected upstream paths are `/v1/chat/completions` and `/v1/embeddings`, not a
native `/api/*` path.

## Troubleshooting

- A target URL missing `/v1` usually means the Source `base_url` was set to
  the server origin instead of the OpenAI-compatible API root. Cyder appends
  `chat/completions` or `embeddings`; it does not invent `/v1`.
- A doubled or misplaced reverse-proxy prefix means the prefix was added both
  to `base_url` and to an operation override. Keep `base_url` at the API root
  and keep overrides relative.
- An Embeddings rejection means the Source operation is disabled, the model
  is not configured as `EMBEDDING`, or the model Source binding was not
  rebuilt. Enable the operation and inspect Source selection before checking
  the upstream service.
- A credential failure means the Provider Key is missing, empty, disabled, or
  unavailable to the runtime. Replace it with a non-empty ordinary key and
  rerun Provider Check.
- A request to `/ai/ollama/*`, `/api/chat`, `/api/embed`, `/api/generate`, or
  `/api/tags` is outside the current Cyder contract. Use the OpenAI downstream
  route and the OpenAI-compatible Source instead.
