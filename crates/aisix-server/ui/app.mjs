const COLLECTIONS = {
  providers: {
    label: 'Providers',
    path: '/admin/providers',
    empty: 'No providers stored in admin config.',
    columns: [
      { key: 'id', label: 'ID' },
      { key: 'kind', label: 'Kind' },
      { key: 'base_url', label: 'Base URL' },
      { key: 'policy_id', label: 'Policy' },
      { key: 'status', label: 'Runtime' },
    ],
    form: [
      { name: 'id', label: 'Provider ID', type: 'text', required: true },
      { name: 'kind', label: 'Kind', type: 'select', options: ['openai', 'azure_openai', 'anthropic'], required: true },
      { name: 'base_url', label: 'Base URL', type: 'text', required: true },
      { name: 'secret_ref', label: 'Secret Ref', type: 'text', required: true },
      { name: 'policy_id', label: 'Policy ID', type: 'reference', source: 'policies' },
      { name: 'cache_mode', label: 'Cache Mode', type: 'select', options: ['', 'inherit', 'enabled', 'disabled'] },
      { name: 'rate_limit_rpm', label: 'RPM', type: 'number' },
      { name: 'rate_limit_tpm', label: 'TPM', type: 'number' },
      { name: 'rate_limit_concurrency', label: 'Concurrency', type: 'number' },
    ],
  },
  models: {
    label: 'Models',
    path: '/admin/models',
    empty: 'No models stored in admin config.',
    columns: [
      { key: 'id', label: 'ID' },
      { key: 'provider_id', label: 'Provider' },
      { key: 'upstream_model', label: 'Upstream' },
      { key: 'policy_id', label: 'Policy' },
      { key: 'status', label: 'Runtime' },
    ],
    form: [
      { name: 'id', label: 'Model ID', type: 'text', required: true },
      { name: 'provider_id', label: 'Provider ID', type: 'reference', source: 'providers', required: true },
      { name: 'upstream_model', label: 'Upstream Model', type: 'text', required: true },
      { name: 'policy_id', label: 'Policy ID', type: 'reference', source: 'policies' },
      { name: 'cache_mode', label: 'Cache Mode', type: 'select', options: ['', 'inherit', 'enabled', 'disabled'] },
      { name: 'rate_limit_rpm', label: 'RPM', type: 'number' },
      { name: 'rate_limit_tpm', label: 'TPM', type: 'number' },
      { name: 'rate_limit_concurrency', label: 'Concurrency', type: 'number' },
    ],
  },
  apikeys: {
    label: 'API Keys',
    path: '/admin/apikeys',
    empty: 'No API keys stored in admin config.',
    columns: [
      { key: 'id', label: 'ID' },
      { key: 'key', label: 'Key' },
      { key: 'allowed_models', label: 'Allowed Models' },
      { key: 'policy_id', label: 'Policy' },
      { key: 'status', label: 'Runtime' },
    ],
    form: [
      { name: 'id', label: 'Key ID', type: 'text', required: true },
      { name: 'key', label: 'Plaintext Key', type: 'text', required: true },
      { name: 'allowed_models', label: 'Allowed Models (comma separated)', type: 'textarea', required: true },
      { name: 'policy_id', label: 'Policy ID', type: 'reference', source: 'policies' },
      { name: 'rate_limit_rpm', label: 'RPM', type: 'number' },
      { name: 'rate_limit_tpm', label: 'TPM', type: 'number' },
      { name: 'rate_limit_concurrency', label: 'Concurrency', type: 'number' },
    ],
  },
  policies: {
    label: 'Policies',
    path: '/admin/policies',
    empty: 'No policies stored in admin config.',
    columns: [
      { key: 'id', label: 'ID' },
      { key: 'rate_limit', label: 'Rate Limit' },
      { key: 'policy_scope', label: 'Used By' },
      { key: 'policy_scope_detail', label: 'References' },
      { key: 'status', label: 'Runtime' },
    ],
    form: [
      { name: 'id', label: 'Policy ID', type: 'text', required: true },
      { name: 'rate_limit_rpm', label: 'RPM', type: 'number', required: true },
      { name: 'rate_limit_tpm', label: 'TPM', type: 'number' },
      { name: 'rate_limit_concurrency', label: 'Concurrency', type: 'number' },
    ],
  },
};

const hasBrowserDom = typeof document !== 'undefined';
const hasSessionStorage = typeof sessionStorage !== 'undefined';
const ADMIN_KEY_STORAGE_KEY = 'aisix-admin-key';
const PLAYGROUND_VIEW = 'playground';
const OPENAPI_VIEW = 'openapi';
const RESOURCE_VIEW = 'resources';
const OPENAPI_YAML_PATH = '/openapi/admin.yaml';
const OPENAPI_STATUS_PATH = `GET ${OPENAPI_YAML_PATH}`;
const OPENAPI_COPY_RESET_DELAY_MS = 1500;

function createInitialOpenApiViewState() {
  return {
    content: '',
    loadState: 'idle',
    error: '',
    copyState: 'idle',
  };
}

function unauthorizedError() {
  const error = new Error('Invalid admin key. Please try again.');
  error.code = 'unauthorized';
  return error;
}

export function viewStatusMeta({ activeView, activeCollection }) {
  if (activeView === PLAYGROUND_VIEW) {
    return {
      title: 'Playground',
      path: 'POST /v1/chat/completions',
    };
  }

  if (activeView === OPENAPI_VIEW) {
    return {
      title: 'OpenAPI',
      path: OPENAPI_STATUS_PATH,
    };
  }

  if (activeView === RESOURCE_VIEW) {
    const resourceConfig = COLLECTIONS[activeCollection] ?? COLLECTIONS.providers;
    return {
      title: resourceConfig.label,
      path: resourceConfig.path,
    };
  }

  return {
    title: 'Unknown view',
    path: 'Unavailable',
  };
}

export function nextOpenApiViewState(
  current = createInitialOpenApiViewState(),
  event = null,
) {
  if (!event) {
    return current;
  }

  if (event.type === 'loading') {
    return {
      ...current,
      loadState: 'loading',
      error: '',
      copyState: 'idle',
    };
  }

  if (event.type === 'success') {
    return {
      ...current,
      content: String(event.content ?? ''),
      loadState: 'ready',
      error: '',
      copyState: 'idle',
    };
  }

  if (event.type === 'error') {
    return {
      ...current,
      loadState: 'error',
      error: String(event.error ?? ''),
      copyState: 'idle',
    };
  }

  if (event.type === 'copy-success') {
    return {
      ...current,
      copyState: 'copied',
    };
  }

  if (event.type === 'copy-reset') {
    return {
      ...current,
      copyState: 'idle',
    };
  }

  return current;
}

export function shouldFetchOpenApiOnTabEnter(openapiState) {
  const nextState = openapiState ?? createInitialOpenApiViewState();
  if (nextState.loadState === 'idle') {
    return true;
  }
  return nextState.loadState === 'error' && !nextState.content;
}

export async function copyOpenApiToClipboard(openapiState, clipboard) {
  const nextState = openapiState ?? createInitialOpenApiViewState();
  if (nextState.loadState !== 'ready' || !nextState.content) {
    return {
      ok: false,
      skipped: true,
      reason: 'unavailable',
    };
  }

  try {
    await clipboard.writeText(nextState.content);
    return {
      ok: true,
      nextState: nextOpenApiViewState(nextState, { type: 'copy-success' }),
    };
  } catch (error) {
    return {
      ok: false,
      skipped: false,
      error: String(error.message ?? error),
    };
  }
}

export function defaultPlaygroundBaseUrl(hostname = '127.0.0.1', protocol = 'http') {
  return `${protocol}://${hostname}:4000`;
}

function browserPlaygroundBaseUrl() {
  if (!hasBrowserDom || !window.location?.hostname) {
    return defaultPlaygroundBaseUrl();
  }
  const protocol = window.location.protocol === 'https:' ? 'https' : 'http';
  return defaultPlaygroundBaseUrl(window.location.hostname, protocol);
}

function createInitialPlaygroundState() {
  return {
    baseUrl: browserPlaygroundBaseUrl(),
    apiKeySelection: 'saved:',
    customApiKey: '',
    modelSelection: 'saved:',
    customModel: '',
    systemPrompt: 'You are a concise assistant.',
    userMessage: 'Say hello in one sentence.',
    requestState: 'idle',
    result: null,
    lastRequestPreview: null,
  };
}

export function nextPlaygroundFormState(current, values) {
  return {
    ...current,
    baseUrl: String(values.base_url ?? current.baseUrl ?? '').trim(),
    apiKeySelection: String(values.api_key_selection ?? current.apiKeySelection ?? 'saved:'),
    customApiKey: String(values.custom_api_key ?? current.customApiKey ?? ''),
    modelSelection: String(values.model_selection ?? current.modelSelection ?? 'saved:'),
    customModel: String(values.custom_model ?? current.customModel ?? ''),
    systemPrompt: String(values.system_prompt ?? current.systemPrompt ?? ''),
    userMessage: String(values.user_message ?? current.userMessage ?? '').trim(),
  };
}

