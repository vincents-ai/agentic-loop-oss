# agentic-loop-oss

Open-source AI agent loop with trait-first architecture and engram integration.

Part of the [ADP (Agentic Development Platform)](https://github.com/vincents-ai) ecosystem.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                 agentic-loop-bin                     │
│  CLI: run, list, show, models, providers            │
└───────────┬─────────────────────────────────────────┘
            │
    ┌───────┴───────┐
    │  trait crates  │          implementation crates
    ├───────────────┤          ├──────────────────────┤
    │ types         │          │ storage-gitrefs       │
    │ storage       │          │ tools-core            │
    │ tools         │          │ wasm (plugin runtime) │
    │ workflow      │          └──────────────────────┤
    │ eval          │          │ llm                   │
    │ llm           │          │  (Arc<dyn LLMProvider>)│
    └───────────────┘          └──────────────────────┘
```

### Trait crates (interfaces)
- **agentic-loop-types** — shared types, workflow models, tool definitions
- **agentic-loop-storage** — `EntityStore` trait for git-refs storage
- **agentic-loop-tools** — `ToolProvider` trait for tool dispatch
- **agentic-loop-workflow** — Gherkin workflow parsing and execution engine
- **agentic-loop-eval** — `HeuristicEvaluator` trait for step evaluation
- **agentic-loop-llm** — `LlmExecutorTrait` for LLM execution, `ModelPool` for rotation

### Implementation crates
- **agentic-loop-storage-gitrefs** — git-refs backed `EntityStore`
- **agentic-loop-tools-core** — 12 built-in tools (bash, file read/write, grep, etc.)
- **agentic-loop-wasm** — WASM plugin runtime (wasmtime 44)
- **agentic-loop-llm** — LLM executor using `vincents-llm` directly (BYOK)
- **agentic-loop-bin** — CLI binary wiring everything together

## Design Principles

1. **Trait-first** — every component depends on traits, not concrete types
2. **`Arc<dyn LLMProvider>`** — the LLM crate depends only on the vincents-llm trait.
   The factory creates providers, the executor uses them. No wrapper types.
3. **No vendor lock-in** — bring your own key (BYOK), any provider
4. **Git-refs storage** — all state stored as git refs, no database required

## Building

```bash
nix develop --command bash -c "cargo build"
```

## Usage

```bash
# List workflows
agentic-loop list

# Show workflow details
agentic-loop show development

# Run a workflow with a specific provider
agentic-loop run quick "Fix the bug in auth.rs" -p anthropic -m claude-sonnet-4-20250514

# Discover free models from OpenRouter
agentic-loop models

# List available LLM providers
agentic-loop providers
```

## License

AGPL-3.0-or-later OR LicenseRef-Commercial
