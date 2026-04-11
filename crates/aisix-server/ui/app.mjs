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
      { name: 'policy_id', label: 'Policy ID', type: 'text' },
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
      { name: 'provider_id', label: 'Provider ID', type: 'text', required: true },
      { name: 'upstream_model', label: 'Upstream Model', type: 'text', required: true },
      { name: 'policy_id', label: 'Policy ID', type: 'text' },
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
      { name: 'policy_id', label: 'Policy ID', type: 'text' },
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

function adminKeyStorage() {
  if (adminKeyStorageMode() !== 'session' || !hasSessionStorage) {
    return null;
  }
  return sessionStorage;
}

const state = {
  adminKey: adminKeyStorage()?.getItem(ADMIN_KEY_STORAGE_KEY) ?? '',
  adminKeyValid: false,
  adminKeyError: '',
  isValidatingAdminKey: false,
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
  selectedId: null,
  formMode: 'create',
  editingId: null,
  draftMode: null,
  editorCollection: null,
  editorValues: null,
  editorId: null,
  revealMap: new Map(),
  connectionState: 'idle',
  lastRefreshed: null,
  flashRevision: null,
};

const appRoot = hasBrowserDom ? document.querySelector('#app') : null;
const modalRoot = hasBrowserDom ? document.querySelector('#modal-root') : null;
const toastRoot = hasBrowserDom ? document.querySelector('#toast-root') : null;

function init() {
  if (!hasBrowserDom || !appRoot || !modalRoot || !toastRoot) {
    return;
  }
  render();
}