function selectedPlaygroundId(selection, prefix) {
  if (!String(selection ?? '').startsWith(prefix)) {
    return '';
  }
  return String(selection).slice(prefix.length);
}

function playgroundSelectedApiKey(data, selection) {
  const selectedApiKeyId = selectedPlaygroundId(selection, 'saved:');
  return data.apikeys.find((item) => item.id === selectedApiKeyId) ?? null;
}

function playgroundSelectedModel(data, selection) {
  const selectedModelId = selectedPlaygroundId(selection, 'saved:');
  return data.models.find((item) => item.id === selectedModelId) ?? null;
}

function normalizeBaseUrl(baseUrl) {
  return String(baseUrl ?? '').trim().replace(/\/+$/, '');
}

export function buildPlaygroundRequest({ baseUrl, apiKey, model, systemPrompt, userMessage }) {
  const headers = new Headers();
  headers.set('authorization', `Bearer ${String(apiKey).trim()}`);
  headers.set('content-type', 'application/json');

  const messages = [];
  if (String(systemPrompt ?? '').trim()) {
    messages.push({ role: 'system', content: String(systemPrompt).trim() });
  }
  messages.push({ role: 'user', content: String(userMessage ?? '').trim() });

  return {
    url: `${normalizeBaseUrl(baseUrl)}/v1/chat/completions`,
    options: {
      method: 'POST',
      headers,
      body: JSON.stringify({
        model: String(model ?? '').trim(),
        stream: false,
        messages,
      }),
    },
  };
}

export function derivePlaygroundHints(data, derived, selection) {
  const modelId = resolvePlaygroundModel(data, selection);
  const apiKey = selection.apiKeySelection !== 'custom'
    ? playgroundSelectedApiKey(data, selection.apiKeySelection)
    : null;
  const model = derived.models.byId[modelId] ?? null;
  const allowsModel = apiKey ? apiKey.allowed_models.includes(modelId) : null;

  return {
    modelExists: {
      ok: Boolean(model),
      message: model ? 'Model exists in admin config.' : `Model '${modelId || 'unknown'}' is not present in admin config.`,
    },
    apiKeyAllowsModel: {
      ok: allowsModel === null ? null : allowsModel,
      message: apiKey
        ? allowsModel
          ? `API key '${apiKey.id}' allows this model.`
          : `API key '${apiKey.id}' does not allow this model.`
        : 'Manual API key selected. Allowlist cannot be checked locally.',
    },
    runtimeStatus: model?.status ?? {
      kind: 'orphaned',
      label: 'Unknown',
      message: 'Runtime status is unknown until the model exists in admin config.',
    },
  };
}

export function resolvePlaygroundApiKey(data, selection) {
  if (selection.apiKeySelection === 'custom') {
    return String(selection.customApiKey ?? '').trim();
  }
  return playgroundSelectedApiKey(data, selection.apiKeySelection)?.key ?? selectedPlaygroundId(selection.apiKeySelection, 'saved:');
}

export function resolvePlaygroundModel(data, selection) {
  if (selection.modelSelection === 'custom') {
    return String(selection.customModel ?? '').trim();
  }
  return playgroundSelectedModel(data, selection.modelSelection)?.id ?? selectedPlaygroundId(selection.modelSelection, 'saved:');
}

export function extractAssistantText(payload) {
  const firstChoice = payload?.choices?.[0];
  const content = firstChoice?.message?.content;
  if (typeof content === 'string') {
    return content;
  }
  if (Array.isArray(content)) {
    return content
      .map((item) => item?.text ?? '')
      .filter(Boolean)
      .join('\n');
  }
  return '';
}

export function classifyPlaygroundFailure({ status, error }) {
  if (error) {
    return { category: 'network_error', title: 'Network error' };
  }
  if (status === 400) {
    return { category: 'invalid_request', title: 'Invalid request' };
  }
  if (status === 401 || status === 403) {
    return { category: 'auth_failed', title: 'Auth failed' };
  }
  if (status === 404 || status === 422) {
    return { category: 'model_rejected', title: 'Model rejected' };
  }
  return { category: 'upstream_error', title: 'Upstream error' };
}

function isJsonMediaType(contentType) {
  const mediaType = String(contentType ?? '').split(';', 1)[0].trim().toLowerCase();
  return mediaType === 'application/json' || mediaType.endsWith('+json');
}

async function readPlaygroundResponseBody(response, contentType) {
  const responseText = await response.text();
  if (!isJsonMediaType(contentType)) {
    return { responseFormat: 'text', responseBody: responseText };
  }
  try {
    return {
      responseFormat: 'json',
      responseBody: JSON.parse(responseText),
    };
  } catch {
    return { responseFormat: 'text', responseBody: responseText };
  }
}

export async function executePlaygroundRequest(input, fetchImpl = fetch, nowImpl = Date.now) {
  const request = buildPlaygroundRequest(input);
  const startedAt = nowImpl();
  let responseFormat = 'text';
  try {
    const response = await fetchImpl(request.url, request.options);
    const finishedAt = nowImpl();
    const contentType = response.headers?.get?.('content-type') ?? '';
    const parsedResponse = await readPlaygroundResponseBody(response, contentType);
    responseFormat = parsedResponse.responseFormat;
    const responseBody = parsedResponse.responseBody;

    if (response.ok) {
      return {
        ok: true,
        status: response.status,
        durationMs: Math.max(0, finishedAt - startedAt),
        assistantText: extractAssistantText(responseBody),
        responseFormat,
        responseBody,
        request,
      };
    }

    const failure = classifyPlaygroundFailure({ status: response.status });
    return {
      ok: false,
      status: response.status,
      durationMs: Math.max(0, finishedAt - startedAt),
      error: failure,
      assistantText: '',
      responseFormat,
      responseBody,
      request,
    };
  } catch (error) {
    return {
      ok: false,
      status: null,
      durationMs: Math.max(0, nowImpl() - startedAt),
      error: classifyPlaygroundFailure({ error }),
      assistantText: '',
      responseFormat,
      responseBody: String(error.message ?? error),
      request,
    };
  }
}

function adminKeyStorage() {
  if (adminKeyStorageMode() !== 'session' || !hasSessionStorage) {
    return null;
  }
  return sessionStorage;
}

export function restoreAdminKeyValidity(adminKey) {
  return Boolean(adminKey && adminKey.trim());
}

export function shouldRefreshOnInit({ adminKey, adminKeyValid }) {
  return Boolean(adminKeyValid && adminKey && adminKey.trim());
}

const initialAdminKey = adminKeyStorage()?.getItem(ADMIN_KEY_STORAGE_KEY) ?? '';

const state = {
  adminKey: initialAdminKey,
  adminKeyValid: restoreAdminKeyValidity(initialAdminKey),
  adminKeyError: '',
  isValidatingAdminKey: false,
  activeView: RESOURCE_VIEW,
  activeCollection: 'providers',
  data: {
    providers: [],
    models: [],
    apikeys: [],
    policies: [],
  },
  derived: null,
  search: '',
  filter: 'all',
  draftMode: null,
  editorCollection: null,
  editorValues: null,
  editorId: null,
  revealMap: new Map(),
  connectionState: 'idle',
  lastRefreshed: null,
  flashRevision: null,
  playground: createInitialPlaygroundState(),
  openapi: createInitialOpenApiViewState(),
  adminSpec: null,
  schemaFields: {},
};

const ADMIN_PUT_SCHEMA_PATHS = {
  providers: '/admin/providers/{id}',
  models: '/admin/models/{id}',
  apikeys: '/admin/apikeys/{id}',
  policies: '/admin/policies/{id}',
};

const appRoot = hasBrowserDom ? document.querySelector('#app') : null;
const modalRoot = hasBrowserDom ? document.querySelector('#modal-root') : null;
const toastRoot = hasBrowserDom ? document.querySelector('#toast-root') : null;
let openApiCopyResetTimeout = null;

function init() {
  if (!hasBrowserDom || !appRoot || !modalRoot || !toastRoot) {
    return;
  }
  render();
  if (shouldRefreshOnInit(state)) {
    void refreshAll();
  }
}

