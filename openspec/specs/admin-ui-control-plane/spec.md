## ADDED Requirements

### Requirement: Admin API and Admin UI share the dedicated admin listener
The gateway SHALL serve the Admin API and the embedded Admin UI from the same dedicated admin listener configured by `server.admin_listen`.

#### Scenario: Operator opens the browser control plane
- **WHEN** the gateway starts with admin enabled
- **THEN** the Admin UI is reachable at `/ui` on `server.admin_listen`
- **AND** the Admin API remains reachable at `/admin/...` on that same listener

### Requirement: Admin listener stays separate from the data plane listener
The gateway SHALL keep the admin listener separate from the data plane listener. `server.admin_listen` MUST NOT reuse the data plane port exposed by `server.listen`.

#### Scenario: Startup detects overlapping listener ports
- **WHEN** admin is enabled and `server.admin_listen` reuses the same port as `server.listen`
- **THEN** startup fails before serving requests

#### Scenario: Data plane listener excludes control-plane routes
- **WHEN** a client sends `/admin/...` or `/ui` traffic to the data plane listener
- **THEN** the data plane listener does not serve those routes

### Requirement: Browser admin key storage stays session-scoped
The embedded Admin UI SHALL require the operator to enter the admin key manually and SHALL keep that key only in browser session-scoped storage.

#### Scenario: Browser session ends
- **WHEN** the operator closes the browser session
- **THEN** the previously entered admin key is no longer retained by the UI
