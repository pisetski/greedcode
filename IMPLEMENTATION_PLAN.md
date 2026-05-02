# greedcode CLI Implementation Plan

## 1. Project Structure

```
greedcode/
├── Cargo.toml
└── src/
    ├── main.rs           # Entry point, clap CLI setup
    ├── api/
    │   ├── mod.rs
    │   ├── shirman.rs    # shir-man.com API client
    │   └── openrouter.rs # OpenRouter chat completion client
    ├── models/
    │   ├── mod.rs
    │   └── types.rs      # Serde structs
    └── output.rs         # SSE streaming and stdout flushing
```

## 2. Dependencies (Cargo.toml)

```toml
[dependencies]
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
clap = { version = "4.5", features = ["derive"] }
anyhow = "1.0"
```

## 3. CLI Interface

```bash
# Single prompt mode
greedcode "who is the strongest man?"
```

MVP behavior:
- A prompt is required.
- Multiple prompt arguments are joined with spaces.
- If no prompt is provided, print usage and exit 2.
- Piped stdin, REPL mode, and explicit model selection are future work.

## 4. API Flow

1. **Validate Environment** → Require `OPENROUTER_API_KEY` before making requests.
2. **Fetch Models** → GET `https://shir-man.com/api/free-llm/top-models`.
3. **Parse Response** → Extract and validate the first model's `id` and optional `name`.
4. **Log Model** → Write `Using model: {name} ({id})` to stderr. If `name` is missing, use the `id` for both fields.
5. **Create Chat Completion** → POST `https://openrouter.ai/api/v1/chat/completions`.
6. **Handle HTTP Status** → If response is not 2xx, print status code and response body to stderr, then exit 1.
7. **Parse SSE Stream** → Read `data:` frames, ignore blank/comment lines, stop on `[DONE]`, deserialize each JSON payload, and extract `choices[].delta.content`.
8. **Write Output** → Write only assistant text to stdout and flush after each content delta.
9. **Exit** → Return 0 after the stream completes successfully.

### OpenRouter Request

```http
POST https://openrouter.ai/api/v1/chat/completions
Authorization: Bearer ${OPENROUTER_API_KEY}
Content-Type: application/json
```

```json
{
  "model": "<model_id_from_shir_man>",
  "messages": [
    { "role": "user", "content": "<prompt>" }
  ],
  "stream": true
}
```

Optional OpenRouter attribution headers can be added later; they are not required for MVP.

## 5. Response Parsing

OpenRouter streaming responses are server-sent events, not raw model text. The MVP must parse the stream before writing to stdout.

Expected stream shape:

```text
data: {"choices":[{"delta":{"content":"Hello"}}]}
data: {"choices":[{"delta":{"content":" world"}}]}
data: [DONE]
```

Rules:
- Buffer bytes until complete lines are available.
- Process only lines beginning with `data:`.
- Trim the `data:` prefix and surrounding whitespace.
- Stop when the payload is `[DONE]`.
- Deserialize JSON payloads and print only `choices[].delta.content` values.
- Ignore deltas without `content`.
- Treat malformed SSE or malformed JSON as an error and exit 1.

## 6. Output Handling

| Output Type          | Destination                      |
|----------------------|----------------------------------|
| Model selection log  | stderr                           |
| Error messages       | stderr                           |
| **Model response**   | **stdout** (pipable)             |

Avoid progress logs beyond model selection in the MVP. Stdout must contain only model response text.

## 7. Error Handling

- Missing `OPENROUTER_API_KEY` → `"OPENROUTER_API_KEY is required"` → exit 1
- Fetch models failed → `"Error fetching models: {detail}"` → exit 1
- Fetch models returned non-2xx → `"Error fetching models: HTTP {status}: {body}"` → exit 1
- No usable model returned → `"No free models available"` → exit 1
- OpenRouter request failed → `"OpenRouter request failed: {detail}"` → exit 1
- OpenRouter returned non-2xx → `"OpenRouter API error: HTTP {status}: {body}"` → exit 1
- Stream parse failed → `"Error parsing stream: {detail}"` → exit 1

## 8. Environment Variables

| Variable               | Required  | Description                              |
|------------------------|-----------|------------------------------------------|
| `OPENROUTER_API_KEY`   | **Yes**   | OpenRouter API key for authentication    |

## 9. Implementation Phases

### Phase 1: Core (MVP)
- Parse prompt from CLI args
- Validate `OPENROUTER_API_KEY`
- Fetch the top free model from shir-man.com
- Send a streaming OpenRouter chat completion request
- Parse SSE frames and print only assistant text to stdout
- Report clear HTTP and stream parsing errors to stderr

### Phase 2: Interactive Mode
- REPL implementation
- Conversation history
- Basic commands (`exit`, `quit`, `clear`, `help`)
- Graceful Ctrl+C behavior

### Phase 3: Polish (Future)
- Configuration file support
- Model selection options
- Piped stdin support
- Quiet/verbose logging controls

## 10. Key Implementation Notes

- Use `reqwest` streaming to get incremental byte chunks
- Parse OpenRouter SSE frames before writing output
- Flush stdout after each chunk for real-time display
- Validate `OPENROUTER_API_KEY` at startup
- Keep stdout reserved for assistant text so output remains pipable