function render() {
  const derived = state.derived ?? deriveRelationshipModel(state.data);
  state.derived = derived;
  const collection = state.activeCollection;
  const resourceConfig = COLLECTIONS[collection];
  const derivedCollection = derived[collection];
  const items = filterItems(derivedCollection.items, collection);
  const editorCollection = state.editorCollection ?? collection;
  const editorItem = state.editorId ? derived[editorCollection]?.byId[state.editorId] ?? null : null;
  const editorValues = state.editorValues ?? defaultFormValues(editorCollection);
  const uiMode = nextAdminUiMode({
    adminKey: state.adminKey,
    adminKeyValid: state.adminKeyValid,
    draftMode: state.draftMode,
  });
  const playgroundHints = derivePlaygroundHints(state.data, derived, state.playground);
  const statusMeta = viewStatusMeta({
    activeView: state.activeView,
    activeCollection: state.activeCollection,
  });

  appRoot.innerHTML = `
    <div class="layout">
      <aside class="sidebar">
        <div class="brand">
          <h1>AISIX Control Plane</h1>
        </div>
        <div class="nav" style="margin-top: 18px;">
          <div class="nav-group">
            <div class="nav-group-title">Resources</div>
            ${Object.entries(COLLECTIONS)
              .map(
                ([key, value]) => `
                  <button class="${state.activeView === RESOURCE_VIEW && key === collection ? 'active' : ''}" data-nav="${key}" type="button">
                    <span>${value.label}</span>
                    <span class="count">${derived[key].items.length}</span>
                  </button>
                `,
              )
              .join('')}
          </div>
          <div class="nav-divider" aria-hidden="true"></div>
          <div class="nav-group">
            <div class="nav-group-title">Tools</div>
            <button class="${state.activeView === PLAYGROUND_VIEW ? 'active' : ''}" data-view="${PLAYGROUND_VIEW}" type="button">
              <span>Playground</span>
              <span class="count">live</span>
            </button>
            <button class="${state.activeView === OPENAPI_VIEW ? 'active' : ''}" data-view="${OPENAPI_VIEW}" type="button">
              <span>OpenAPI</span>
              <span class="count">yaml</span>
            </button>
          </div>
        </div>
      </aside>
      <main class="main">
        <section class="status-bar">
          <div>
            <strong>${statusMeta.title}</strong>
            <div class="muted">Current endpoint: ${statusMeta.path}</div>
          </div>
          <div class="status-grid">
            <span class="badge ${badgeClassForConnection()}">${connectionText()}</span>
            <span class="badge">Stored vs Runtime Semantics</span>
            <span class="badge">Last refresh: ${state.lastRefreshed ? formatTimestamp(state.lastRefreshed) : 'never'}</span>
            ${state.flashRevision ? `<span class="badge success">Revision ${state.flashRevision}</span>` : ''}
          </div>
        </section>
        <section class="workspace">
          ${state.activeView === PLAYGROUND_VIEW
            ? renderPlaygroundView(playgroundHints)
            : state.activeView === OPENAPI_VIEW
              ? renderOpenApiView(state.openapi)
            : uiMode.mode === 'editing'
              ? renderEditorView(editorCollection, editorValues, editorItem)
              : renderListView(collection, items)}
        </section>
      </main>
    </div>
    ${!state.adminKeyValid ? renderAdminKeyGate() : ''}
  `;

  bindGlobalEvents();
  if (state.activeView === RESOURCE_VIEW) {
    bindTableEvents(items);
    bindEditorEvents(editorItem);
  }
}

function renderListView(collection, items) {
  const resourceConfig = COLLECTIONS[collection];
  return `
    <div class="panel">
      <div class="panel-header">
        <div>
          <h2>${resourceConfig.label}</h2>
          <div class="muted">${resourceConfig.empty}</div>
        </div>
        <div class="detail-actions">
          <button class="secondary-button" type="button" id="refresh-button">Refresh</button>
          <button class="button" type="button" id="create-button">Create ${resourceConfig.label.slice(0, -1) || resourceConfig.label}</button>
        </div>
      </div>
      <div class="controls">
        <input id="search-input" type="search" placeholder="Search by id, relation, or summary" value="${escapeHtml(state.search)}" />
        <select id="status-filter">
          <option value="all" ${state.filter === 'all' ? 'selected' : ''}>All statuses</option>
          <option value="valid" ${state.filter === 'valid' ? 'selected' : ''}>Valid</option>
          <option value="missing_dependency" ${state.filter === 'missing_dependency' ? 'selected' : ''}>Missing dependency</option>
          <option value="orphaned" ${state.filter === 'orphaned' ? 'selected' : ''}>Orphaned</option>
        </select>
        <button class="secondary-button" type="button" id="clear-filter-button">Clear</button>
      </div>
      ${renderTable(collection, items)}
    </div>
  `;
}

function renderEditorView(collection, values, item) {
  const resourceConfig = COLLECTIONS[collection];
  const modeLabel = state.draftMode === 'create' ? 'Create resource' : 'Edit resource';
  const heading = state.draftMode === 'create'
    ? `Create ${resourceConfig.label.slice(0, -1) || resourceConfig.label}`
    : escapeHtml(item?.id ?? values.id ?? resourceConfig.label);
  const statusLine = item ? `<div class="status-line">${statusBadgeMarkup(item.status)}</div>` : '<div class="muted">New resource draft</div>';

  return `
    <div class="panel">
      <div class="detail-sections">
        <div class="detail-header">
          <div>
            <div class="muted">${modeLabel}</div>
            <h2>${heading}</h2>
            ${statusLine}
          </div>
          <div class="detail-actions">
            <button class="secondary-button" type="button" id="back-button">Back</button>
            ${collection === 'apikeys' && item ? `<button class="secondary-button" type="button" id="toggle-secret-button">${state.revealMap.get(item.id) ? 'Hide key' : 'Reveal key'}</button>` : ''}
            ${state.draftMode === 'edit' && item ? `<button class="secondary-button danger-button" type="button" id="delete-button">Delete</button>` : ''}
          </div>
        </div>
        ${item ? renderEditorSummary(collection, item) : ''}
        <section>
          <div class="split-line">
            <h3>${modeLabel}</h3>
            <span class="muted">PUT ${resourceConfig.path}/:id</span>
          </div>
          <form id="resource-form" class="form-grid">
            ${renderFormFields(collection, values, { readonlyId: state.draftMode === 'edit' })}
            <div class="form-actions">
              <button class="button" type="submit">Save</button>
              <button class="secondary-button" type="button" id="back-button-secondary">Back</button>
              ${state.draftMode === 'edit' && item ? `<button class="secondary-button danger-button" type="button" id="delete-button-secondary">Delete</button>` : ''}
            </div>
          </form>
        </section>
      </div>
    </div>
  `;
}

function renderPlaygroundView(hints) {
  const savedApiKeys = state.data.apikeys;
  const models = state.data.models;
  const result = state.playground.result;
  const showCustomApiKey = state.playground.apiKeySelection === 'custom';
  const showCustomModel = state.playground.modelSelection === 'custom';
  return `
    <div class="playground-grid">
      <div class="panel playground-panel">
        <div class="panel-header">
          <div>
            <h2>Data Plane Playground</h2>
            <div class="muted">This sends a real request to the data plane, not the Admin API.</div>
          </div>
        </div>
        <form id="playground-form" class="form-grid">
          <label>
            Data Plane Base URL
            <input name="base_url" type="text" value="${escapeHtml(state.playground.baseUrl)}" required />
            <small>Defaults to the data plane port 4000.</small>
          </label>
          <label>
            API Key
            <select name="api_key_selection">
              ${savedApiKeys.map((item) => `<option value="saved:${escapeHtml(item.id)}" ${state.playground.apiKeySelection === `saved:${item.id}` ? 'selected' : ''}>${escapeHtml(item.id)}</option>`).join('')}
              <option value="custom" ${showCustomApiKey ? 'selected' : ''}>Custom</option>
            </select>
          </label>
          ${showCustomApiKey
            ? `<label>
                API Key
                <input name="custom_api_key" type="password" value="${escapeHtml(state.playground.customApiKey)}" placeholder="sk-..." required />
              </label>`
            : ''}
          <label>
            Model
            <select name="model_selection">
              ${models.map((item) => `<option value="saved:${escapeHtml(item.id)}" ${state.playground.modelSelection === `saved:${item.id}` ? 'selected' : ''}>${escapeHtml(item.id)}</option>`).join('')}
              <option value="custom" ${showCustomModel ? 'selected' : ''}>Custom</option>
            </select>
          </label>
          ${showCustomModel
            ? `<label>
                Model
                <input name="custom_model" type="text" value="${escapeHtml(state.playground.customModel)}" placeholder="gpt-4o-mini" required />
              </label>`
            : ''}
          <label>
            System Prompt
            <textarea name="system_prompt">${escapeHtml(state.playground.systemPrompt)}</textarea>
          </label>
          <label>
            User Message
            <textarea name="user_message" required>${escapeHtml(state.playground.userMessage)}</textarea>
          </label>
          <div class="form-actions">
            <button class="button" type="submit" ${state.playground.requestState === 'submitting' ? 'disabled' : ''}>${state.playground.requestState === 'submitting' ? 'Sending...' : 'Send Test Request'}</button>
          </div>
        </form>
      </div>
      <div class="playground-side">
        <div class="panel">
          <div class="panel-header">
                <div>
                  <h2>Local Hints</h2>
                  <div class="muted">Local checks only. They do not block sending the live request. Final truth comes from the live data-plane response.</div>
                </div>
              </div>
          <div class="hint-list">
            ${renderHintRow('Model exists in admin config', hints.modelExists.ok, hints.modelExists.message)}
            ${renderHintRow('Selected API key allows model', hints.apiKeyAllowsModel.ok, hints.apiKeyAllowsModel.message)}
            ${renderHintRow('Runtime relationship status', hints.runtimeStatus.kind === 'valid', hints.runtimeStatus.message)}
          </div>
        </div>
        <div class="panel">
          <div class="panel-header">
            <div>
              <h2>Result</h2>
              <div class="muted">Latest live probe against the data plane.</div>
            </div>
          </div>
          ${renderPlaygroundResult(result)}
        </div>
      </div>
    </div>
  `;
}

