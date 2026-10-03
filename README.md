# ColorBalance

ColorBalance builds a reusable color transform from one X-Rite or Calibrite ColorChecker Classic RAW reference photo and applies it to every image in a batch captured with the same camera, lighting, exposure, and RAW settings. It measures what it does, refuses references it cannot trust, and never modifies input files.

Status: implementation started. Milestone 1 is in progress; the Rust workspace, CLI, and CI are in place and the calibration core is being built.

## Documentation

Read in this order:

1. [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) - product scope, color pipeline, quality gates, milestones, and acceptance criteria
2. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - platform and performance decisions, deployment modes, decision log
3. [AGENTS.md](AGENTS.md) - working rules for AI agents and contributors

## Roadmap

Work is tracked as GitHub issues grouped by milestone:

- [Milestone 1: Measured calibration core](https://github.com/benkl/colorbalance/milestone/1)
- [Milestone 2: Safe batch workflow](https://github.com/benkl/colorbalance/milestone/2)
- [Milestone 3: Interchange and independent validation](https://github.com/benkl/colorbalance/milestone/3)
- [Milestone 4: Desktop release](https://github.com/benkl/colorbalance/milestone/4)
- [Milestone 5: Web-capable platform](https://github.com/benkl/colorbalance/milestone/5)

Each issue lists scope, dependencies, and acceptance criteria. An issue is done when its acceptance criteria are observable, not when code compiles.

## Stack

Rust engine with LibRaw, a Tauri 2 desktop application with a React UI, and an optional WebAssembly browser path. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full decision record.

## Repository layout

```text
docs/       implementation plan, architecture, decision log, development guide
crates/     Rust workspace: colorbalance-core, colorbalance-raw, colorbalance-cli
.github/    CI workflow for Windows, macOS, Linux, and wasm32
apps/       Tauri desktop and browser applications (later milestones)
research/   Python verification notebooks (non-runtime)
```

## License

To be decided before the first public release.
