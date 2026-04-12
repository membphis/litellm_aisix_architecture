import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import * as app from './app.mjs';

import {
  buildPlaygroundRequest,
  classifyPlaygroundFailure,
  executePlaygroundRequest,
  nextPlaygroundFormState,
  resolvePlaygroundApiKey,
  resolvePlaygroundModel,
  adminKeyStorageMode,
  buildRowActions,
  buildDeleteImpact,
  buildJsonAdminHeaders,
  defaultPlaygroundBaseUrl,
  derivePlaygroundHints,
  buildReferenceOptions,
  buildResourcePayload,
  deriveRelationshipModel,
  extractAssistantText,
  finishEditorFlow,
  nextAdminUiMode,
  nextAdminRefreshState,
  nextDetailMode,
  shouldRefreshOnInit,
  restoreAdminKeyValidity,
  maskApiKey,
  renderEditorSummary,
  renderField,
  startCreateAction,
  startEditAction,
  validateAdminKey,
} from './app.mjs';

test('defaultPlaygroundBaseUrl uses data plane default port', () => {
  assert.equal(defaultPlaygroundBaseUrl(), 'http://127.0.0.1:4000');
  assert.equal(defaultPlaygroundBaseUrl('gateway.internal'), 'http://gateway.internal:4000');
  assert.equal(defaultPlaygroundBaseUrl('gateway.internal', 'https'), 'https://gateway.internal:4000');
});

test('nextPlaygroundFormState updates single-field api key and model selections', () => {
  const next = nextPlaygroundFormState({
    baseUrl: 'http://127.0.0.1:4000',
    apiKeySelection: 'saved:demo',
    customApiKey: '',
    modelSelection: 'saved:gpt-4o-mini',
    customModel: '',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
    requestState: 'idle',
    result: null,
    lastRequestPreview: null,
  }, {
    base_url: 'https://gateway.internal:4000',
    api_key_selection: 'custom',
    custom_api_key: 'sk-manual',
    model_selection: 'custom',
    custom_model: 'gpt-4.1-mini',
    system_prompt: 'You are direct.',
    user_message: 'Ping.',
  });

  assert.equal(next.baseUrl, 'https://gateway.internal:4000');
  assert.equal(next.apiKeySelection, 'custom');
  assert.equal(next.customApiKey, 'sk-manual');
  assert.equal(next.modelSelection, 'custom');
  assert.equal(next.customModel, 'gpt-4.1-mini');
});

test('resolvePlaygroundApiKey and resolvePlaygroundModel prefer saved selections unless custom is chosen', () => {
  const data = {
    apikeys: [{ id: 'demo', key: 'sk-demo-secret', allowed_models: ['gpt-4o-mini'] }],
    models: [{ id: 'gpt-4o-mini' }],
  };

  assert.equal(resolvePlaygroundApiKey(data, {
    apiKeySelection: 'saved:demo',
    customApiKey: 'sk-manual',
  }), 'sk-demo-secret');
  assert.equal(resolvePlaygroundApiKey(data, {
    apiKeySelection: 'custom',
    customApiKey: 'sk-manual',
  }), 'sk-manual');
  assert.equal(resolvePlaygroundModel(data, {
    modelSelection: 'saved:gpt-4o-mini',
    customModel: 'gpt-4.1-mini',
  }), 'gpt-4o-mini');
  assert.equal(resolvePlaygroundModel(data, {
    modelSelection: 'custom',
    customModel: 'gpt-4.1-mini',
  }), 'gpt-4.1-mini');
});

test('resolvePlaygroundApiKey and resolvePlaygroundModel still return values when saved resources are missing', () => {
  const data = {
    apikeys: [],
    models: [],
  };

  assert.equal(resolvePlaygroundApiKey(data, {
    apiKeySelection: 'saved:missing-key',
    customApiKey: '',
  }), 'missing-key');
  assert.equal(resolvePlaygroundModel(data, {
    modelSelection: 'saved:missing-model',
    customModel: '',
  }), 'missing-model');
});

test('buildPlaygroundRequest creates non-streaming chat completion request', () => {
  const request = buildPlaygroundRequest({
    baseUrl: 'http://127.0.0.1:4000/',
    apiKey: 'sk-demo-secret',
    model: 'gpt-4o-mini',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
  });

  assert.equal(request.url, 'http://127.0.0.1:4000/v1/chat/completions');
  assert.equal(request.options.method, 'POST');
  assert.equal(request.options.headers.get('authorization'), 'Bearer sk-demo-secret');
  assert.equal(request.options.headers.get('content-type'), 'application/json');
  assert.deepEqual(JSON.parse(request.options.body), {
    model: 'gpt-4o-mini',
    stream: false,
    messages: [
      { role: 'system', content: 'You are concise.' },
      { role: 'user', content: 'Say hello.' },
    ],
  });
});