export function renderOpenApiView(openapiState) {
  const nextState = openapiState ?? createInitialOpenApiViewState();
  const canCopy = nextState.loadState === 'ready' && Boolean(nextState.content);
  const copyLabel = canCopy && nextState.copyState === 'copied' ? 'Copied' : 'Copy';
  let content = '<div class="muted">OpenAPI YAML has not been loaded yet.</div>';

  if (nextState.loadState === 'loading') {
    content = '<div class="muted">Loading OpenAPI YAML...</div>';
  } else if (nextState.loadState === 'error') {
    content = `<div class="badge danger danger-text">${escapeHtml(nextState.error || 'OpenAPI load failed.')}</div>`;
  } else if (nextState.loadState === 'ready') {
    content = `<pre>${escapeHtml(nextState.content)}</pre>`;
  }

  return `
    <div class="panel">
      <div class="panel-header">
        <div>
          <h2>Admin OpenAPI Contract</h2>
          <div class="muted">OpenAPI 3.1 contract for debugging admin endpoints, validating request shapes, generate clients, and wiring API tools.</div>
        </div>
        <div class="detail-actions">
          <a class="secondary-button" href="${OPENAPI_YAML_PATH}">Open Raw YAML</a>
        </div>
      </div>
      <div class="openapi-code-block">
        <div class="openapi-code-actions">
          <button class="secondary-button" type="button" id="copy-openapi-button" ${canCopy ? '' : 'disabled'}>${copyLabel}</button>
        </div>
        ${content}
      </div>
    </div>
  `;
}

function renderHintRow(title, ok, message) {
  const tone = ok == null ? '' : ok ? 'success' : 'warning';
  const label = ok == null ? 'Manual check' : ok ? 'Looks good' : 'Check this';
  return `
    <div class="hint-row">
      <div class="split-line">
        <strong>${escapeHtml(title)}</strong>
        <span class="badge ${tone}">${escapeHtml(label)}</span>
      </div>
      <div class="muted">${escapeHtml(message)}</div>
    </div>
  `;
}

export function renderPlaygroundResult(result) {
  if (!result) {
    return '<div class="playground-empty muted">Send a live request to validate the selected configuration.</div>';
  }

  const tone = result.ok ? 'success' : 'danger';
  const title = result.ok ? 'Data plane reachable' : result.error?.title ?? 'Request failed';
  const responseBody = typeof result.responseBody === 'string' ? result.responseBody : JSON.stringify(result.responseBody, null, 2);
  const requestBody = result.request?.options?.body ? JSON.stringify(JSON.parse(result.request.options.body), null, 2) : '{}';
  const responseTitle = result.responseFormat === 'json' ? 'Original Response JSON' : 'Original Response';
  const responseMeta = [];
  if (result.status != null) responseMeta.push(`HTTP ${result.status}`);
  responseMeta.push(`${result.durationMs} ms`);
  if (result.responseBody?.usage) responseMeta.push(`Usage ${JSON.stringify(result.responseBody.usage)}`);

  return `
    <div class="playground-result">
      <div class="status-line">
        <span class="badge ${tone}">${escapeHtml(title)}</span>
        ${responseMeta.map((segment) => `<span class="badge">${escapeHtml(segment)}</span>`).join('')}
      </div>
      <div class="playground-output">
        <strong>Assistant</strong>
        <pre>${escapeHtml(result.assistantText || 'No assistant text returned.')}</pre>
      </div>
      <div class="playground-output">
        <strong>Original Request JSON</strong>
        <pre>${escapeHtml(requestBody)}</pre>
      </div>
      <div class="playground-output">
        <strong>${responseTitle}</strong>
        <pre>${escapeHtml(responseBody)}</pre>
      </div>
    </div>
  `;
}

function syncEditorValuesFromForm(formData) {
  if (!state.draftMode || !state.editorCollection) {
    return;
  }
  const nextValues = { ...state.editorValues };
  for (const [key, value] of formData.entries()) {
    nextValues[key] = value;
  }
  state.editorValues = nextValues;
}

function renderAdminKeyGate() {
  return `
    <div class="modal-backdrop open">
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="admin-key-title">
        <div class="detail-header">
          <div>
            <h2 id="admin-key-title">Enter Admin Key</h2>
            <p class="muted">A valid admin key is required before the control plane can load.</p>
          </div>
        </div>
        <form id="admin-key-form" class="form-grid">
          <label>
            Admin Key
            <input id="admin-key-input" name="admin_key" type="password" placeholder="x-admin-key" value="${escapeHtml(state.adminKey)}" required />
            <small>Stored in sessionStorage for this browser tab only.</small>
          </label>
          ${state.adminKeyError ? `<div class="badge danger">${escapeHtml(state.adminKeyError)}</div>` : ''}
          <div class="form-actions">
            <button class="button" type="submit" ${state.isValidatingAdminKey ? 'disabled' : ''}>${state.isValidatingAdminKey ? 'Validating...' : 'Validate'}</button>
          </div>
        </form>
      </div>
    </div>
  `;
}

function renderTable(collection, items) {
  const columns = COLLECTIONS[collection].columns;
  return `
    <div class="table">
      <div class="table-header">${columns.map((column) => `<div>${column.label}</div>`).join('')}<div>Actions</div></div>
      ${
        items.length
          ? items
              .map((item) => {
                const actions = buildRowActions(collection, item.id);
                return `
                  <div class="table-row" data-row-id="${item.id}">
                    ${columns
                      .map((column) => `<div>${escapeHtml(formatColumnValue(collection, column.key, item.raw, item))}</div>`)
                      .join('')}
                    <div class="row-actions">
                      ${actions
                        .map((action) =>
                          action.kind === 'edit'
                            ? `<button class="secondary-button" type="button" data-edit-id="${action.id}">Edit</button>`
                            : `<button class="secondary-button danger-button" type="button" data-delete-id="${action.id}">Delete</button>`,
                        )
                        .join('')}
                    </div>
                  </div>
                `;
              })
              .join('')
          : `<div class="table-empty">${COLLECTIONS[collection].empty}</div>`
      }
    </div>
  `;
}

function renderSummary(collection, item) {
  const raw = item.raw;
  const entries = [];
  if (collection === 'providers') {
    entries.push(['Kind', raw.kind]);
    entries.push(['Base URL', raw.base_url]);
    entries.push(['Policy', raw.policy_id ?? 'none']);
    entries.push(['Cache', raw.cache?.mode ?? 'inherit']);
    entries.push(['Rate limit', formatRateLimit(raw.rate_limit)]);
  } else if (collection === 'models') {
    entries.push(['Provider', raw.provider_id]);
    entries.push(['Upstream', raw.upstream_model]);
    entries.push(['Policy', raw.policy_id ?? 'none']);
    entries.push(['Cache', raw.cache?.mode ?? 'inherit']);
    entries.push(['Rate limit', formatRateLimit(raw.rate_limit)]);
  } else if (collection === 'apikeys') {
    entries.push(['Key', state.revealMap.get(item.id) ? raw.key : maskApiKey(raw.key)]);
    entries.push(['Allowed models', raw.allowed_models.join(', ') || 'none']);
    entries.push(['Policy', raw.policy_id ?? 'none']);
    entries.push(['Rate limit', formatRateLimit(raw.rate_limit)]);
  } else if (collection === 'policies') {
    entries.push(['Rate limit', formatRateLimit(raw.rate_limit)]);
    entries.push(['Used by', item.referencedBy.length ? String(item.referencedBy.length) : '0']);
  }

  return `<dl class="definition-list">${entries
    .map(([term, value]) => `<dt>${escapeHtml(term)}</dt><dd>${escapeHtml(value)}</dd>`)
    .join('')}</dl>`;
}

export function renderEditorSummary(collection, item) {
  return `
    <section>
      <div class="split-line">
        <h3>Summary</h3>
        <span class="muted">Stored in admin config</span>
      </div>
      ${renderSummary(collection, item)}
    </section>
  `;
}

