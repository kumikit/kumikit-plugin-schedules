# Scheduled Prompts (`kumikit.schedules`)

Create recurring prompts on a cron schedule in any open workspace.

## Install

```sh
kumikit plugin add kumikit.schedules
```

## Usage

- Run **New scheduled prompt** from the command palette to create a recurring prompt (either
  a fresh thread on every run or reusing one thread).
- Supports standard 5-field cron expressions (e.g. `0 9 * * 1-5`) and shorthands
  (`@hourly`, `@daily`, `@weekly`, `@monthly`), with optional IANA time zones.
- Manage, pause, run now, or delete schedules from **Settings → Schedules** in Kumikit
  (or via `schedule.*` protocol methods and `kumikit.schedules/*`).