test('derivePlaygroundHints reports model, api key allowlist, and runtime status', () => {
  const data = {
    providers: [{ id: 'openai', kind: 'openai', base_url: 'https://api.openai.com', auth: { secret_ref: 'env:OPENAI_API_KEY' }, policy_id: null, rate_limit: null, cache: null }],
    models: [{ id: 'gpt-4o-mini', provider_id: 'openai', upstream_model: 'gpt-4o-mini', policy_id: null, rate_limit: null, cache: null }],
    apikeys: [{ id: 'demo', key: 'sk-demo-secret', allowed_models: ['gpt-4o-mini'], policy_id: null, rate_limit: null }],
    policies: [],
  };
  const derived = deriveRelationshipModel(data);

  const hints = derivePlaygroundHints(data, derived, {
    modelSelection: 'saved:gpt-4o-mini',
    customModel: '',
    apiKeySelection: 'saved:demo',
    customApiKey: '',
  });

  assert.equal(hints.modelExists.ok, true);
  assert.equal(hints.apiKeyAllowsModel.ok, true);
  assert.equal(hints.runtimeStatus.kind, 'valid');
});

test('extractAssistantText returns first assistant message text', () => {
  const text = extractAssistantText({
    choices: [
      { message: { role: 'assistant', content: 'Hello from AISIX.' } },
    ],
  });

  assert.equal(text, 'Hello from AISIX.');
});

test('classifyPlaygroundFailure maps response failures and network errors', () => {
  assert.deepEqual(classifyPlaygroundFailure({ status: 401, message: 'bad key' }), {
    category: 'auth_failed',
    title: 'Auth failed',
  });
  assert.deepEqual(classifyPlaygroundFailure({ status: 400, message: 'bad request' }), {
    category: 'invalid_request',
    title: 'Invalid request',
  });
  assert.deepEqual(classifyPlaygroundFailure({ status: 503, message: 'upstream unavailable' }), {
    category: 'upstream_error',
    title: 'Upstream error',
  });
  assert.deepEqual(classifyPlaygroundFailure({ error: new Error('network down') }), {
    category: 'network_error',
    title: 'Network error',
  });
});

test('executePlaygroundRequest returns success payload with latency and assistant text', async () => {
  const result = await executePlaygroundRequest({
    baseUrl: 'http://127.0.0.1:4000',
    apiKey: 'sk-demo-secret',
    model: 'gpt-4o-mini',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
  }, async () => ({
    ok: true,
    status: 200,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: async () => ({
      id: 'chatcmpl-123',
      model: 'gpt-4o-mini',
      usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
      choices: [{ message: { role: 'assistant', content: 'Hello.' } }],
    }),
    text: async () => JSON.stringify({
      id: 'chatcmpl-123',
      model: 'gpt-4o-mini',
      usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
      choices: [{ message: { role: 'assistant', content: 'Hello.' } }],
    }),
  }), () => 1000);

  assert.equal(result.ok, true);
  assert.equal(result.status, 200);
  assert.equal(result.assistantText, 'Hello.');
  assert.equal(result.durationMs, 0);
  assert.equal(result.responseFormat, 'json');
  assert.equal(result.responseBody.id, 'chatcmpl-123');
});

test('executePlaygroundRequest marks failed json responses as json format', async () => {
  const result = await executePlaygroundRequest({
    baseUrl: 'http://127.0.0.1:4000',
    apiKey: 'sk-demo-secret',
    model: 'gpt-4o-mini',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
  }, async () => ({
    ok: false,
    status: 401,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: async () => ({ error: { message: 'bad key' } }),
    text: async () => JSON.stringify({ error: { message: 'bad key' } }),
  }));

  assert.equal(result.ok, false);
  assert.equal(result.responseFormat, 'json');
  assert.deepEqual(result.error, {
    category: 'auth_failed',
    title: 'Auth failed',
  });
});

test('executePlaygroundRequest marks failed text responses as text format', async () => {
  const result = await executePlaygroundRequest({
    baseUrl: 'http://127.0.0.1:4000',
    apiKey: 'sk-demo-secret',
    model: 'gpt-4o-mini',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
  }, async () => ({
    ok: false,
    status: 503,
    headers: new Headers({ 'content-type': 'text/plain' }),
    text: async () => 'upstream unavailable',
  }));

  assert.equal(result.ok, false);
  assert.equal(result.responseFormat, 'text');
  assert.deepEqual(result.error, {
    category: 'upstream_error',
    title: 'Upstream error',
  });
});

