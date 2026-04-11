# Admin UI List Flow Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework the embedded Admin UI so it uses a left-nav plus right-content layout, blocks on admin key validation, removes the Relations pane, and moves resource editing into an explicit list-to-editor flow.

**Architecture:** Keep the current embedded UI as a single browser app in `crates/aisix-server/ui/app.mjs`, but change its state model from “always-on detail panel” to explicit `locked`, `listing`, and `editing` UI states. Reuse the existing admin API contract and relationship derivation data, but stop rendering the relationship pane and instead use derived data only for status labels, delete impact messaging, search text, and reference option generation.

**Tech Stack:** Vanilla browser JavaScript, static HTML/CSS, existing axum-served embedded assets, Node built-in test runner.

---

### Task 1: Add failing UI state helper tests for locked and editor flows

**Files:**
- Modify: `crates/aisix-server/ui/app.test.mjs`
- Modify: `crates/aisix-server/ui/app.mjs`

- [ ] **Step 1: Write the failing tests for UI state helpers**

```javascript
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: FAIL because `nextAdminUiMode` and `nextDetailMode` do not exist.

- [ ] **Step 3: Add the minimal state helper exports**

```javascript
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/aisix-server/ui/app.test.mjs crates/aisix-server/ui/app.mjs
git commit -m "test: add admin ui mode helpers"
```

### Task 2: Add blocking admin key validation flow

**Files:**
- Modify: `crates/aisix-server/ui/app.mjs`
- Modify: `crates/aisix-server/ui/app.test.mjs`

- [ ] **Step 1: Write the failing validation behavior tests**

```javascript
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: FAIL because `validateAdminKey` does not exist.

- [ ] **Step 3: Add minimal validation helper and locked state fields**

```javascript
state.adminKeyValid = false;
state.adminKeyError = '';
state.isValidatingAdminKey = false;
state.draftMode = null;
state.editorCollection = null;
state.editorValues = null;
state.editorId = null;
```

```javascript
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
```

- [ ] **Step 4: Replace sidebar key input with blocking modal rendering**

```javascript
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
            <button class="button" type="submit">Validate</button>
          </div>
        </form>
      </div>
    </div>
  `;
}
```

- [ ] **Step 5: Bind form submission to validation before refresh**

```javascript
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
```

- [ ] **Step 6: Re-lock on 401s from collection/load/save/delete requests**

```javascript
function handleUnauthorized() {
  state.adminKeyValid = false;
  state.connectionState = 'idle';
  state.adminKeyError = 'Admin key expired or is invalid. Please enter it again.';
  render();
}
```

Call `handleUnauthorized()` when `response.status === 401` in `fetchCollection`, `saveResource`, and `deleteResource`.

- [ ] **Step 7: Run tests to verify they pass**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add crates/aisix-server/ui/app.mjs crates/aisix-server/ui/app.test.mjs
git commit -m "feat: gate admin ui behind key validation"
```

### Task 3: Replace always-on detail pane with explicit list and editor views

**Files:**
- Modify: `crates/aisix-server/ui/app.mjs`
- Modify: `crates/aisix-server/ui/index.html`

- [ ] **Step 1: Write the failing view-mode tests**

```javascript
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: FAIL because `startEditAction` and `startCreateAction` do not exist.

- [ ] **Step 3: Add minimal editor state helpers**

```javascript
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
```

- [ ] **Step 4: Replace `showCreateModal` usage with right-panel editor mode**

```javascript
document.querySelector('#create-button')?.addEventListener('click', () => {
  Object.assign(state, startCreateAction(state.activeCollection));
  render();
});
```

```javascript
function closeEditor() {
  state.draftMode = null;
  state.editorCollection = null;
  state.editorId = null;
  state.editorValues = null;
}
```

- [ ] **Step 5: Render the right side as list view or editor view only**

```javascript
const uiMode = nextAdminUiMode({
  adminKey: state.adminKey,
  adminKeyValid: state.adminKeyValid,
  draftMode: state.draftMode,
});
```

```javascript
${uiMode.mode === 'editing'
  ? renderEditorView(state.editorCollection, state.editorValues)
  : renderListView(collection, items)}
