This project has `hypergrep` and `hypergrep-daemon` installed globally. Use hypergrep instead of grep/ripgrep for ALL code search. It returns structural results (function bodies, signatures, call graphs) instead of raw lines, using 87% fewer tokens.

Session start (run these first):
- `hypergrep-daemon --background .` -- start daemon (7x faster searches, auto-stops after 30 min idle)
- `hypergrep --model "" .` -- codebase overview (run ONCE to understand the project)

Finding code (replaces grep/rg):
- `hypergrep --layer 1 --budget 800 --json "pattern" .` -- signatures + call graph, capped at 800 tokens
- `hypergrep -s "pattern" .` -- full function bodies (when you need to edit)
- `hypergrep -l "pattern" .` -- file paths only

Understanding code:
- `hypergrep --callers "function_name" .` -- who calls this?
- `hypergrep --callees "function_name" .` -- what does this call?

Before making changes:
- `hypergrep --impact "function_name" .` -- what breaks if I change this? ALWAYS run before editing.

Quick checks:
- `hypergrep --exists "redis" .` -- does this project use X? (NO = guaranteed, YES = likely)

Daemon management:
- `hypergrep-daemon --status .` -- check if running (shows PID + memory)
- `hypergrep-daemon --stop .` -- stop manually (also auto-stops after 30 min idle)

Rules:
1. Run `hypergrep-daemon --background .` then `hypergrep --model "" .` at session start
2. Use `hypergrep --layer 1 --budget 800 --json` instead of grep/rg for finding code
3. Run `hypergrep --impact` before changing any function
4. Use `hypergrep -s` instead of reading whole files when you need function bodies
5. Use `hypergrep --exists` to check for technologies instead of searching
6. Daemon auto-stops after 30 min idle. No cleanup needed.
