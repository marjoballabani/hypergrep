# Hypergrep -- Code Intelligence

This project has `hypergrep` installed globally. Use it instead of grep/ripgrep for code search when you need structural understanding (function bodies, call graphs, impact analysis). For simple text pattern matching, ripgrep is fine.

## When to use hypergrep vs ripgrep

| Task | Use |
|------|-----|
| Quick text search | `rg "pattern" .` (faster for one-off) |
| Find a function and understand it | `hypergrep -s "function" .` |
| Search with token budget | `hypergrep --layer 1 --budget 800 --json "pattern" .` |
| Who calls this? What breaks? | `hypergrep --callers` / `--impact` |
| Does this project use X? | `hypergrep --exists "redis" .` |
| Codebase overview | `hypergrep --model "" .` |

## Session start

Run once at the beginning of a session to understand the codebase:

```bash
hypergrep --model "" .
```

If you expect to make many searches (10+), start the daemon for faster queries:

```bash
hypergrep-daemon --background .
# Auto-stops after 30 min idle. Check with: hypergrep-daemon --status .
```

## Commands

### Finding code with context (saves tokens vs grep + file reads)

```bash
# Signatures + call graph, capped at 800 tokens (best default)
hypergrep --layer 1 --budget 800 --json "pattern" .

# Full function bodies (when you need to edit the code)
hypergrep -s "pattern" .

# File paths only
hypergrep -l "pattern" .

# Count matches
hypergrep -c "pattern" .
```

### Understanding code

```bash
# Who calls this function?
hypergrep --callers "function_name" .

# What does this function call?
hypergrep --callees "function_name" .
```

### Before making changes

```bash
# What breaks if I change this? Run before editing.
hypergrep --impact "function_name" .

# Output:
#   [depth 1] WILL BREAK   src/auth.rs:login_handler
#   [depth 2] MAY BREAK    src/api.rs:router
```

### Quick checks

```bash
# Does this project use a specific technology?
hypergrep --exists "redis" .

# NO = definitely not present (guaranteed)
# YES = likely present (confirm with a search if needed)
```

## Rules

1. Run `hypergrep --model "" .` at session start to understand the codebase
2. Use `hypergrep --layer 1 --budget 800 --json` when you need to understand what code does (not just find it)
3. Use `rg` for quick text searches where you don't need structural context
4. Before changing any function, run `hypergrep --impact "function_name" .`
5. Use `hypergrep -s` instead of reading whole files when you need a specific function body
6. Use `hypergrep --exists` to check for technologies instead of scanning