```

Use the existing `workspace` section, but remove the second always-on detail card column and collapse `grid-template-columns` to a single right content panel.

- [ ] **Step 6: Remove modal-based create flow and keep delete modal only**

Delete `showCreateModal()` and its event binding. Keep `showDeleteModal()` for destructive confirmation.

- [ ] **Step 7: Run tests to verify they pass**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add crates/aisix-server/ui/app.mjs crates/aisix-server/ui/index.html crates/aisix-server/ui/app.test.mjs
git commit -m "feat: switch admin ui to list and editor views"
```

### Task 4: Add list-row action buttons and remove click-to-open details

**Files:**
- Modify: `crates/aisix-server/ui/app.mjs`
- Modify: `crates/aisix-server/ui/index.html`

- [ ] **Step 1: Write the failing row action helper test**

```javascript
test('buildRowActions exposes edit and delete actions only', () => {
  assert.deepEqual(buildRowActions('providers', 'openai'), [
    { kind: 'edit', collection: 'providers', id: 'openai' },
    { kind: 'delete', collection: 'providers', id: 'openai' },
  ]);
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: FAIL because `buildRowActions` does not exist.

- [ ] **Step 3: Add the minimal row action helper**

```javascript
export function buildRowActions(collection, id) {
  return [
    { kind: 'edit', collection, id },
    { kind: 'delete', collection, id },
  ];
}
```

- [ ] **Step 4: Add an Actions column in `renderTable()`**

```javascript
<div class="table-header">
  ${columns.map((column) => `<div>${column.label}</div>`).join('')}
  <div>Actions</div>
</div>
```

```javascript
<div class="table-row" data-row-id="${item.id}">
  ${columns.map(...).join('')}
  <div class="row-actions">
    <button type="button" data-edit-id="${item.id}">Edit</button>
    <button type="button" data-delete-id="${item.id}">Delete</button>
  </div>
</div>
```

- [ ] **Step 5: Bind explicit row action events and stop selecting on row click**

```javascript
document.querySelectorAll('[data-edit-id]').forEach((button) => {
  button.addEventListener('click', () => {
    const item = items.find((candidate) => candidate.id === button.dataset.editId);
    Object.assign(state, startEditAction(state.activeCollection, item.raw));
    render();
  });
});

document.querySelectorAll('[data-delete-id]').forEach((button) => {
  button.addEventListener('click', () => {
    showDeleteModal(state.activeCollection, button.dataset.deleteId);
  });
});
```

Remove `data-row-select` click handling and Enter/Space selection behavior.

- [ ] **Step 6: Run tests to verify they pass**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/aisix-server/ui/app.mjs crates/aisix-server/ui/index.html crates/aisix-server/ui/app.test.mjs
git commit -m "feat: add explicit admin ui row actions"
```

### Task 5: Remove Relations section and keep only compact editor metadata

**Files:**
- Modify: `crates/aisix-server/ui/app.mjs`

- [ ] **Step 1: Write the failing summary helper test**

```javascript
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: FAIL because `renderEditorSummary` does not exist.

- [ ] **Step 3: Extract minimal editor summary renderer without Relations**

```javascript
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
```

Use `renderEditorSummary()` in the editor view and remove the old `Relations` section entirely.

- [ ] **Step 4: Run tests to verify they pass**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/aisix-server/ui/app.mjs crates/aisix-server/ui/app.test.mjs
git commit -m "refactor: remove admin ui relations pane"
```

### Task 6: Add editable reference inputs with predefined suggestions and custom values

**Files:**
- Modify: `crates/aisix-server/ui/app.mjs`
- Modify: `crates/aisix-server/ui/app.test.mjs`

- [ ] **Step 1: Write the failing reference-option tests**