test('executePlaygroundRequest preserves raw text when application/json body is invalid json', async () => {
  const result = await executePlaygroundRequest({
    baseUrl: 'http://127.0.0.1:4000',
    apiKey: 'sk-demo-secret',
    model: 'gpt-4o-mini',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
  }, async () => ({
    ok: false,
    status: 401,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: async () => {
      throw new Error('Unexpected token < in JSON');
    },
    text: async () => '<html>bad gateway</html>',
  }));

  assert.equal(result.ok, false);
  assert.equal(result.responseFormat, 'text');
  assert.equal(result.responseBody, '<html>bad gateway</html>');
  assert.deepEqual(result.error, {
    category: 'auth_failed',
    title: 'Auth failed',
  });
});

test('executePlaygroundRequest parses valid application/problem+json responses as json', async () => {
  const result = await executePlaygroundRequest({
    baseUrl: 'http://127.0.0.1:4000',
    apiKey: 'sk-demo-secret',
    model: 'gpt-4o-mini',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
  }, async () => ({
    ok: false,
    status: 422,
    headers: new Headers({ 'content-type': 'application/problem+json' }),
    json: async () => ({
      type: 'https://example.com/problem',
      title: 'Invalid model',
      detail: 'model not allowed',
    }),
    text: async () => JSON.stringify({
      type: 'https://example.com/problem',
      title: 'Invalid model',
      detail: 'model not allowed',
    }),
  }));

  assert.equal(result.ok, false);
  assert.equal(result.responseFormat, 'json');
  assert.deepEqual(result.responseBody, {
    type: 'https://example.com/problem',
    title: 'Invalid model',
    detail: 'model not allowed',
  });
  assert.deepEqual(result.error, {
    category: 'model_rejected',
    title: 'Model rejected',
  });
});

test('executePlaygroundRequest marks network failures as text format', async () => {
  const result = await executePlaygroundRequest({
    baseUrl: 'http://127.0.0.1:4000',
    apiKey: 'sk-demo-secret',
    model: 'gpt-4o-mini',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
  }, async () => {
    throw new Error('network down');
  });

  assert.equal(result.ok, false);
  assert.equal(result.responseFormat, 'text');
  assert.deepEqual(result.error, {
    category: 'network_error',
    title: 'Network error',
  });
});

test('renderPlaygroundResult uses text response title when application/json body is invalid json', async () => {
  const result = await executePlaygroundRequest({
    baseUrl: 'http://127.0.0.1:4000',
    apiKey: 'sk-demo-secret',
    model: 'gpt-4o-mini',
    systemPrompt: 'You are concise.',
    userMessage: 'Say hello.',
  }, async () => ({
    ok: false,
    status: 401,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: async () => {
      throw new Error('Unexpected token < in JSON');
    },
    text: async () => '<html>bad gateway</html>',
  }));

  const html = app.renderPlaygroundResult(result);

  assert.equal(result.ok, false);
  assert.match(html, /<strong>Original Response<\/strong>/);
  assert.doesNotMatch(html, /<strong>Original Response JSON<\/strong>/);
});

test('renderPlaygroundResult labels json failures with original request and json response titles', () => {
  const html = app.renderPlaygroundResult?.({
    ok: false,
    status: 401,
    durationMs: 12,
    error: { category: 'auth_failed', title: 'Auth failed' },
    assistantText: '',
    responseFormat: 'json',
    responseBody: { error: { message: 'bad key' } },
    request: {
      options: {
        body: JSON.stringify({ model: 'gpt-4o-mini', messages: [{ role: 'user', content: 'hello' }] }),
      },
    },
  });

  assert.equal(typeof app.renderPlaygroundResult, 'function');
  assert.match(html, /<strong>Original Request JSON<\/strong>/);
  assert.match(html, /<strong>Original Response JSON<\/strong>/);
  assert.doesNotMatch(html, /<strong>Raw Response<\/strong>/);
});

test('renderPlaygroundResult labels text failures with original request and text response titles', () => {
  const html = app.renderPlaygroundResult?.({
    ok: false,
    status: 503,
    durationMs: 25,
    error: { category: 'upstream_error', title: 'Upstream error' },
    assistantText: '',
    responseFormat: 'text',
    responseBody: 'upstream unavailable',
    request: {
      options: {
        body: JSON.stringify({ model: 'gpt-4o-mini', messages: [{ role: 'user', content: 'hello' }] }),
      },
    },
  });

  assert.equal(typeof app.renderPlaygroundResult, 'function');
  assert.match(html, /<strong>Original Request JSON<\/strong>/);
  assert.match(html, /<strong>Original Response<\/strong>/);
  assert.doesNotMatch(html, /<strong>Original Response JSON<\/strong>/);
});

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

