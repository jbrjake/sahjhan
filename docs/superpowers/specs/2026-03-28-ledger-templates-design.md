# Ledger Templates — Design Spec

Addresses: GitHub issue #10 (three interrelated bugs in multi-ledger management)

## Problem

Three bugs combine to make multi-ledger workflows non-functional:

1. `ledger create --path` joins the user-supplied path with `data_dir`, creating files in the wrong location.
2. `renders.toml` `ledger = "run"` does a literal registry lookup. No registry entry is named `"run"` — the actual entries are `"run-24"`, `"run-25"`. The render silently falls back to the default ledger.
3. Status always shows "Run 0" because it counts `protocol_init` events, which are never created.

The root cause is that sahjhan has no concept of ledger templates. Protocols can't declare "I have a category of ledger called `run` that gets instantiated per-use with an identifier." Everything is either a hardcoded registry name or a raw file path.

## Design

### Grammar Expansion: `[ledgers]` in protocol.toml

Protocol authors declare ledger templates in protocol.toml:

```toml
[ledgers.run]
description = "Per-run audit ledger"
path_template = "docs/holtz/runs/{template.instance_id}/ledger.jsonl"

[ledgers.project]
description = "Cross-run project ledger"
path = "docs/holtz/project.jsonl"
```

Two forms:

