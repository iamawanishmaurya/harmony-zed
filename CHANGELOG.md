# Changelog

All notable changes to this project will be documented in this file.

## 0.2.0 - 2026-04-08

- Added automatic host/client project file sync for created, updated, and deleted files and folders.
- Added persistent file sync event storage and new host APIs for file timeline replication.
- Added a dashboard `Files` section with live file activity cards, summary counters, machine/actor metadata, and project-impact summaries.
- Added new network sync config defaults: `auto_sync`, `sync_interval_seconds`, and `max_sync_file_bytes`.
- Updated the docs to cover cross-laptop file replication, dashboard verification, and the new project-local `.harmony/network-sync-state.json` state file.