```javascript
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
  assert.match(field, /datalist/);
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: FAIL because reference option support does not exist.

- [ ] **Step 3: Mark reference-capable fields in `COLLECTIONS`**

```javascript
{ name: 'policy_id', label: 'Policy ID', type: 'reference', source: 'policies' }
{ name: 'provider_id', label: 'Provider ID', type: 'reference', source: 'providers', required: true }
```

- [ ] **Step 4: Add reference option builder**

```javascript
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
```

- [ ] **Step 5: Render reference fields as input + datalist**

```javascript
if (field.type === 'reference') {
  const listId = `${field.name}-options`;
  return `
    <label>
      ${escapeHtml(field.label)}
      <input name="${field.name}" list="${listId}" value="${escapeHtml(String(value))}" ${required} ${readonly} />
      <datalist id="${listId}">
        ${(field.options ?? []).map((option) => `<option value="${escapeHtml(option)}"></option>`).join('')}
      </datalist>
    </label>
  `;
}
```

- [ ] **Step 6: Thread the generated options into editor rendering**

Before rendering form fields, merge dynamic `options` onto `reference` fields from `buildReferenceOptions(collection, state.data)`.

- [ ] **Step 7: Run tests to verify they pass**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add crates/aisix-server/ui/app.mjs crates/aisix-server/ui/app.test.mjs
git commit -m "feat: add editable admin ui reference inputs"
```

### Task 7: Save/back behavior should always return to the list

**Files:**
- Modify: `crates/aisix-server/ui/app.mjs`
- Modify: `crates/aisix-server/ui/app.test.mjs`

- [ ] **Step 1: Write the failing navigation helper tests**

```javascript
test('finishEditorFlow resets editor state back to listing', () => {
  assert.deepEqual(finishEditorFlow({
    draftMode: 'edit',
    editorCollection: 'models',
    editorId: 'gpt-4o-mini',
    editorValues: { id: 'gpt-4o-mini' },
  }), {
    draftMode: null,
    editorCollection: null,
    editorId: null,
    editorValues: null,
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: FAIL because `finishEditorFlow` does not exist.

- [ ] **Step 3: Add the minimal editor reset helper**

```javascript
export function finishEditorFlow() {
  return {
    draftMode: null,
    editorCollection: null,
    editorId: null,
    editorValues: null,
  };
}
```

- [ ] **Step 4: Add Back button and return-to-list behavior in editor form**

```javascript
<div class="form-actions">
  <button class="button" type="submit">Save</button>
  <button class="secondary-button" type="button" id="back-button">Back</button>
  ${state.draftMode === 'edit' ? `<button class="secondary-button danger-button" type="button" id="delete-button">Delete</button>` : ''}
</div>
```

Bind `#back-button` to `Object.assign(state, finishEditorFlow()); render();`

- [ ] **Step 5: Return to listing after successful save and delete**

After successful `saveResource()` and `deleteResource()`, call `Object.assign(state, finishEditorFlow());` before the final `render()`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/aisix-server/ui/app.mjs crates/aisix-server/ui/app.test.mjs
git commit -m "feat: return admin ui editor actions to list view"
```

### Task 8: Run focused and full verification

**Files:**
- Modify: any files changed by `cargo fmt` or final cleanup

- [ ] **Step 1: Run UI-targeted verification first**

Run: `node --test "crates/aisix-server/ui/app.test.mjs"`
Expected: PASS

- [ ] **Step 2: Run server-side regression coverage for embedded UI routes**

Run: `cargo test -p aisix-server admin_router_serves_ui_and_admin_api -- --exact && cargo test -p aisix-server data_plane_router_does_not_serve_ui_entrypoint -- --exact`
Expected: PASS

- [ ] **Step 3: Run full project verification**

Run: `cargo fmt --all -- --check && cargo test && cargo clippy -- -D warnings`
Expected: PASS

- [ ] **Step 4: Inspect final worktree**

Run: `git status --short`
Expected: Only intended UI-related files are modified.

- [ ] **Step 5: Commit any final formatting-only changes if needed**

```bash
git add -A
git commit -m "style: normalize admin ui list flow formatting"
```

Only do this step if verification changed tracked files.

- [ ] **Step 6: Push updated branch**

```bash
git push
```
