# Admin API RESTful Support Design

## Background

Current Admin API only supports `PUT /admin/<collection>/:id` for `providers`, `models`, `apikeys`, and `policies`.
This leaves the API in a write-only state and breaks expected REST-style resource management semantics.

The goal of this design is to extend the existing Admin API to support complete resource-oriented management for the current four collections while preserving the existing etcd-backed runtime reload flow.

## Scope

In scope:

- Add single-resource read endpoints for all four admin collections.
- Add collection read endpoints for all four admin collections.
- Add single-resource delete endpoints for all four admin collections.
- Preserve existing `PUT` behavior and auth requirements.
- Keep responses aligned with current lightweight Admin API style.
- Add automated tests for the new HTTP methods and edge cases.

Out of scope:

- Adding `POST` or `PATCH` semantics.
- Introducing pagination, filtering, or field projection.
- Redacting `ApiKeyConfig.key` in read responses.
- Refactoring the Admin API into a generic dynamic router.

## API Surface

Existing endpoints to keep:

- `PUT /admin/providers/:id`
- `PUT /admin/models/:id`
- `PUT /admin/apikeys/:id`
- `PUT /admin/policies/:id`

New collection read endpoints:

- `GET /admin/providers`
- `GET /admin/models`
- `GET /admin/apikeys`
- `GET /admin/policies`

New single-resource endpoints:

- `GET /admin/providers/:id`
- `DELETE /admin/providers/:id`
- `GET /admin/models/:id`
- `DELETE /admin/models/:id`
- `GET /admin/apikeys/:id`
- `DELETE /admin/apikeys/:id`
- `GET /admin/policies/:id`
- `DELETE /admin/policies/:id`

## Response Semantics

### PUT

No semantic change.

- Continue returning `200 OK` with the existing write result payload.
- Continue requiring path id and body id to match.

Write result payload remains:

```json
{
  "id": "openai",
  "path": "/aisix/providers/openai",
  "revision": 123
}
```

### GET Single Resource

- Return `200 OK` and the full stored resource JSON when the resource exists.
- Return `404 Not Found` when the resource does not exist.

`ApiKeyConfig.key` is returned as stored, without masking or redaction.

### GET Collection

- Return `200 OK` and a JSON array of resources.
- Return a bare array instead of an envelope object.
- Sort items by `id` ascending before returning so responses are stable for clients and tests.

### DELETE

- Return `200 OK` and the same write-result-style payload used by `PUT` when a resource is deleted.
- Return `404 Not Found` when the target resource does not exist.

## Error Semantics

- `401 Unauthorized`: missing or invalid `x-admin-key`.
- `400 Bad Request`: path id does not match body id on `PUT`.
- `404 Not Found`: requested resource does not exist for single-resource `GET` or `DELETE`.
- `500 Internal Server Error`: etcd access failure, JSON serialization failure, or JSON deserialization failure.

This keeps the external behavior explicit without introducing custom admin-specific error envelopes beyond the existing gateway error handling.

## Architecture

The implementation should preserve the current structure:

- `crates/aisix-server/src/app.rs` continues to define the Axum routes.
- `crates/aisix-server/src/admin/mod.rs` continues to own shared `AdminState` behavior.
- Each resource file in `crates/aisix-server/src/admin/` continues to own the HTTP handlers for one resource type.

The design intentionally extends the current per-resource modules instead of replacing them with a dynamic collection router. This keeps the code easy to follow and avoids unnecessary type erasure or string-based dispatch.

## Data Access Design

`crates/aisix-config/src/etcd.rs` currently supports:

- `put_json`
- `delete`
- `load_prefix`
- `watch_prefix`

It should be extended with read-oriented helpers:

- `get_json(prefix, collection, id) -> Option<T>`
- `list_json(prefix, collection) -> Vec<T>`

`delete(prefix, collection, id)` should also expose whether the delete actually removed an existing key, so the Admin layer can map missing resources to `404 Not Found` instead of always treating delete as success.