function renderFormFields(collection, values, options = {}) {
  const referenceOptions = buildReferenceOptions(collection, state.data);
  const fields = fieldsForCollection(collection).map((field) => {
    const enriched = field.type === 'reference' ? { ...field, options: referenceOptions[field.name] ?? [] } : field;
    if (options.readonlyId && field.name === 'id') {
      return { ...enriched, readonly: true };
    }
    return enriched;
  });
  const fieldMarkup = fields.map((field) => renderField(field, values[field.name] ?? ''));
  if (collection === 'apikeys') {
    return `${fieldMarkup.slice(0, 4).join('')}<div class="field-grid">${fieldMarkup.slice(4).join('')}</div>`;
  }
  const normal = [];
  const metrics = [];
  for (const field of fieldMarkup) {
    if (field.includes('rate_limit_')) {
      metrics.push(field);
    } else {
      normal.push(field);
    }
  }
  return `${normal.join('')}<div class="field-grid">${metrics.join('')}</div>`;
}

export function renderField(field, value) {
  const required = field.required ? 'required' : '';
  const readonly = field.readonly ? 'readonly' : '';
  if (field.type === 'reference') {
    const options = field.options ?? [];
    const selectedValue = options.includes(String(value)) ? String(value) : '';
    return `
      <label>
        ${escapeHtml(field.label)}
        <select name="${field.name}__preset" ${readonly}>
          <option value="">Select existing...</option>
          ${options.map((option) => `<option value="${escapeHtml(option)}" ${selectedValue === option ? 'selected' : ''}>${escapeHtml(option)}</option>`).join('')}
        </select>
        <input name="${field.name}" type="text" value="${escapeHtml(String(value))}" placeholder="Manual value" ${required} ${readonly} />
      </label>
    `;
  }
  if (field.type === 'select') {
    return `
      <label>
        ${escapeHtml(field.label)}
        <select name="${field.name}" ${required}>
          ${field.options.map((option) => `<option value="${option}" ${String(value) === option ? 'selected' : ''}>${option || 'None'}</option>`).join('')}
        </select>
      </label>
    `;
  }
  if (field.type === 'textarea') {
    return `
      <label>
        ${escapeHtml(field.label)}
        <textarea name="${field.name}" ${required}>${escapeHtml(String(value))}</textarea>
      </label>
    `;
  }
  return `
      <label>
        ${escapeHtml(field.label)}
        <input name="${field.name}" type="${field.type}" value="${escapeHtml(String(value))}" ${required} ${readonly} />
      </label>
    `;
  }

function bindGlobalEvents() {
  document.querySelector('#admin-key-form')?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const formData = new FormData(event.currentTarget);
    const adminKey = String(formData.get('admin_key') ?? '').trim();

    state.adminKey = adminKey;
    state.isValidatingAdminKey = true;
    state.adminKeyError = '';
    render();

    const result = await validateAdminKey(adminKey);
    state.isValidatingAdminKey = false;

    if (!result.valid) {
      state.adminKeyValid = false;
      state.adminKeyError = result.message;
      render();
      return;
    }

    adminKeyStorage()?.setItem(ADMIN_KEY_STORAGE_KEY, adminKey);
    state.adminKeyValid = true;
    state.adminKeyError = '';
    await refreshAll();
  });

  document.querySelector('[data-view="playground"]')?.addEventListener('click', () => {
    state.activeView = PLAYGROUND_VIEW;
    Object.assign(state, finishEditorFlow());
    render();
  });

  document.querySelector(`[data-view="${OPENAPI_VIEW}"]`)?.addEventListener('click', async () => {
    state.activeView = OPENAPI_VIEW;
    Object.assign(state, finishEditorFlow());
    render();
    if (shouldFetchOpenApiOnTabEnter(state.openapi)) {
      await refreshOpenApiYaml();
    }
  });

  document.querySelectorAll('[data-nav]').forEach((button) => {
    button.addEventListener('click', () => {
      state.activeView = RESOURCE_VIEW;
      state.activeCollection = button.dataset.nav;
      Object.assign(state, finishEditorFlow());
      render();
    });
  });

  document.querySelector('#search-input')?.addEventListener('input', (event) => {
    state.search = event.target.value;
    render();
  });

  document.querySelector('#status-filter')?.addEventListener('change', (event) => {
    state.filter = event.target.value;
    render();
  });

  document.querySelector('#clear-filter-button')?.addEventListener('click', () => {
    state.search = '';
    state.filter = 'all';
    render();
  });

  document.querySelector('#refresh-button')?.addEventListener('click', () => {
    void refreshAll();
  });

  document.querySelector('#copy-openapi-button')?.addEventListener('click', async () => {
    const clipboard = navigator.clipboard;
    // #copy-openapi-button still delegates to navigator.clipboard.writeText through the helper.
    const result = await copyOpenApiToClipboard(state.openapi, clipboard);
    if (!result.ok) {
      if (!result.skipped) {
        showToast('Copy failed', result.error, 'danger');
      }
      return;
    }

    state.openapi = result.nextState;
    render();

    if (openApiCopyResetTimeout) {
      clearTimeout(openApiCopyResetTimeout);
    }
    openApiCopyResetTimeout = setTimeout(() => {
      state.openapi = nextOpenApiViewState(state.openapi, { type: 'copy-reset' });
      openApiCopyResetTimeout = null;
      render();
    }, OPENAPI_COPY_RESET_DELAY_MS);
  });

  document.querySelector('#create-button')?.addEventListener('click', () => {
    Object.assign(state, startCreateAction(state.activeCollection));
    render();
  });

  document.querySelector('#playground-form')?.addEventListener('submit', (event) => {
    event.preventDefault();
    void submitPlaygroundForm(event.currentTarget);
  });

  document.querySelector('#playground-form')?.addEventListener('change', (event) => {
    const form = event.currentTarget;
    const values = Object.fromEntries(new FormData(form).entries());
    state.playground = nextPlaygroundFormState(state.playground, values);
    render();
  });
}

async function submitPlaygroundForm(form) {
  const formData = new FormData(form);
  state.playground = {
    ...nextPlaygroundFormState(state.playground, Object.fromEntries(formData.entries())),
    requestState: 'submitting',
  };
  render();

  const result = await executePlaygroundRequest({
    baseUrl: state.playground.baseUrl,
    apiKey: resolvePlaygroundApiKey(state.data, state.playground),
    model: resolvePlaygroundModel(state.data, state.playground),
    systemPrompt: state.playground.systemPrompt,
    userMessage: state.playground.userMessage,
  });

  state.playground = {
    ...state.playground,
    requestState: result.ok ? 'success' : 'error',
    result,
    lastRequestPreview: result.request,
  };
  render();
}

function bindTableEvents(items) {
  document.querySelectorAll('[data-edit-id]').forEach((button) => {
    button.addEventListener('click', () => {
      const item = items.find((candidate) => candidate.id === button.dataset.editId);
      if (!item) {
        return;
      }
      Object.assign(state, startEditAction(state.activeCollection, item.raw));
      render();
    });
  });

  document.querySelectorAll('[data-delete-id]').forEach((button) => {
    button.addEventListener('click', () => {
      showDeleteModal(state.activeCollection, button.dataset.deleteId);
    });
  });
}

function bindEditorEvents(selected) {
  if (!state.draftMode) {
    return;
  }

  const goBack = () => {
    Object.assign(state, finishEditorFlow());
    render();
  };

  document.querySelector('#back-button')?.addEventListener('click', goBack);
  document.querySelector('#back-button-secondary')?.addEventListener('click', goBack);

  document.querySelectorAll('[name$="__preset"]').forEach((select) => {
    select.addEventListener('change', (event) => {
      const target = event.currentTarget;
      const fieldName = target.name.replace(/__preset$/, '');
      const input = document.querySelector(`[name="${fieldName}"]`);
      if (!(input instanceof HTMLInputElement)) {
        return;
      }
      if (target.value) {
        input.value = target.value;
      }
    });
  });

  document.querySelector('#toggle-secret-button')?.addEventListener('click', () => {
    if (!selected) {
      return;
    }
    const current = state.revealMap.get(selected.id) ?? false;
    state.revealMap.set(selected.id, !current);
    render();
  });

  document.querySelector('#delete-button')?.addEventListener('click', () => {
    if (selected) {
      showDeleteModal(state.editorCollection, selected.id);
    }
  });

  document.querySelector('#delete-button-secondary')?.addEventListener('click', () => {
    if (selected) {
      showDeleteModal(state.editorCollection, selected.id);
    }
  });

  document.querySelector('#resource-form')?.addEventListener('submit', (event) => {
    event.preventDefault();
    const formData = new FormData(event.currentTarget);
    syncEditorValuesFromForm(formData);
    const values = Object.fromEntries(formData.entries());
    void saveResource(state.editorCollection, values, selected?.id ?? null);
  });
}