test('stored admin key restores validated state after refresh', () => {
  assert.equal(restoreAdminKeyValidity(''), false);
  assert.equal(restoreAdminKeyValidity('change-me-admin-key'), true);
});

test('restored valid admin key triggers initial refresh', () => {
  assert.equal(shouldRefreshOnInit({ adminKey: '', adminKeyValid: false }), false);
  assert.equal(shouldRefreshOnInit({ adminKey: 'change-me-admin-key', adminKeyValid: true }), true);
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

test('json admin headers keep x-admin-key when adding content-type', () => {
  const headers = buildJsonAdminHeaders('change-me-admin-key');

  assert.equal(headers.get('x-admin-key'), 'change-me-admin-key');
  assert.equal(headers.get('content-type'), 'application/json');
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

test('admin key validation converts rejected fetch into invalid result', async () => {
  const result = await validateAdminKey('change-me-admin-key', async () => {
    throw new Error('network down');
  });

  assert.equal(result.valid, false);
  assert.match(result.message, /network down/i);
});

test('startEditAction opens editor state for an existing resource', () => {
  const next = startEditAction('models', {
    id: 'gpt-4o-mini',
    provider_id: 'openai',
    upstream_model: 'gpt-4o-mini',
    policy_id: null,
    rate_limit: null,
    cache: null,
  });

  assert.equal(next.draftMode, 'edit');
  assert.equal(next.editorCollection, 'models');
  assert.equal(next.editorId, 'gpt-4o-mini');
});

test('startCreateAction opens editor state with default values', () => {
  const next = startCreateAction('providers');

  assert.equal(next.draftMode, 'create');
  assert.equal(next.editorCollection, 'providers');
  assert.equal(next.editorId, null);
  assert.equal(typeof next.editorValues, 'object');
});

test('buildRowActions exposes edit and delete actions only', () => {
  assert.deepEqual(buildRowActions('providers', 'openai'), [
    { kind: 'edit', collection: 'providers', id: 'openai' },
    { kind: 'delete', collection: 'providers', id: 'openai' },
  ]);
});

test('editor summary does not include relations section markers', () => {
  const html = renderEditorSummary('models', {
    id: 'gpt-4o-mini',
    raw: {
      id: 'gpt-4o-mini',
      provider_id: 'openai',
      upstream_model: 'gpt-4o-mini',
      policy_id: null,
      rate_limit: null,
      cache: null,
    },
    status: { kind: 'valid', label: 'Valid', message: '' },
    dependsOn: [],
    referencedBy: [],
  });

  assert.doesNotMatch(html, /Relations/i);
});

test('buildReferenceOptions returns provider and policy suggestions for model forms', () => {
  const options = buildReferenceOptions('models', {
    providers: [{ id: 'openai' }],
    models: [],
    apikeys: [],
    policies: [{ id: 'standard' }],
  });

  assert.deepEqual(options.provider_id, ['openai']);
  assert.deepEqual(options.policy_id, ['standard']);
});

test('reference fields keep custom values not present in suggestions', () => {
  const field = renderField(
    { name: 'provider_id', label: 'Provider ID', type: 'reference', options: ['openai'] },
    'custom-provider',
  );

  assert.match(field, /value="custom-provider"/);
  assert.match(field, /select/);
  assert.match(field, /Manual value/);
});

test('finishEditorFlow resets editor state back to listing', () => {
  assert.deepEqual(
    finishEditorFlow({
      draftMode: 'edit',
      editorCollection: 'models',
      editorId: 'gpt-4o-mini',
      editorValues: { id: 'gpt-4o-mini' },
    }),
    {
      draftMode: null,
      editorCollection: null,
      editorId: null,
      editorValues: null,
    },
  );
});

test('workspace layout prevents empty list panel from stretching vertically', () => {
  const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8');

  assert.match(html, /\.workspace\s*\{[\s\S]*align-items:\s*start;/);
  assert.match(html, /\.main\s*\{[\s\S]*align-content:\s*start;/);
});

test('playground hints copy states that checks do not block live requests', () => {
  const source = readFileSync(new URL('./app.mjs', import.meta.url), 'utf8');

  assert.match(source, /Local checks only\. They do not block sending the live request\./);
});
