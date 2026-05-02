# AGENTS.md

## Project
`greedcode` is a Rust 2024 CLI that fetches the current top free ShirMan model, sends prompts through OpenRouter, streams SSE responses, and renders output to stdout.

Read `IMPLEMENTATION_PLAN.md` for architecture and product context before changing API flow or CLI behavior.

## Commands
- Check: `cargo check`
- Test: `cargo test`
- Format: `cargo fmt`
- Lint: `cargo clippy --all-targets --all-features`
- Run locally: `cargo run -- "<prompt>"`

## Working Rules
- Keep changes small and idiomatic Rust.
- Preserve stdout for assistant response text only; write logs and errors to stderr.
- Preserve streaming behavior and flush output incrementally.
- Keep API clients in `src/api/`, shared response types in `src/models/`, and terminal rendering in `src/output.rs`.
- Add or update unit tests for parser, rendering, and boundary handling changes.
- Do not expose, print, commit, or modify secrets in `.env`; `OPENROUTER_API_KEY` is required at runtime.

## Before Finishing
- Run `cargo fmt`.
- Run `cargo test`.
- Run `cargo clippy --all-targets --all-features` when code changed.
