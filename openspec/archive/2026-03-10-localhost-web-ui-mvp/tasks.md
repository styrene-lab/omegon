+++
id = "b6f7a34c-1ddb-4db7-8cc3-2b28fde9bed7"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# localhost-web-ui-mvp — Tasks

## 1. Start a localhost-only web UI server

- [x] 1.1 Start server on localhost
- [x] 1.2 Stop server cleanly
- [x] 1.3 Write tests for Start a localhost-only web UI server

## 2. Serve a read-only dashboard shell

- [x] 2.1 Load dashboard shell
- [x] 2.2 Write tests for Serve a read-only dashboard shell

## 3. Expose a versioned ControlPlaneState snapshot

- [x] 3.1 Fetch full state snapshot
- [x] 3.2 Snapshot is derived from live process state
- [x] 3.3 Write tests for Expose a versioned ControlPlaneState snapshot

## 4. Expose read-only slice routes for debugging and composition

- [x] 4.1 Fetch read-only slices
- [x] 4.2 Write tests for Expose read-only slice routes for debugging and composition

## 5. Reject unsupported mutation routes in MVP

- [x] 5.1 Unsupported write method is refused
- [x] 5.2 Unknown mutation route is absent
- [x] 5.3 Write tests for Reject unsupported mutation routes in MVP

## 6. Use polling-first browser updates

- [x] 6.1 Browser refreshes by polling
- [x] 6.2 Write tests for Use polling-first browser updates

## 7. Command surface for web UI lifecycle

- [x] 7.1 Inspect server status before start
- [x] 7.2 Open server in browser
- [x] 7.3 Write tests for Command surface for web UI lifecycle

## 8. HTML shell delivery strategy

- [x] 8.1 HTML shell is transport-light
- [x] 8.2 Write tests for HTML shell delivery strategy
