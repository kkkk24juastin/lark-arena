# Repository Guidelines

## Project Structure & Module Organization

This is a single Rust 2024 binary crate. `src/main.rs` wires configuration, storage, the Feishu client, and the Axum server together. Shared orchestration lives in `src/bot.rs`, HTTP setup in `src/server.rs`, environment loading in `src/config.rs`, and redb persistence in `src/storage.rs`. Werewolf game logic lives in `src/werewolf/`; AI personality profiles live in `src/persona.rs`. Feishu events, API calls, and card builders belong in `src/feishu/`. Keep protocol/UI code out of game state machines.

Tests currently live beside their implementation in `#[cfg(test)] mod tests` blocks. CI configuration is in `.github/workflows/ci.yml`; container packaging is defined by `Dockerfile`.

## Build, Test, and Development Commands

- `cargo check --locked`: type-check the pinned dependencies.
- `cargo test --locked`: run all unit and persistence tests.
- `cargo test --locked werewolf`: run tests whose names contain `werewolf`.
- `cargo run --release`: start the bot using values from `.env`.
- `cargo build --release --locked`: produce `target/release/lark-arena`.
- `docker build -t lark-arena .`: reproduce CI's test-gated container build.

Local startup requires `FEISHU_APP_ID` and `FEISHU_APP_SECRET`. Copy `.env.example` to `.env` and fill in local values; never commit secrets.

## Coding Style & Naming Conventions

Use standard `rustfmt` formatting with four-space indentation; run `cargo fmt -- --check` when rustfmt is installed. Use `snake_case` for modules, functions, variables, and tests; `PascalCase` for types and enums; and `SCREAMING_SNAKE_CASE` for constants. Prefer domain-focused functions, exhaustive enum matching, and `anyhow::Context` at I/O or API boundaries. Preserve existing bilingual user-facing copy.

## Testing Guidelines

Add focused unit tests next to changed logic. Name tests after observable behavior, such as `start_game_rejects_too_few_players`. Cover game transitions, role edge cases, serialization compatibility, and failure paths. There is no stated coverage threshold, but every bug fix should include a regression test. Run `cargo test --locked` before opening a PR.

## Commit & Pull Request Guidelines

Recent commits use concise, imperative summaries, often `area: change`, for example `storage: add schema version envelope`. Keep each commit scoped to one behavior. PRs should explain the user-visible effect, note configuration or storage-schema changes, link relevant issues, and list verification commands. Include screenshots or representative card JSON when changing Feishu card layouts.

## Security & Configuration

Do not commit `.env`, API keys, app secrets, access tokens, generated `*.redb` databases, or logs. Treat webhook payloads as untrusted input and avoid logging credentials or private role information.
