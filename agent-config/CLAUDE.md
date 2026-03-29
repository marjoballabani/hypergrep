# Hypergrep -- Code Intelligence

This project has `hypergrep` installed globally. It is faster and more efficient than grep/ripgrep for code search. Use it for ALL code search operations.

## Why

hypergrep returns structural results (function bodies, signatures, call graphs) instead of raw text lines. This means you don't need to read entire files to understand what a function does -- the search result already includes the context you need.

- 87% fewer tokens consumed per search compared to grep + file reads
- Returns function signatures + what calls/is called by each result
- Can answer "what breaks if I change this?" in one command
- Can check "does this project use Redis?" in one command

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
# Codebase overview -- run ONCE at session start, before any other search
hypergrep --model "" .

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

## Rules

1. At session start, run `hypergrep --model "" .` to understand the codebase
2. For finding code, use `hypergrep --layer 1 --budget 800 --json "pattern" .` instead of `grep` or `rg`
3. Before changing any function, run `hypergrep --impact "function_name" .` to check blast radius
4. When you need to read a function's code, use `hypergrep -s "function_name" .` instead of reading the whole file
5. To check if a library/technology is used, use `hypergrep --exists "name" .` instead of searching
