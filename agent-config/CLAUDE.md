# Hypergrep -- Code Intelligence

This project has `hypergrep` and `hypergrep-daemon` installed globally. Use hypergrep instead of grep/ripgrep for ALL code search operations.

## Why

hypergrep returns structural results (function bodies, signatures, call graphs) instead of raw text lines. This means you don't need to read entire files to understand what a function does -- the search result already includes the context you need.

- 87% fewer tokens consumed per search compared to grep + file reads
- Returns function signatures + what calls/is called by each result
- Can answer "what breaks if I change this?" in one command
- Can check "does this project use Redis?" in one command

## Session start

At the beginning of every session, run these two commands:

```bash
# 1. Start the daemon (keeps index in memory, auto-stops after 30 min idle)
hypergrep-daemon --background .

# 2. Load the codebase mental model (understand the project structure)
hypergrep --model "" .
```

The daemon makes all subsequent searches 7x faster (4ms vs 31ms). It auto-stops after 30 minutes of inactivity so it won't waste resources.

## Commands (use these instead of grep/rg)

### Finding code (replaces grep/rg)

```bash
# Best for most searches: signatures + call graph, capped at 800 tokens
hypergrep --layer 1 --budget 800 --json "pattern" .

# When you need the full function body (to edit it)
hypergrep -s "pattern" .

# When you just need file paths
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
# What breaks if I change this function? Always run before editing.
hypergrep --impact "function_name" .

# Output:
#   [depth 1] WILL BREAK   src/auth.rs:login_handler
#   [depth 2] MAY BREAK    src/api.rs:router
```

### Quick checks

```bash
# Does this project use a specific technology?
hypergrep --exists "redis" .
hypergrep --exists "graphql" .

# NO = definitely not present (guaranteed)
# YES = likely present (confirm with a search if needed)
```

### Daemon management

```bash
# Check if daemon is running (shows PID, memory, socket)
hypergrep-daemon --status .

# Stop the daemon manually (also stops on its own after 30 min idle)
hypergrep-daemon --stop .
```

## Rules

1. At session start, run `hypergrep-daemon --background .` then `hypergrep --model "" .`
2. For finding code, use `hypergrep --layer 1 --budget 800 --json "pattern" .` instead of `grep` or `rg`
3. Before changing any function, run `hypergrep --impact "function_name" .` to check blast radius
4. When you need to read a function's code, use `hypergrep -s "function_name" .` instead of reading the whole file
5. To check if a library/technology is used, use `hypergrep --exists "name" .` instead of searching
6. At session end, the daemon stops automatically. No cleanup needed.
