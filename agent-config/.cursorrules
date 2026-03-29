This project has `hypergrep` installed globally. Use it instead of grep/ripgrep for ALL code search. It returns structural results (function bodies, signatures, call graphs) instead of raw lines, using 87% fewer tokens.

Finding code (replaces grep/rg):
- `hypergrep --layer 1 --budget 800 --json "pattern" .` -- signatures + call graph, capped at 800 tokens
- `hypergrep -s "pattern" .` -- full function bodies (when you need to edit)
- `hypergrep -l "pattern" .` -- file paths only

Understanding code:
- `hypergrep --model "" .` -- codebase overview (run ONCE at session start)
- `hypergrep --callers "function_name" .` -- who calls this?
- `hypergrep --callees "function_name" .` -- what does this call?

Before making changes:
- `hypergrep --impact "function_name" .` -- what breaks if I change this? ALWAYS run before editing.

Quick checks:
- `hypergrep --exists "redis" .` -- does this project use X? (NO = guaranteed, YES = likely)

Rules:
1. Run `hypergrep --model "" .` at session start to understand the codebase
2. Use `hypergrep --layer 1 --budget 800 --json` instead of grep/rg for finding code
3. Run `hypergrep --impact` before changing any function
4. Use `hypergrep -s` instead of reading whole files when you need function bodies
5. Use `hypergrep --exists` to check for technologies instead of searching