async function refreshAll() {
  if (!state.adminKeyValid) {
    state.connectionState = 'idle';
    render();
    return;
  }

  const next = nextAdminRefreshState(state.adminKey);
  state.connectionState = next.connectionState;
  if (!next.shouldRefresh) {
    render();
    return;
  }
  render();
  try {
    if (!state.adminSpec) {
      state.adminSpec = await fetchAdminSpec();
      state.schemaFields = Object.fromEntries(
        Object.keys(COLLECTIONS).map((collection) => [
          collection,
          buildSchemaBackedFieldDefinitions(collection, extractAdminPutSchema(state.adminSpec, collection)),
        ]),
      );
    }
    const [providers, models, apikeys, policies] = await Promise.all([
      fetchCollection('providers'),
      fetchCollection('models'),
      fetchCollection('apikeys'),
      fetchCollection('policies'),
    ]);
    state.data = { providers, models, apikeys, policies };
    state.derived = deriveRelationshipModel(state.data);
    state.connectionState = 'ready';
    state.lastRefreshed = Date.now();
    render();
  } catch (error) {
    if (!state.adminKeyValid) {
      return;
    }
    state.connectionState = 'error';
    showToast('Connection error', error.message, 'danger');
    render();
  }
}

export async function fetchOpenApiYaml(adminKey, fetchImpl = fetch) {
  const response = await fetchImpl(OPENAPI_YAML_PATH, {
    headers: { 'x-admin-key': String(adminKey ?? '').trim() },
  });

  if (response.status === 401) {
    throw unauthorizedError();
  }

  const yaml = await response.text();
  if (!response.ok) {
    throw new Error(yaml);
  }

  return yaml;
}

async function refreshOpenApiYaml() {
  state.openapi = nextOpenApiViewState(state.openapi, { type: 'loading' });
  render();

  try {
    const content = await fetchOpenApiYaml(state.adminKey);
    state.openapi = nextOpenApiViewState(state.openapi, { type: 'success', content });
    render();
  } catch (error) {
    const message = String(error.message ?? error);
    state.openapi = nextOpenApiViewState(state.openapi, { type: 'error', error: message });
    if (error.code === 'unauthorized') {
      handleUnauthorized();
      return;
    }
    showToast('OpenAPI load failed', message, 'danger');
    render();
  }
}

async function fetchCollection(collection) {
  const response = await fetch(COLLECTIONS[collection].path, {
    headers: adminHeaders(),
  });
  if (response.status === 401) {
    handleUnauthorized();
    throw new Error('Invalid admin key. Please try again.');
  }
  if (!response.ok) {
    throw new Error(`Failed to load ${collection}: ${await extractError(response)}`);
  }
  return response.json();
}

async function fetchAdminSpec() {
  const response = await fetch('/openapi/admin.json', {
    headers: adminHeaders(),
  });
  if (response.status === 401) {
    handleUnauthorized();
    throw new Error('Invalid admin key. Please try again.');
  }
  if (!response.ok) {
    throw new Error(`Failed to load admin spec: ${await extractError(response)}`);
  }
  return response.json();
}

async function saveResource(collection, values, currentId = null) {
  try {
    const payload = buildResourcePayload(collection, values);
    const id = payload.id ?? currentId;
    const response = await fetch(`${COLLECTIONS[collection].path}/${encodeURIComponent(id)}`, {
      method: 'PUT',
      headers: buildJsonAdminHeaders(state.adminKey),
      body: JSON.stringify(payload),
    });
    if (response.status === 401) {
      handleUnauthorized();
      throw new Error('Invalid admin key. Please try again.');
    }
    if (!response.ok) {
      throw new Error(await extractError(response));
    }
    const result = await response.json();
    state.flashRevision = result.revision;
    showToast('Stored successfully', `${collection} '${id}' stored at revision ${result.revision}.`, 'success');
    await refreshAll();
    state.activeCollection = collection;
    Object.assign(state, finishEditorFlow());
    render();
  } catch (error) {
    showToast('Save failed', error.message, 'danger');
  }
}

