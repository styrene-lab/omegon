+++
id = "fd298cde-3ba3-4abb-a46b-15922b0dd4e4"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# web-ui — Delta Spec

## ADDED Requirements

### Requirement: Start a localhost-only web UI server

pi-kit SHALL provide a first-party web UI extension that can start a browser-facing HTTP server bound to localhost for the current session.

#### Scenario: Start server on localhost

Given pi-kit is running in a repository session
When the operator starts the web UI server
Then pi-kit starts an HTTP server bound to `127.0.0.1`
And pi-kit reports the local URL including the chosen port
And the server does not bind to `0.0.0.0` by default

#### Scenario: Stop server cleanly

Given the web UI server is running
When the operator stops the web UI server
Then pi-kit terminates the server it started for that session
And subsequent HTTP requests to the previous port fail

### Requirement: Serve a read-only dashboard shell

The root web UI route SHALL return a browser-renderable dashboard shell for localhost operators.

#### Scenario: Load dashboard shell

Given the web UI server is running
When a browser requests `GET /`
Then the response status is `200`
And the response content type is HTML
And the page includes client code that can fetch normalized state from the API

### Requirement: Expose a versioned ControlPlaneState snapshot

The web UI SHALL expose one normalized read-only snapshot contract for browser rendering.

#### Scenario: Fetch full state snapshot

Given the web UI server is running
When a browser requests `GET /api/state`
Then the response status is `200`
And the response content type is JSON
And the body contains `schemaVersion`
And the body contains top-level sections `session`, `dashboard`, `designTree`, `openspec`, `cleave`, `models`, `memory`, and `health`

#### Scenario: Snapshot is derived from live process state

Given the web UI server is running
And pi-kit subsystem state changes during the session
When a browser requests `GET /api/state` after the change
Then the returned snapshot reflects the latest live shared state and on-demand scanned lifecycle data
And pi-kit does not require a separate browser history database to serve the update

### Requirement: Expose read-only slice routes for debugging and composition

The web UI SHALL expose stable read-only JSON routes for key state slices.

#### Scenario: Fetch read-only slices

Given the web UI server is running
When a browser requests `GET /api/design-tree`, `GET /api/openspec`, `GET /api/cleave`, `GET /api/models`, `GET /api/memory`, and `GET /api/health`
Then each route returns status `200`
And each route returns JSON only for its documented slice
And none of the routes mutate pi-kit state

### Requirement: Reject unsupported mutation routes in MVP

The first web UI release SHALL not expose browser-driven mutation or command execution endpoints.

#### Scenario: Unsupported write method is refused

Given the web UI server is running
When a client sends `POST` to `/api/state`
Then the server responds with a non-success status
And the request does not trigger a pi-kit command or state mutation

#### Scenario: Unknown mutation route is absent

Given the web UI server is running
When a client requests a non-existent mutation endpoint
Then the server responds with `404` or `405`
And no operator action is performed

### Requirement: Use polling-first browser updates

The browser experience SHALL work using periodic snapshot fetches without requiring SSE or WebSockets in the MVP.

#### Scenario: Browser refreshes by polling

Given the dashboard shell is loaded in a browser
When the page periodically fetches `/api/state`
Then the dashboard can update visible status from successive snapshots
And the MVP does not require a websocket or server-sent events endpoint to function

## MODIFIED Requirements

### Requirement: Command surface for web UI lifecycle

The web UI extension SHALL provide an operator command surface for starting, inspecting, opening, and stopping the localhost server.

#### Scenario: Inspect server status before start

Given the web UI server is not running
When the operator requests web UI status
Then pi-kit reports that the server is stopped
And no browser is opened implicitly

#### Scenario: Open server in browser

Given the web UI server is running
When the operator requests the web UI to open in a browser
Then pi-kit opens the localhost URL using the platform browser launcher
And the URL points to the running local server

### Requirement: HTML shell delivery strategy

The MVP HTML experience SHALL be delivered as a lightweight built-in shell that fetches state from JSON endpoints.

#### Scenario: HTML shell is transport-light

Given the web UI server is running
When a browser requests `GET /`
Then the server can return a lightweight built-in HTML shell
And the shell obtains current dashboard data from `GET /api/state`
And the server does not need to server-render the entire control-plane snapshot into the HTML response for the MVP