The store layer remains responsible only for etcd I/O and JSON conversion. HTTP status mapping stays in the server/admin layer.

## AdminState Design

`AdminState` should be expanded with symmetric CRUD helpers for each supported resource:

- `get_provider`, `list_providers`, `delete_provider`
- `get_model`, `list_models`, `delete_model`
- `get_apikey`, `list_apikeys`, `delete_apikey`
- `get_policy`, `list_policies`, `delete_policy`

Internally these should be backed by small shared helpers:

- generic `get<T>`
- generic `list<T>`
- generic `delete`

This keeps duplication low while preserving the current explicit resource-specific API at the handler boundary.

## Routing Design

`app.rs` should register both collection and item routes when admin is enabled.

Expected shape:

- `/admin/providers` supports `GET`
- `/admin/providers/:id` supports `GET`, `PUT`, `DELETE`
- `/admin/models` supports `GET`
- `/admin/models/:id` supports `GET`, `PUT`, `DELETE`
- `/admin/apikeys` supports `GET`
- `/admin/apikeys/:id` supports `GET`, `PUT`, `DELETE`
- `/admin/policies` supports `GET`
- `/admin/policies/:id` supports `GET`, `PUT`, `DELETE`

No method support is added when admin is disabled.

## Handler Design

Each resource module should continue following the current pattern:

- authenticate with `require_admin`
- extract path parameters when needed
- deserialize JSON body for `PUT`
- call the matching `AdminState` method
- return `Json(...)` on success

New handlers per resource module:

- `get_<resource>`
- `list_<resource_plural>`
- `delete_<resource>`

Only shared logic that is clearly cross-cutting should live in `admin/mod.rs`.
The design avoids introducing a heavy generic handler abstraction because the current codebase is small and explicit handlers are easier to maintain.

## Sorting and Determinism

Collection reads must sort by `id` ascending after deserialization and before response serialization.

This is required because:

- etcd prefix scans do not provide an API-level contract that is obvious to callers.
- deterministic ordering reduces noisy tests.
- stable output makes the Admin API easier to use manually.

## Security Considerations

- All new `GET` and `DELETE` endpoints must use the same `x-admin-key` authentication as existing `PUT` endpoints.
- `ApiKeyConfig.key` is intentionally returned in plaintext because this was explicitly requested.
- Provider `auth.secret_ref` is already stored as configuration data and should continue to be returned as stored.

The security tradeoff is accepted for this phase in favor of full configuration round-tripping from the Admin API.

## Runtime Behavior

The runtime reload behavior remains unchanged:

- `PUT` and `DELETE` mutate config in etcd.
- the background watcher asynchronously reloads the runtime snapshot.
- a successful admin write/delete response means etcd accepted the mutation, not that the new runtime snapshot is already active.

`GET` reads are direct reads from etcd-backed config state, not reads from the compiled runtime snapshot.

## Testing Strategy

Extend `crates/aisix-server/tests/admin_reload.rs` with coverage for:

- single-resource `GET` success for each collection
- collection `GET` success for each collection
- collection `GET` stable ordering by `id`
- single-resource `GET` returning `404` for missing entries
- `DELETE` success for each collection
- `DELETE` returning `404` for missing entries
- deleted resources no longer present in etcd after success
- auth failures for `GET` and `DELETE`

The tests should continue using the live etcd harness so the behavior matches the real Admin API integration path.

## Implementation Notes

- Prefer the smallest viable extension to the existing code.
- Keep resource-specific modules separate.
- Reuse the current `AdminWriteResult` for successful delete responses.
- Add one focused not-found error helper in the admin layer instead of duplicating error construction.

## Success Criteria

The work is complete when:

- all four admin collections support `PUT`, `GET`, and `DELETE` on item routes
- all four admin collections support `GET` on collection routes
- missing single-resource reads and deletes return `404`
- collection reads return deterministic `id`-sorted arrays
- existing `PUT` behavior continues to work
- tests cover the new methods and pass