async function deleteResource(collection, id) {
  try {
    const response = await fetch(`${COLLECTIONS[collection].path}/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      headers: adminHeaders(),
    });
    if (response.status === 401) {
      handleUnauthorized();
      throw new Error('Invalid admin key. Please try again.');
    }
    if (!response.ok) {
      throw new Error(await extractError(response));
    }
    const result = await response.json();
    state.flashRevision = result.revision;
    showToast('Deleted successfully', `${collection} '${id}' deleted at revision ${result.revision}.`, 'success');
    closeModal();
    await refreshAll();
    Object.assign(state, finishEditorFlow());
    render();
  } catch (error) {
    showToast('Delete failed', error.message, 'danger');
  }
}

function showDeleteModal(collection, id) {
  const impact = buildDeleteImpact(collection, id, state.data);
  modalRoot.classList.add('open');
  modalRoot.innerHTML = `
    <div class="modal" role="dialog" aria-modal="true" aria-labelledby="delete-title">
      <div class="detail-header">
        <div>
          <h2 id="delete-title">Confirm delete</h2>
          <p class="muted">Stored resources may remain referenced until dependent config is updated.</p>
        </div>
        <button class="ghost-button" type="button" id="close-delete-button">Close</button>
      </div>
      <div class="impact-list">
        <div class="impact-item"><strong>${escapeHtml(impact.title)}</strong></div>
        ${impact.lines.map((line) => `<div class="impact-item">${escapeHtml(line)}</div>`).join('')}
      </div>
      <div class="form-actions" style="margin-top: 18px;">
        <button class="secondary-button danger-button" type="button" id="confirm-delete-button">Delete ${escapeHtml(id)}</button>
        <button class="secondary-button" type="button" id="cancel-delete-button">Cancel</button>
      </div>
    </div>
  `;
  modalRoot.querySelector('#close-delete-button')?.addEventListener('click', closeModal);
  modalRoot.querySelector('#cancel-delete-button')?.addEventListener('click', closeModal);
  modalRoot.querySelector('#confirm-delete-button')?.addEventListener('click', () => {
    void deleteResource(collection, id);
  });
}

function closeModal() {
  modalRoot.classList.remove('open');
  modalRoot.innerHTML = '';
}

function showToast(title, message, tone) {
  const toast = document.createElement('div');
  toast.className = 'toast';
  toast.innerHTML = `<strong>${escapeHtml(title)}</strong><div class="muted">${escapeHtml(message)}</div>`;
  if (tone === 'danger') {
    toast.style.borderColor = '#fecaca';
  }
  if (tone === 'success') {
    toast.style.borderColor = '#bbf7d0';
  }
  toastRoot.prepend(toast);
  setTimeout(() => {
    toast.remove();
  }, 3600);
}

function adminHeaders() {
  const headers = new Headers();
  if (state.adminKey) {
    headers.set('x-admin-key', state.adminKey);
  }
  return headers;
}

export function buildJsonAdminHeaders(adminKey) {
  const headers = new Headers();
  if (adminKey) {
    headers.set('x-admin-key', adminKey);
  }
  headers.set('content-type', 'application/json');
  return headers;
}

function handleUnauthorized() {
  state.adminKeyValid = false;
  state.connectionState = 'idle';
  state.adminKeyError = 'Admin key expired or is invalid. Please enter it again.';
  render();
}

export async function validateAdminKey(adminKey, fetchImpl = fetch) {
  try {
    const response = await fetchImpl(COLLECTIONS.providers.path, {
      headers: { 'x-admin-key': adminKey.trim() },
    });

    if (response.ok) {
      return { valid: true, message: '' };
    }

    if (response.status === 401) {
      return { valid: false, message: 'Invalid admin key. Please try again.' };
    }

    return { valid: false, message: `Admin API validation failed: ${await extractError(response)}` };
  } catch (error) {
    return { valid: false, message: `Admin API validation failed: ${error.message}` };
  }
}

async function extractError(response) {
  const contentType = response.headers.get('content-type') ?? '';
  if (contentType.includes('application/json')) {
    const data = await response.json();
    return data.error?.message ?? JSON.stringify(data);
  }
  return response.text();
}

function filterItems(items, collection) {
  return items.filter((item) => {
    if (state.filter !== 'all' && item.status.kind !== state.filter) {
      return false;
    }
    const haystack = [
      item.id,
      item.summary,
      item.status.message,
      ...item.dependsOn.map((relation) => relation.id),
      ...item.referencedBy.map((relation) => relation.id),
    ]
      .join(' ')
      .toLowerCase();
    return haystack.includes(state.search.toLowerCase());
  });
}

function formatColumnValue(collection, key, raw, item) {
  if (key === 'status') {
    return item.status.label;
  }
  if (collection === 'apikeys' && key === 'key') {
    return state.revealMap.get(item.id) ? raw.key : maskApiKey(raw.key);
  }
  if (collection === 'apikeys' && key === 'allowed_models') {
    return raw.allowed_models.join(', ') || 'none';
  }
  if (collection === 'policies' && key === 'rate_limit') {
    return formatRateLimit(raw.rate_limit);
  }
  if (collection === 'policies' && key === 'policy_scope') {
    return item.referencedBy.length ? `${item.referencedBy.length} resources` : 'No references';
  }
  if (collection === 'policies' && key === 'policy_scope_detail') {
    return item.referencedBy.map((relation) => relation.id).join(', ') || 'none';
  }
  return String(raw[key] ?? 'none');
}

function formatRateLimit(rateLimit) {
  if (!rateLimit) {
    return 'inherit / none';
  }
  const segments = [];
  if (rateLimit.rpm != null) segments.push(`rpm ${rateLimit.rpm}`);
  if (rateLimit.tpm != null) segments.push(`tpm ${rateLimit.tpm}`);
  if (rateLimit.concurrency != null) segments.push(`cc ${rateLimit.concurrency}`);
  return segments.length ? segments.join(', ') : 'inherit / none';
}

function badgeClassForConnection() {
  if (state.connectionState === 'error') return 'danger';
  if (state.connectionState === 'ready') return 'success';
  if (state.connectionState === 'loading') return 'warning';
  return '';
}

function connectionText() {
  if (state.connectionState === 'loading') return 'Refreshing...';
  if (state.connectionState === 'ready') return 'Admin API reachable';
  if (state.connectionState === 'error') return 'Admin API error';
  return 'Waiting for admin key';
}

function statusBadgeMarkup(status) {
  return `<span class="badge ${status.kind === 'valid' ? 'success' : status.kind === 'missing_dependency' ? 'warning' : 'danger'}">${escapeHtml(status.label)}</span>`;
}

function formatTimestamp(timestamp) {
  return new Intl.DateTimeFormat(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(timestamp));
}

function escapeHtml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function parseOptionalNumber(value) {
  if (value == null || value === '') {
    return null;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function buildRateLimit(values) {
  const rpm = parseOptionalNumber(values.rate_limit_rpm);
  const tpm = parseOptionalNumber(values.rate_limit_tpm);
  const concurrency = parseOptionalNumber(values.rate_limit_concurrency);
  if (rpm == null && tpm == null && concurrency == null) {
    return null;
  }
  return { rpm, tpm, concurrency };
}

export function buildResourcePayload(collection, values) {
  if (collection === 'providers') {
    return {
      id: values.id.trim(),
      kind: values.kind,
      base_url: values.base_url.trim(),
      auth: { secret_ref: values.secret_ref.trim() },
      policy_id: values.policy_id?.trim() || null,
      rate_limit: buildRateLimit(values),
      cache: values.cache_mode ? { mode: values.cache_mode } : null,
    };
  }
  if (collection === 'models') {
    return {
      id: values.id.trim(),
      provider_id: values.provider_id.trim(),
      upstream_model: values.upstream_model.trim(),
      policy_id: values.policy_id?.trim() || null,
      rate_limit: buildRateLimit(values),
      cache: values.cache_mode ? { mode: values.cache_mode } : null,
    };
  }
  if (collection === 'apikeys') {
    return {
      id: values.id.trim(),
      key: values.key.trim(),
      allowed_models: values.allowed_models
        .split(/[,\n]/)
        .map((value) => value.trim())
        .filter(Boolean),
      policy_id: values.policy_id?.trim() || null,
      rate_limit: buildRateLimit(values),
    };
  }
  return {
    id: values.id.trim(),
    rate_limit: {
      rpm: parseOptionalNumber(values.rate_limit_rpm),
      tpm: parseOptionalNumber(values.rate_limit_tpm),
      concurrency: parseOptionalNumber(values.rate_limit_concurrency),
    },
  };
}

export function nextAdminRefreshState(adminKey) {
  if (!adminKey || !adminKey.trim()) {
    return { shouldRefresh: false, connectionState: 'idle' };
  }
  return { shouldRefresh: true, connectionState: 'loading' };
}

export function nextAdminUiMode({ adminKey, adminKeyValid, draftMode }) {
  if (!adminKey || !adminKey.trim() || !adminKeyValid) {
    return { locked: true, mode: 'locked' };
  }

  return {
    locked: false,
    mode: draftMode ? 'editing' : 'listing',
  };
}

export function nextDetailMode({ draftMode, editingId }) {
  return draftMode === 'create' || (draftMode === 'edit' && editingId) ? 'editing' : 'listing';
}

export function startEditAction(collection, raw) {
  return {
    draftMode: 'edit',
    editorCollection: collection,
    editorId: raw.id,
    editorValues: toFormValues(collection, raw),
  };
}

export function startCreateAction(collection) {
  return {
    draftMode: 'create',
    editorCollection: collection,
    editorId: null,
    editorValues: defaultFormValues(collection),
  };
}

export function buildRowActions(collection, id) {
  return [
    { kind: 'edit', collection, id },
    { kind: 'delete', collection, id },
  ];
}

export function buildReferenceOptions(collection, data) {
  if (collection === 'models') {
    return {
      provider_id: data.providers.map((item) => item.id),
      policy_id: data.policies.map((item) => item.id),
    };
  }
  if (collection === 'providers') {
    return { policy_id: data.policies.map((item) => item.id) };
  }
  if (collection === 'apikeys') {
    return { policy_id: data.policies.map((item) => item.id) };
  }
  return {};
}

export function finishEditorFlow() {
  return {
    draftMode: null,
    editorCollection: null,
    editorId: null,
    editorValues: null,
  };
}

export function adminKeyStorageMode() {
  return 'session';
}

export function defaultFormValues(collection, fields = fieldsForCollection(collection)) {
  const values = {};
  for (const field of fields) {
    values[field.name] = '';
  }
  return values;
}

export function extractAdminPutSchema(spec, collection) {
  return spec?.paths?.[ADMIN_PUT_SCHEMA_PATHS[collection]]?.put?.requestBody?.content?.['application/json']?.schema ?? null;
}

export function buildSchemaBackedFieldDefinitions(collection, schema) {
  if (!schema?.properties) {
    return COLLECTIONS[collection].form;
  }

  const required = new Set(schema.required ?? []);
  const fields = [];

  function addField(name, label, property, override = {}) {
    if (!property) {
      return;
    }
    const normalized = unwrapNullableSchema(property);
    const options = normalized?.enum;
    fields.push({
      name,
      label,
      type: Array.isArray(options) ? 'select' : override.type ?? 'text',
      options,
      required: override.required ?? required.has(override.requiredKey ?? name),
      source: override.source,
    });
  }

  if (collection === 'providers') {
    addField('id', 'Provider ID', schema.properties.id);
    addField('kind', 'Kind', schema.properties.kind);
    addField('base_url', 'Base URL', schema.properties.base_url);
    const authSchema = unwrapNullableSchema(schema.properties.auth);
    addField('secret_ref', 'Secret Ref', authSchema?.properties?.secret_ref, {
      required: (authSchema?.required ?? []).includes('secret_ref'),
    });
    addField('policy_id', 'Policy ID', schema.properties.policy_id, { type: 'reference', source: 'policies' });
    const cacheSchema = unwrapNullableSchema(schema.properties.cache);
    addField('cache_mode', 'Cache Mode', cacheSchema?.properties?.mode);
    return fields;
  }

  if (collection === 'models') {
    addField('id', 'Model ID', schema.properties.id);
    addField('provider_id', 'Provider ID', schema.properties.provider_id, { type: 'reference', source: 'providers' });
    addField('upstream_model', 'Upstream Model', schema.properties.upstream_model);
    addField('policy_id', 'Policy ID', schema.properties.policy_id, { type: 'reference', source: 'policies' });
    const cacheSchema = unwrapNullableSchema(schema.properties.cache);
    addField('cache_mode', 'Cache Mode', cacheSchema?.properties?.mode);
    return fields;
  }

  if (collection === 'apikeys') {
    addField('id', 'Key ID', schema.properties.id);
    addField('key', 'Plaintext Key', schema.properties.key);
    fields.push({ name: 'allowed_models', label: 'Allowed Models (comma separated)', type: 'textarea', required: required.has('allowed_models') });
    addField('policy_id', 'Policy ID', schema.properties.policy_id, { type: 'reference', source: 'policies' });
    return fields;
  }

  addField('id', 'Policy ID', schema.properties.id);
  fields.push({ name: 'rate_limit_rpm', label: 'RPM', type: 'number', required: true });
  fields.push({ name: 'rate_limit_tpm', label: 'TPM', type: 'number' });
  fields.push({ name: 'rate_limit_concurrency', label: 'Concurrency', type: 'number' });
  return fields;
}

function unwrapNullableSchema(schema) {
  if (!schema?.anyOf) {
    return schema;
  }
  return schema.anyOf.find((candidate) => candidate.type !== 'null') ?? schema.anyOf[0] ?? schema;
}

function fieldsForCollection(collection) {
  return state.schemaFields[collection] ?? COLLECTIONS[collection].form;
}

function toFormValues(collection, raw) {
  if (!raw) {
    return defaultFormValues(collection);
  }
  if (collection === 'providers') {
    return {
      id: raw.id ?? '',
      kind: raw.kind ?? 'openai',
      base_url: raw.base_url ?? '',
      secret_ref: raw.auth?.secret_ref ?? '',
      policy_id: raw.policy_id ?? '',
      cache_mode: raw.cache?.mode ?? '',
      rate_limit_rpm: raw.rate_limit?.rpm ?? '',
      rate_limit_tpm: raw.rate_limit?.tpm ?? '',
      rate_limit_concurrency: raw.rate_limit?.concurrency ?? '',
    };
  }
  if (collection === 'models') {
    return {
      id: raw.id ?? '',
      provider_id: raw.provider_id ?? '',
      upstream_model: raw.upstream_model ?? '',
      policy_id: raw.policy_id ?? '',
      cache_mode: raw.cache?.mode ?? '',
      rate_limit_rpm: raw.rate_limit?.rpm ?? '',
      rate_limit_tpm: raw.rate_limit?.tpm ?? '',
      rate_limit_concurrency: raw.rate_limit?.concurrency ?? '',
    };
  }
  if (collection === 'apikeys') {
    return {
      id: raw.id ?? '',
      key: raw.key ?? '',
      allowed_models: raw.allowed_models?.join(', ') ?? '',
      policy_id: raw.policy_id ?? '',
      rate_limit_rpm: raw.rate_limit?.rpm ?? '',
      rate_limit_tpm: raw.rate_limit?.tpm ?? '',
      rate_limit_concurrency: raw.rate_limit?.concurrency ?? '',
    };
  }
  return {
    id: raw.id ?? '',
    rate_limit_rpm: raw.rate_limit?.rpm ?? '',
    rate_limit_tpm: raw.rate_limit?.tpm ?? '',
    rate_limit_concurrency: raw.rate_limit?.concurrency ?? '',
  };
}

function relationStatus(kind, message) {
  const labelMap = {
    valid: 'Valid',
    missing_dependency: 'Missing dependency',
    orphaned: 'Orphaned',
  };
  return { kind, label: labelMap[kind], message };
}

export function maskApiKey(secret) {
  if (!secret) {
    return '****';
  }
  if (secret.length <= 4) {
    return '*'.repeat(secret.length);
  }
  if (secret.length <= 8) {
    return `${secret.slice(0, 2)}...${secret.slice(-2)}`;
  }
  return `${secret.slice(0, 4)}...${secret.slice(-4)}`;
}

function makeDerivedItem(collection, raw, summary) {
  return {
    id: raw.id,
    collection,
    raw,
    summary,
    dependsOn: [],
    referencedBy: [],
    status: relationStatus('valid', 'Stored and dependency-valid based on current admin resources.'),
  };
}

function addRelation(targetList, relation) {
  targetList.push(relation);
}

function makeRelation(collection, id, description, status) {
  return {
    collection,
    id,
    label: `${COLLECTIONS[collection].label.slice(0, -1) || collection} · ${id}`,
    description,
    status,
  };
}

function isRuntimeUsable(item) {
  return item.status.kind === 'valid';
}

export function deriveRelationshipModel(data) {
  const derived = {
    providers: { items: [], byId: {} },
    models: { items: [], byId: {} },
    apikeys: { items: [], byId: {} },
    policies: { items: [], byId: {} },
  };

  for (const provider of data.providers) {
    const item = makeDerivedItem('providers', provider, `${provider.kind} ${provider.base_url}`);
    derived.providers.items.push(item);
    derived.providers.byId[item.id] = item;
  }
  for (const model of data.models) {
    const item = makeDerivedItem('models', model, `${model.provider_id} -> ${model.upstream_model}`);
    derived.models.items.push(item);
    derived.models.byId[item.id] = item;
  }
  for (const apikey of data.apikeys) {
    const item = makeDerivedItem('apikeys', apikey, `${apikey.allowed_models.length} allowed models`);
    derived.apikeys.items.push(item);
    derived.apikeys.byId[item.id] = item;
  }
  for (const policy of data.policies) {
    const item = makeDerivedItem('policies', policy, formatRateLimit(policy.rate_limit));
    derived.policies.items.push(item);
    derived.policies.byId[item.id] = item;
  }

  const policyItems = derived.policies.byId;
  const providerItems = derived.providers.byId;
  const modelItems = derived.models.byId;

  for (const item of derived.providers.items) {
    if (item.raw.policy_id) {
      const policy = policyItems[item.raw.policy_id];
      const status = policy
        ? relationStatus('valid', 'Referenced policy exists.')
        : relationStatus('missing_dependency', `Policy '${item.raw.policy_id}' is missing, so this provider is excluded from runtime.`);
      addRelation(item.dependsOn, makeRelation('policies', item.raw.policy_id, 'Provider policy', status));
      if (policy) {
        addRelation(policy.referencedBy, makeRelation('providers', item.id, 'Provider references this policy', relationStatus('valid', 'Active reference.')));
      } else {
        item.status = relationStatus('missing_dependency', `Stored in etcd, currently excluded from runtime because policy '${item.raw.policy_id}' is missing.`);
      }
    }
  }

  for (const item of derived.models.items) {
    const provider = providerItems[item.raw.provider_id];
    const providerStatus = provider
      ? relationStatus('valid', 'Referenced provider exists.')
      : relationStatus('missing_dependency', `Provider '${item.raw.provider_id}' is missing, so this model is excluded from runtime.`);
    addRelation(item.dependsOn, makeRelation('providers', item.raw.provider_id, 'Target provider', providerStatus));
    if (provider) {
      addRelation(provider.referencedBy, makeRelation('models', item.id, 'Model depends on this provider', relationStatus('valid', 'Active dependency.')));
    } else {
      item.status = relationStatus('missing_dependency', `Stored in etcd, currently excluded from runtime because provider '${item.raw.provider_id}' is missing.`);
    }

    if (item.raw.policy_id) {
      const policy = policyItems[item.raw.policy_id];
      const policyStatus = policy
        ? relationStatus('valid', 'Referenced policy exists.')
        : relationStatus('missing_dependency', `Policy '${item.raw.policy_id}' is missing, so this model is excluded from runtime.`);
      addRelation(item.dependsOn, makeRelation('policies', item.raw.policy_id, 'Model policy', policyStatus));
      if (policy) {
        addRelation(policy.referencedBy, makeRelation('models', item.id, 'Model references this policy', relationStatus('valid', 'Active reference.')));
      } else {
        item.status = relationStatus('missing_dependency', `Stored in etcd, currently excluded from runtime because policy '${item.raw.policy_id}' is missing.`);
      }
    }
  }

  for (const item of derived.apikeys.items) {
    let hasMissingModel = false;
    for (const modelId of item.raw.allowed_models) {
      const model = modelItems[modelId];
      const status = !model
        ? relationStatus('missing_dependency', `Allowed model '${modelId}' is missing, so this API key is excluded from runtime.`)
        : isRuntimeUsable(model)
          ? relationStatus('valid', 'Allowed model exists and is runtime-valid.')
          : relationStatus('missing_dependency', `Allowed model '${modelId}' exists in storage but is currently excluded from runtime.`);
      addRelation(item.dependsOn, makeRelation('models', modelId, 'Allowed model', status));
      if (model) {
        addRelation(model.referencedBy, makeRelation('apikeys', item.id, 'API key allows this model', relationStatus('valid', 'Active allowlist reference.')));
      }
      if (!model || !isRuntimeUsable(model)) {
        hasMissingModel = true;
      }
    }
    if (item.raw.policy_id) {
      const policy = policyItems[item.raw.policy_id];
      const policyStatus = policy
        ? relationStatus('valid', 'Referenced policy exists.')
        : relationStatus('missing_dependency', `Policy '${item.raw.policy_id}' is missing, so this API key is excluded from runtime.`);
      addRelation(item.dependsOn, makeRelation('policies', item.raw.policy_id, 'Key policy', policyStatus));
      if (policy) {
        addRelation(policy.referencedBy, makeRelation('apikeys', item.id, 'API key references this policy', relationStatus('valid', 'Active reference.')));
      } else {
        hasMissingModel = true;
      }
    }
    if (hasMissingModel) {
      item.status = relationStatus('missing_dependency', 'Stored in etcd, currently excluded from runtime because one or more dependencies are missing.');
    }
  }

  for (const item of derived.policies.items) {
    if (!item.referencedBy.length) {
      item.status = relationStatus('orphaned', 'Stored and valid, but currently unused by providers, models, or API keys.');
    }
  }

  return derived;
}

export function buildDeleteImpact(collection, id, data) {
  const derived = deriveRelationshipModel(data);
  const item = derived[collection].byId[id];
  const lines = [];

  for (const relation of item?.referencedBy ?? []) {
    const label = COLLECTIONS[relation.collection].label;
    lines.push(`${label}: ${relation.id}`);
  }

  if (!lines.length) {
    lines.push('No known references will remain after delete.');
  }

  return {
    title: `Delete ${collection} '${id}'`,
    lines,
  };
}

if (hasBrowserDom) {
  init();
}
