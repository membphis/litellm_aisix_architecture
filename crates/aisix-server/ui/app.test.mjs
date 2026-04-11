import test from 'node:test';
import assert from 'node:assert/strict';

import {
  adminKeyStorageMode,
  buildDeleteImpact,
  buildResourcePayload,
  deriveRelationshipModel,
  nextAdminUiMode,
  nextAdminRefreshState,
  nextDetailMode,
  maskApiKey,
  validateAdminKey,
} from './app.mjs';

test('buildResourcePayload normalizes provider form fields', () => {
  const payload = buildResourcePayload('providers', {
    id: 'openai',
    kind: 'openai',
    base_url: 'https://api.openai.com',
    secret_ref: 'env:OPENAI_API_KEY',
    policy_id: '',
    rate_limit_rpm: '60',
    rate_limit_tpm: '',
    rate_limit_concurrency: '2',
    cache_mode: 'enabled',
  });

  assert.deepEqual(payload, {
    id: 'openai',
    kind: 'openai',
    base_url: 'https://api.openai.com',
    auth: { secret_ref: 'env:OPENAI_API_KEY' },
    policy_id: null,
    rate_limit: {
      rpm: 60,
      tpm: null,
      concurrency: 2,
    },
    cache: { mode: 'enabled' },
  });
});

test('deriveRelationshipModel marks missing dependencies and reverse references', () => {
  const data = {
    providers: [],
    models: [
      {
        id: 'gpt-4o-mini',
        provider_id: 'missing-provider',
        upstream_model: 'gpt-4o-mini',
        policy_id: 'standard',
        rate_limit: null,
        cache: null,
      },
    ],
    apikeys: [
      {
        id: 'demo',
        key: 'sk-demo-secret',
        allowed_models: ['gpt-4o-mini'],
        policy_id: null,
        rate_limit: null,
      },
    ],
    policies: [
      {
        id: 'standard',
        rate_limit: { rpm: 60, tpm: null, concurrency: null },
      },
    ],
  };

  const derived = deriveRelationshipModel(data);

  assert.equal(derived.models.byId['gpt-4o-mini'].status.kind, 'missing_dependency');
  assert.equal(derived.models.byId['gpt-4o-mini'].dependsOn[0].id, 'missing-provider');
  assert.equal(derived.policies.byId.standard.referencedBy[0].id, 'gpt-4o-mini');
  assert.equal(derived.apikeys.byId.demo.dependsOn[0].id, 'gpt-4o-mini');
});

test('deriveRelationshipModel propagates invalid model runtime state to dependent apikey', () => {
  const data = {
    providers: [],
    models: [
      {
        id: 'gpt-4o-mini',
        provider_id: 'missing-provider',
        upstream_model: 'gpt-4o-mini',
        policy_id: null,
        rate_limit: null,
        cache: null,
      },
    ],
    apikeys: [
      {
        id: 'demo',
        key: 'sk-demo-secret',
        allowed_models: ['gpt-4o-mini'],
        policy_id: null,
        rate_limit: null,
      },
    ],
    policies: [],
  };

  const derived = deriveRelationshipModel(data);

  assert.equal(derived.models.byId['gpt-4o-mini'].status.kind, 'missing_dependency');
  assert.equal(derived.apikeys.byId.demo.status.kind, 'missing_dependency');
  assert.match(derived.apikeys.byId.demo.status.message, /currently excluded from runtime/i);
});

test('maskApiKey keeps short secrets hidden and long secrets partially visible', () => {
  assert.equal(maskApiKey('abcd'), '****');
  assert.equal(maskApiKey('sk-demo-secret'), 'sk-d...cret');
});

test('buildDeleteImpact lists dependent resources before delete', () => {
  const data = {
    providers: [{ id: 'openai', kind: 'openai', base_url: 'https://api.openai.com', auth: { secret_ref: 'env:OPENAI_API_KEY' }, policy_id: null, rate_limit: null, cache: null }],
    models: [{ id: 'gpt-4o-mini', provider_id: 'openai', upstream_model: 'gpt-4o-mini', policy_id: null, rate_limit: null, cache: null }],
    apikeys: [],
    policies: [],
  };

  const impact = buildDeleteImpact('providers', 'openai', data);

  assert.equal(impact.title, "Delete providers 'openai'");
  assert.deepEqual(impact.lines, ['Models: gpt-4o-mini']);
});

test('nextAdminRefreshState waits for admin key before loading', () => {
  assert.deepEqual(nextAdminRefreshState(''), { shouldRefresh: false, connectionState: 'idle' });
  assert.deepEqual(nextAdminRefreshState('test-admin-key'), { shouldRefresh: true, connectionState: 'loading' });
});

test('admin key persistence stays session-scoped', () => {
  assert.equal(adminKeyStorageMode(), 'session');
});

test('admin lock state requires successful validation before entering listing mode', () => {
  assert.deepEqual(nextAdminUiMode({ adminKey: '', adminKeyValid: false, draftMode: null }), {
    locked: true,
    mode: 'locked',
  });

  assert.deepEqual(nextAdminUiMode({ adminKey: 'test-key', adminKeyValid: true, draftMode: null }), {
    locked: false,
    mode: 'listing',
  });
});

test('editor mode only opens for explicit create or edit actions', () => {
  assert.equal(nextDetailMode({ draftMode: null, editingId: null }), 'listing');
  assert.equal(nextDetailMode({ draftMode: 'create', editingId: null }), 'editing');
  assert.equal(nextDetailMode({ draftMode: 'edit', editingId: 'gpt-4o-mini' }), 'editing');
});

test('admin key validation probe targets providers list with x-admin-key header', async () => {
  let captured;
  const fetchImpl = async (url, options) => {
    captured = { url, options };
    return { ok: true, json: async () => [] };
  };

  const result = await validateAdminKey('change-me-admin-key', fetchImpl);

  assert.equal(result.valid, true);
  assert.equal(captured.url, '/admin/providers');
  assert.equal(captured.options.headers['x-admin-key'], 'change-me-admin-key');
});

test('admin key validation keeps modal open on unauthorized response', async () => {
  const result = await validateAdminKey('bad-key', async () => ({
    ok: false,
    status: 401,
    headers: new Headers(),
    text: async () => 'unauthorized',
  }));

  assert.equal(result.valid, false);
  assert.match(result.message, /invalid admin key/i);
});
