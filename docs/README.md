# Roobu Documentation

This directory contains implementation-focused documentation for operating and extending Roobu.

## Documentation Map

- [Architecture](architecture.md)
  - System overview, ingest/search/cluster flow, storage model, and data lifecycle.
- [Commands](commands.md)
  - Detailed command behavior for ingest, search, and cluster.
- [Configuration](configuration.md)
  - Defaults, environment variables, models layout, and runtime tuning knobs.
- [Sites](sites.md)
  - Supported site matrix, auth requirements, and site-specific behavior.
- [Adding a New Site](adding-a-new-site.md)
  - Contributor guide for implementing and wiring a new site client.
- [Development Workflow](development.md)
  - Day-to-day developer commands and quality gates.
- [Troubleshooting](troubleshooting.md)
  - Common failure modes and practical fixes.

## Recommended Reading Order

1. Read [Architecture](architecture.md) to understand how data flows through the system.
2. Use [Commands](commands.md) and [Configuration](configuration.md) to run and tune Roobu.
3. Use [Sites](sites.md) for operational knowledge of each source.
4. Use [Adding a New Site](adding-a-new-site.md) when extending the codebase.

## Scope

The docs here intentionally mirror current code behavior in the src tree. If behavior changes, update these docs in the same pull request to keep operational guidance accurate.