- **Template** (`path_template`): A pattern with `{template.instance_id}` that gets resolved when a ledger is instantiated. The variable `{template.name}` (the template's own name, e.g., `"run"`) is also available.
- **Fixed** (`path`): A single, known path. No instantiation needed.

These are mutually exclusive per entry. Validation rejects entries with both or neither.

### Config Structs

```rust
// src/config/protocol.rs

/// A ledger declaration in protocol.toml.
#[derive(Debug, Deserialize, Clone)]
pub struct LedgerTemplateConfig {
    pub description: String,
    /// Fixed path (for singleton ledgers).
    pub path: Option<String>,
    /// Path template with {template.instance_id} and {template.name} variables.
    pub path_template: Option<String>,
}
```

`ProtocolFile` gains:

```rust
#[serde(default)]
pub ledgers: HashMap<String, LedgerTemplateConfig>,
```

`ProtocolConfig` gains:

```rust
pub ledgers: HashMap<String, LedgerTemplateConfig>,
```

### Registry Expansion

Registry entries gain optional `template` and `instance_id` fields:

```rust
// src/ledger/registry.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRegistryEntry {
    pub name: String,
    pub path: String,
    pub mode: LedgerMode,
    pub created: String,
    /// Which protocol template this ledger was created from (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Instance identifier within the template (e.g., "25" for run-25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}
```

On-disk format (ledgers.toml):

```toml
[[ledgers]]
name = "run-25"
path = "docs/holtz/runs/25/ledger.jsonl"
mode = "stateful"
template = "run"
instance_id = "25"
created = "2026-03-28T15:28:19Z"
```

The `LedgerRegistry::create` method gains `template` and `instance_id` parameters (both `Option<&str>`). Existing callers pass `None` for both — no breaking change.

A new method `LedgerRegistry::resolve_by_template` finds ledger entries by template name:

```rust
/// Find all registry entries created from a given template.
pub fn resolve_by_template(&self, template: &str) -> Vec<&LedgerRegistryEntry> {
    self.entries
        .iter()
        .filter(|e| e.template.as_deref() == Some(template))
        .collect()
}
```

### CLI: `ledger create --from`

New syntax for template-based creation:

```
sahjhan ledger create --from run 25
```

Clap definition:

```rust
LedgerAction::Create {
    /// Ledger name (for direct creation without template)
    #[arg(long, required_unless_present = "from")]
    name: Option<String>,

    /// File path (for direct creation without template)
    #[arg(long, required_unless_present = "from")]
    path: Option<String>,

    /// Create from a protocol-declared ledger template
    #[arg(long)]
    from: Option<String>,

    /// Instance identifier for the template (e.g., "25" for run-25)
    #[arg(requires = "from")]
    instance_id: Option<String>,

    /// Ledger mode: stateful or event-only
    #[arg(long, default_value = "stateful")]
    mode: String,
}
```

Two modes:

1. **Direct** (existing, fixed): `ledger create --name foo --path bar` — creates at the specified path, registers with no template association. The `--path` bug is fixed: if the path is absolute, use as-is; if relative, resolve relative to **cwd** (not `data_dir`). The registry stores the path relative to `data_dir` if it falls under `data_dir`, otherwise absolute.

2. **From template**: `ledger create --from run 25` — looks up `[ledgers.run]` in protocol.toml, resolves `path_template` by replacing `{template.instance_id}` with `"25"` and `{template.name}` with `"run"`, derives the ledger name as `run-25`, registers with `template = "run"` and `instance_id = "25"`.

Template-based creation errors if:
- The template name doesn't exist in protocol.toml
- The template has `path` instead of `path_template` (fixed ledgers aren't instantiable)
- `instance_id` is missing when `path_template` contains `{template.instance_id}`
- A registry entry with the derived name already exists

### Bug 1 Fix: Path Handling in Direct Mode

Current code at `src/cli/ledger.rs:51-57` joins `--path` with `data_dir`. Fix:

```rust
// Resolve output path:
// - Absolute paths: use as-is
// - Relative paths: resolve relative to cwd (not data_dir)
let ledger_file = if PathBuf::from(path).is_absolute() {
    PathBuf::from(path)
} else {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(path)
};

// For registry storage, compute path relative to data_dir if under it,
// otherwise store the absolute path.
let registry_stored_path = compute_registry_path(&ledger_file, &data_dir);
```

Where `compute_registry_path` strips the `data_dir` prefix if the file is under it (so `resolve_registry_path` round-trips correctly), otherwise stores the absolute path.

The same fix applies to `cmd_ledger_import` which has the identical pattern.

### Bug 2 Fix: Render Ledger Resolution via `ledger_template`

`RenderConfig` gains a new field:

```rust
// src/config/renders.rs

#[derive(Debug, Deserialize, Clone)]
pub struct RenderConfig {
    pub target: String,
    pub template: String,
    pub trigger: String,
    pub event_types: Option<Vec<String>>,
    /// Direct ledger name from registry.
    pub ledger: Option<String>,
    /// Ledger template name from protocol.toml — resolves to the active
    /// (targeted) ledger if it was created from this template.
    pub ledger_template: Option<String>,
}
```

Validation rejects configs with both `ledger` and `ledger_template` set.

The downstream protocol uses:

```toml
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"
ledger_template = "run"
```

Resolution logic in `resolve_render_ledger`:

1. If `ledger` is set: literal registry lookup (existing behavior, unchanged).
2. If `ledger_template` is set: check if the **active ledger** (the one targeted by `--ledger` or the default) was created from this template. If yes, use it. If no, find the most recently created registry entry with `template == ledger_template` and use that.
3. If neither: use default ledger (existing behavior).

This requires the render engine to know the active ledger's registry metadata. The `render_triggered` and `render_all` callers already pass the active ledger — they also need to pass the active ledger's registry entry (or at minimum its `template` field). This is done via a new `with_active_ledger_name` builder method on `RenderEngine`:

```rust
impl RenderEngine {
    pub fn with_active_ledger_name(mut self, name: String) -> Self {
        self.active_ledger_name = Some(name);
        self
    }
}
```

The engine uses this to look up the registry entry and check its `template` field.

### Bug 3 Fix: Status Display from Registry Metadata

Replace the `protocol_init` event counting at `src/cli/status.rs:118-119` with registry metadata lookup.

The `cmd_status` function already has access to `targeting` (the `--ledger` flag). When a named ledger is targeted:

1. Look up the registry entry.
2. If it has `template` and `instance_id`, display: `{template} {instance_id}` (e.g., `run 25`).
3. If no template metadata, display the ledger name.
4. If no named ledger (default), omit the run indicator.

```rust
// Build instance label from registry metadata
let instance_label = if let Some(ref name) = targeting.ledger_name {
    let reg_path = registry_path_from_config(&config);
    if let Ok(registry) = LedgerRegistry::new(&reg_path) {
        if let Ok(entry) = registry.resolve(Some(name)) {
            match (&entry.template, &entry.instance_id) {
                (Some(tmpl), Some(id)) => format!(" · {} {}", tmpl, id),
                _ => format!(" · {}", name),
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    }
} else {
    String::new()
};

println!(
    "  sahjhan · {} v{}{}",
    config.protocol.name, config.protocol.version, instance_label
);
```

Output: `sahjhan · holtz v1.0.0 · run 25`

### Template Variables in Render Context

When a render uses `ledger_template`, the template's `instance_id` is injected into the Tera context as `template_instance_id`. This lets templates reference the instance ID (e.g., `{{ template_instance_id }}` in a Tera template that renders "Run {{ template_instance_id }}").

### Validation

`validate_deep` gains checks for the `[ledgers]` section:

- Each entry must have exactly one of `path` or `path_template`.
- `path_template` values must contain `{template.instance_id}`.
- `path` values (fixed ledgers) must be valid relative or absolute paths.
- `ledger_template` values in renders.toml must reference a template name that exists in `[ledgers]`.
- Renders must not set both `ledger` and `ledger_template`.

### Files Changed

| File | Change |
|------|--------|
| `src/config/protocol.rs` | Add `LedgerTemplateConfig`, add `ledgers` to `ProtocolFile` |
| `src/config/mod.rs` | Propagate `ledgers` to `ProtocolConfig`, add validation |
| `src/config/renders.rs` | Add `ledger_template` field to `RenderConfig` |
| `src/ledger/registry.rs` | Add `template`, `instance_id` to `LedgerRegistryEntry`; add `resolve_by_template`; extend `create` signature |
| `src/cli/ledger.rs` | Fix `--path` resolution; add `--from` template-based creation |
| `src/main.rs` | Update `LedgerAction::Create` clap definition |
| `src/render/engine.rs` | Update `resolve_render_ledger` for `ledger_template`; add `with_active_ledger_name` |
| `src/cli/transition.rs` | Pass active ledger name to render engine |
| `src/cli/status.rs` | Replace `protocol_init` counting with registry metadata |
| `src/cli/commands.rs` | Add `compute_registry_path` helper |
| `tests/` | New tests for template creation, render resolution, status display |
| `CLAUDE.md` | Update lookup tables |

### What This Does NOT Do

- No auto-creation of ledgers. `ledger create --from run 25` is explicit.
- No auto-incrementing instance IDs. The caller chooses the ID.
- No lifecycle management (archiving, rotation). That's a separate concern.
- No changes to gate evaluation, template variable resolution, or the state machine.
- No holtz-specific logic. `{template.instance_id}` and `{template.name}` are generic grammar.