function render() {
  const derived = state.derived ?? deriveRelationshipModel(state.data);
  state.derived = derived;
  const collection = state.activeCollection;
  const resourceConfig = COLLECTIONS[collection];
  const derivedCollection = derived[collection];
  const items = filterItems(derivedCollection.items, collection);
  const selected = state.selectedId ? derivedCollection.byId[state.selectedId] : items[0] ?? null;
  if (!state.selectedId && selected) {
    state.selectedId = selected.id;
  }

  appRoot.innerHTML = `
    <div class="layout">
      <aside class="sidebar">
        <div class="brand">
          <h1>AISIX Control Plane</h1>
          <p>Embedded admin UI for stored config, relationships, and runtime validity hints.</p>
        </div>
        <div class="nav" style="margin-top: 18px;">
          ${Object.entries(COLLECTIONS)
            .map(
              ([key, value]) => `
                <button class="${key === collection ? 'active' : ''}" data-nav="${key}" type="button">
                  <span>${value.label}</span>
                  <span class="count">${derived[key].items.length}</span>
                </button>
              `,
            )
            .join('')}
        </div>
      </aside>
      <main class="main">
        <section class="status-bar">
          <div>
            <strong>${resourceConfig.label}</strong>
            <div class="muted">Current endpoint: ${resourceConfig.path}</div>
          </div>
          <div class="status-grid">
            <span class="badge ${badgeClassForConnection()}">${connectionText()}</span>
            <span class="badge">Stored vs Runtime Semantics</span>
            <span class="badge">Last refresh: ${state.lastRefreshed ? formatTimestamp(state.lastRefreshed) : 'never'}</span>
            ${state.flashRevision ? `<span class="badge success">Revision ${state.flashRevision}</span>` : ''}
          </div>
        </section>
        <section class="workspace">
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
          <div class="detail-card">
            ${selected ? renderDetail(collection, selected) : renderEmptyDetail(resourceConfig.label)}
          </div>
        </section>
      </main>
      ${!state.adminKeyValid ? renderAdminKeyGate() : ''}
    </div>
  `;

  bindGlobalEvents();
  bindTableEvents(items);
  bindDetailEvents(selected);
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
      <div class="table-header">${columns.map((column) => `<div>${column.label}</div>`).join('')}</div>
      ${
        items.length
          ? items
              .map((item) => {
                const raw = item.raw;
                return `
                  <div class="table-row ${item.id === state.selectedId ? 'selected' : ''}" data-row-id="${item.id}" tabindex="0">
                    ${columns
                      .map((column) => `<button type="button" data-row-select="${item.id}">${escapeHtml(formatColumnValue(collection, column.key, raw, item))}</button>`)
                      .join('')}
                  </div>
                `;
              })
              .join('')
          : `<div class="table-empty">${COLLECTIONS[collection].empty}</div>`
      }
    </div>
  `;
}

function renderEmptyDetail(label) {
  return `
    <div class="detail-header">
      <div>
        <h2>No ${label.toLowerCase()} selected</h2>
        <p class="muted">Choose a row or create a new resource to begin.</p>
      </div>
    </div>
  `;
}

function renderDetail(collection, item) {
  const formValues = toFormValues(collection, item.raw);
  const statusBadge = statusBadgeMarkup(item.status);
  return `
    <div class="detail-sections">
      <div class="detail-header">
        <div>
          <div class="muted">${COLLECTIONS[collection].label.slice(0, -1) || COLLECTIONS[collection].label}</div>
          <h2>${escapeHtml(item.id)}</h2>
          <div class="status-line">${statusBadge}</div>
        </div>
        <div class="detail-actions">
          ${collection === 'apikeys' ? `<button class="secondary-button" type="button" id="toggle-secret-button">${state.revealMap.get(item.id) ? 'Hide key' : 'Reveal key'}</button>` : ''}
          <button class="secondary-button" type="button" id="refresh-one-button">Refresh</button>
        </div>
      </div>
      <section>
        <div class="split-line">
          <h3>Summary</h3>
          <span class="muted">Stored in admin config</span>
        </div>
        ${renderSummary(collection, item)}
      </section>
      <section>
        <h3>Relations</h3>
        <div class="relation-list">
          <div class="relation-item">
            <strong>Depends on</strong>
            ${item.dependsOn.length ? item.dependsOn.map(renderRelationItem).join('') : '<p class="muted">No direct dependencies.</p>'}
          </div>
          <div class="relation-item">
            <strong>Referenced by</strong>
            ${item.referencedBy.length ? item.referencedBy.map(renderRelationItem).join('') : '<p class="muted">No reverse references.</p>'}
          </div>
        </div>
      </section>
      <section>
        <div class="split-line">
          <h3>${state.formMode === 'edit' && state.editingId === item.id ? 'Edit resource' : 'Edit resource'}</h3>
          <span class="muted">PUT ${COLLECTIONS[collection].path}/:id</span>
        </div>
        <form id="resource-form" class="form-grid">
          ${renderFormFields(collection, formValues, { readonlyId: true })}
          <div class="form-actions">
            <button class="button" type="submit">Save ${COLLECTIONS[collection].label.slice(0, -1) || COLLECTIONS[collection].label}</button>
            <button class="secondary-button" type="button" id="clone-button">Clone into create</button>
            <button class="secondary-button danger-button" type="button" id="delete-button">Delete</button>
          </div>
        </form>
      </section>
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

function renderRelationItem(relation) {
  return `
    <div class="relation-item" style="margin-top: 10px;">
      <div class="split-line">
        <button class="detail-link" type="button" data-jump-collection="${relation.collection}" data-jump-id="${relation.id}">${escapeHtml(relation.label)}</button>
        ${statusBadgeMarkup(relation.status)}
      </div>
      <div class="muted">${escapeHtml(relation.description)}</div>
    </div>
  `;
}

function renderFormFields(collection, values, options = {}) {
  const fields = COLLECTIONS[collection].form.map((field) => {
    if (options.readonlyId && field.name === 'id') {
      return { ...field, readonly: true };
    }
    return field;
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

function renderField(field, value) {
  const required = field.required ? 'required' : '';
  const readonly = field.readonly ? 'readonly' : '';
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

  document.querySelectorAll('[data-nav]').forEach((button) => {
    button.addEventListener('click', () => {
      state.activeCollection = button.dataset.nav;
      state.selectedId = null;
      state.formMode = 'edit';
      state.editingId = null;
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

  document.querySelector('#create-button')?.addEventListener('click', () => {
    state.selectedId = null;
    showCreateModal(state.activeCollection);
  });
}

function bindTableEvents(items) {
  document.querySelectorAll('[data-row-select]').forEach((button) => {
    button.addEventListener('click', () => {
      state.selectedId = button.dataset.rowSelect;
      render();
    });
  });

  document.querySelectorAll('[data-row-id]').forEach((row) => {
    row.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        state.selectedId = row.dataset.rowId;
        render();
      }
    });
  });

  if (!items.some((item) => item.id === state.selectedId)) {
    state.selectedId = items[0]?.id ?? null;
  }
}

function bindDetailEvents(selected) {
  if (!selected) {
    return;
  }

  document.querySelector('#toggle-secret-button')?.addEventListener('click', () => {
    const current = state.revealMap.get(selected.id) ?? false;
    state.revealMap.set(selected.id, !current);
    render();
  });

  document.querySelector('#refresh-one-button')?.addEventListener('click', () => {
    void refreshAll();
  });

  document.querySelector('#clone-button')?.addEventListener('click', () => {
    showCreateModal(state.activeCollection, toFormValues(state.activeCollection, selected.raw));
  });

  document.querySelector('#delete-button')?.addEventListener('click', () => {
    showDeleteModal(state.activeCollection, selected.id);
  });

  document.querySelector('#resource-form')?.addEventListener('submit', (event) => {
    event.preventDefault();
    const formData = new FormData(event.currentTarget);
    const values = Object.fromEntries(formData.entries());
    void saveResource(state.activeCollection, values, selected.id);
  });

  document.querySelectorAll('[data-jump-collection]').forEach((button) => {
    button.addEventListener('click', () => {
      state.activeCollection = button.dataset.jumpCollection;
      state.selectedId = button.dataset.jumpId;
      render();
    });
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

async function saveResource(collection, values, currentId = null) {
  try {
    const payload = buildResourcePayload(collection, values);
    const id = payload.id ?? currentId;
    const response = await fetch(`${COLLECTIONS[collection].path}/${encodeURIComponent(id)}`, {
      method: 'PUT',
      headers: {
        ...adminHeaders(),
        'content-type': 'application/json',
      },
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
    state.selectedId = id;
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
    state.selectedId = null;
    render();
  } catch (error) {
    showToast('Delete failed', error.message, 'danger');
  }
}

function showCreateModal(collection, initialValues = null) {
  const defaults = initialValues ?? defaultFormValues(collection);
  modalRoot.classList.add('open');
  modalRoot.innerHTML = `
    <div class="modal" role="dialog" aria-modal="true" aria-labelledby="create-title">
      <div class="detail-header">
        <div>
          <h2 id="create-title">Create ${COLLECTIONS[collection].label.slice(0, -1) || COLLECTIONS[collection].label}</h2>
          <p class="muted">Writes directly to ${COLLECTIONS[collection].path}/:id using the current admin key.</p>
        </div>
        <button class="ghost-button" type="button" id="close-modal-button">Close</button>
      </div>
      <form id="create-form" class="form-grid">
        ${renderFormFields(collection, defaults)}
        <div class="form-actions">
          <button class="button" type="submit">Create</button>
          <button class="secondary-button" type="button" id="cancel-create-button">Cancel</button>
        </div>
      </form>
    </div>
  `;
  modalRoot.querySelector('#close-modal-button')?.addEventListener('click', closeModal);
  modalRoot.querySelector('#cancel-create-button')?.addEventListener('click', closeModal);
  modalRoot.querySelector('#create-form')?.addEventListener('submit', (event) => {
    event.preventDefault();
    const formData = new FormData(event.currentTarget);
    const values = Object.fromEntries(formData.entries());
    closeModal();
    void saveResource(collection, values);
  });
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

function handleUnauthorized() {
  state.adminKeyValid = false;
  state.connectionState = 'idle';
  state.adminKeyError = 'Admin key expired or is invalid. Please enter it again.';
  render();
}

export async function validateAdminKey(adminKey, fetchImpl = fetch) {
  const response = await fetchImpl('/admin/providers', {
    headers: { 'x-admin-key': adminKey.trim() },
  });

  if (response.ok) {
    return { valid: true, message: '' };
  }

  if (response.status === 401) {
    return { valid: false, message: 'Invalid admin key. Please try again.' };
  }

  return { valid: false, message: `Admin API validation failed: ${await extractError(response)}` };
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

export function adminKeyStorageMode() {
  return 'session';
}

function defaultFormValues(collection) {
  const values = {};
  for (const field of COLLECTIONS[collection].form) {
    values[field.name] = '';
  }
  return values;
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
