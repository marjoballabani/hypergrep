This project has `hypergrep` installed globally. Use it instead of grep/ripgrep when you need structural understanding of code (function bodies, call graphs, impact analysis). For simple text searches, ripgrep is fine.

When to use hypergrep vs ripgrep:
- Quick text search -> `rg "pattern" .` (faster for one-off)
- Find + understand a function -> `hypergrep -s "function" .`
- Search with token budget -> `hypergrep --layer 1 --budget 800 --json "pattern" .`
- Who calls this / what breaks -> `hypergrep --callers` / `--impact`
- Does project use X -> `hypergrep --exists "redis" .`
- Codebase overview -> `hypergrep --model "" .`

Session start:
- `hypergrep --model "" .` -- understand the codebase (run once)
- `hypergrep-daemon --background .` -- optional, start if making 10+ searches (auto-stops after 30 min)

Commands:
- `hypergrep --layer 1 --budget 800 --json "pattern" .` -- signatures + call graph in 800 tokens
- `hypergrep -s "pattern" .` -- full function bodies (to edit)
- `hypergrep --callers "function_name" .` -- who calls this?
- `hypergrep --callees "function_name" .` -- what does this call?
- `hypergrep --impact "function_name" .` -- what breaks? (run before editing)
- `hypergrep --exists "redis" .` -- technology check (NO = guaranteed, YES = likely)

Rules:
1. Run `hypergrep --model "" .` at session start
2. Use hypergrep when you need to understand code, rg when you just need to find text
3. Run `hypergrep --impact` before changing any function
4. Use `hypergrep -s` instead of reading whole files for specific functions
