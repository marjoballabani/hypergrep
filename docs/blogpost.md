---
title: Your AI Agent Wastes 87% of Its Tokens Just Finding Code. I Fixed That.
published: true
description: AI coding agents waste 60-80% of tokens on grep + file reads. Hypergrep returns function bodies, call graphs, and impact analysis instead of raw lines. 87% reduction measured on real codebases. Works with Claude Code, Cursor, Copilot. Open source, built in Rust.
tags: rust, ai, opensource, devtools
canonical_url: https://marjoballabani.github.io/hypergrep/
series: hypergrep
cover_image: https://marjoballabani.github.io/hypergrep/cover.png
---

## Or: How I Stopped Worrying and Learned to Love the Trigram

You know that feeling when you ask an AI agent to fix a simple bug, and it spends 45 seconds reading your entire codebase before changing 3 lines?

I do. I watched it happen. Repeatedly. So I decided to count exactly how bad it was.

Turns out, **60-80% of the tokens your AI agent consumes go to navigation** -- searching for code, reading files, searching again, reading more files. Not reasoning. Not writing code. Just finding things.

It's like hiring a plumber who spends 4 hours opening every door in your house before fixing the one leaking pipe in the bathroom.

So I built [Hypergrep](https://github.com/marjoballabani/hypergrep). And the plumber now has a floor plan.

---

### Table of Contents

- [The Problem Nobody Talks About](#the-problem-nobody-talks-about)
- [The Experiment](#the-experiment)
- [How Hypergrep Works](#how-hypergrep-works)
- [The Secret Sauce: Semantic Compression](#the-secret-sauce-semantic-compression)
- [Real Benchmarks (No Hand-Waving)](#real-benchmarks-no-hand-waving)
- [The Feature That Changes Everything](#the-feature-that-changes-everything)
- [Everything Else It Does](#everything-else-it-does)
- [What I Got Wrong](#what-i-got-wrong)
- [The Prior Art](#the-prior-art)
- [Install and Try It](#install-and-try-it)

---

## The Problem Nobody Talks About

Every AI coding agent -- Claude Code, Cursor, Copilot, Cody, aider -- uses the same approach to understand your code:

```
1. grep for something
2. get raw text lines back
3. have no idea what those lines mean
4. read the full file to understand context
5. repeat 50-200 times per session
```

This is the equivalent of navigating a city by reading street signs one at a time. It works. It's just painfully inefficient.

Jake Nesler measured this in early 2026 and found that a single question consumed ~12,000 tokens when the actual answer required ~800. The agent read 25 files to locate 3 functions. That's a 93% waste ratio.

And this isn't a bug. It's the architecture. grep was built for humans who want to see matching lines. AI agents don't want lines -- they want understanding.

---

## The Experiment

I picked a real codebase (ripgrep's own source code -- 208 files, 52K lines, because irony is the best testing methodology) and measured what happens when an agent investigates how the `Matcher` system works.

### Approach A: Agent with ripgrep

```
Step 1: rg "Matcher"
         -> 376 matching lines across dozens of files
         -> 10,174 tokens consumed
         -> Agent's internal monologue: "cool, 376 lines. I understand nothing."

Step 2: Read 5 files to understand context
         -> auth.rs, session.rs, middleware.rs...
         -> 9,284 tokens consumed
         -> Agent: "OK, I think I'm starting to get it..."

Step 3: rg "impl.*Matcher" (refine search)
         -> 43 lines
         -> 1,122 tokens consumed
         -> Agent: "Now I can actually answer the question"
```

**Total: 20,580 tokens. 5+ tool calls. Agent spent 45% of budget reading files.**

### Approach B: Agent with Hypergrep

```
Step 1: hypergrep --model
         -> Codebase overview: directory structure, key abstractions, entry points
         -> 1,413 tokens
         -> Agent: "I know this codebase now."

Step 2: hypergrep --layer 1 --budget 1000 "Matcher"
         -> Top results with function signatures + who calls what
         -> 1,400 tokens
         -> Agent: "I see the Matcher trait, its implementations, and the call chain."

Step 3: hypergrep --impact "Matcher"
         -> What breaks if Matcher changes
         -> 1 token
         -> Agent: "And I know the blast radius."
```

**Total: 2,814 tokens. 3 tool calls. Zero file reads.**

Same understanding. **87% fewer tokens.** Not estimated. Measured.

```
Tokens consumed:

ripgrep:    ==================================================  20,580
hypergrep:  ======                                               2,814
                                                         87% reduction
```

---

## How Hypergrep Works

Hypergrep is **not** a faster grep. Let me be clear about this because I wasted a lot of time thinking the goal was speed.

ripgrep is faster for single text searches: 23ms vs 40ms. If you need to grep once and leave, use ripgrep. Seriously.

Hypergrep is a different tool entirely. It combines three capabilities that no other single tool provides:

### 1. Trigram-Indexed Text Search

When you search for "authenticate", Hypergrep doesn't scan every file. It looks up which files contain the trigrams "aut", "uth", "the", "hen", "ent", "nti", "tic", "ica", "cat", "ate" -- and only checks those files.

```
All 208 files
     |
     | trigram filter (which files contain these 3-char sequences?)
     v
12 candidate files
     |
     | regex verification (does the regex actually match?)
     v
3 true matches
```

This is the same technique behind Google Code Search (Russ Cox, 2012). Zero false negatives -- if a file matches, it's guaranteed to be in the candidate set. The filter can only remove files that provably cannot match.

First query builds the index (~40ms for 200 files, cached to disk). After that, searches are 4.4ms. Seven times faster than ripgrep's 31ms.

### 2. Tree-sitter Structural Awareness

Here's where it gets interesting.

ripgrep returns this:
```
src/auth.rs:47:    let hashed = hash_password(pass);
```

Line 47. Great. Which function is this in? What are the arguments? What does the function return? The agent doesn't know. It has to read the file.

Hypergrep returns this:

```bash
$ hypergrep -s "hash_password" src/

src/auth.rs:1-8 function authenticate
fn authenticate(user: &str, pass: &str) -> bool {
    let hashed = hash_password(pass);
    check_db(user, hashed)
}
```

The **complete function body**. The agent immediately knows: this is the `authenticate` function, it takes a user and password, it calls `hash_password` and `check_db`, and it returns a bool.

No file read needed. The search result *is* the understanding.

This works because Hypergrep parses every file with tree-sitter during indexing. It knows where every function, class, struct, method, and trait boundary is. When a line matches, it expands to the smallest enclosing symbol.

**16 languages supported**: Rust, Python, JavaScript, TypeScript, Go, Java, C, C++, Ruby, PHP, Swift, C#, Scala, Lua, Zig, Bash. Plus tree-sitter parsing for HTML, CSS, JSON, TOML, YAML, and HCL.

If a file is in a language without a grammar, it falls back to regular line-level search. Same as ripgrep. No worse.

### 3. Live Call Graph

During indexing, Hypergrep also extracts function call relationships from the AST. It builds a call graph: function A calls function B, function C calls function A.

This enables two commands that no grep tool can answer:

```bash
# Who calls this function?
$ hypergrep --callers "authenticate" src/
  src/api.rs:login_handler
  src/api.rs:api_key_verify
  src/middleware.rs:auth_middleware

# What does this function call?
$ hypergrep --callees "authenticate" src/
  src/auth.rs:hash_password
  src/auth.rs:check_db
  src/session.rs:create_session
```

Response time: **2.5 microseconds**. Not milliseconds. Microseconds. It's a hash table lookup.

---

## The Secret Sauce: Semantic Compression

Speed is nice, but the real innovation is **information density per token**.

I built three output layers:

| Layer | What you get | Tokens | When to use |
|-------|-------------|--------|-------------|
| `--layer 0` | File + function name + kind | ~15 | "Which files are relevant?" |
| `--layer 1` | Signature + calls + called_by | ~80-120 | "What does this do?" (sweet spot) |
| `--layer 2` | Full source code | ~200-800 | "I need to edit this function" |

Layer 1 is the sweet spot. Here's what the agent actually receives:

```json
[
  {
    "name": "search",
    "kind": "function",
    "file": "crates/core/main.rs",
    "line_range": [107, 151],
    "signature": "fn search(args: &HiArgs, mode: SearchMode) -> Result<bool>",
    "calls": ["search_path", "matcher", "printer", "walk_builder"],
    "called_by": ["search_parallel", "run", "try_main"],
    "tokens": 85
  }
]
```

85 tokens. The agent knows:
- The function name, file, and line range
- The full type signature
- Everything it calls (dependencies going down)
- Everything that calls it (dependents going up)

Without Hypergrep, getting this understanding means reading `main.rs` (~2,000 tokens) and probably `search.rs` too (~3,000 tokens). That's 5,000 tokens for what Hypergrep delivers in 85.

And then there's the budget parameter:

```bash
hypergrep --layer 1 --budget 500 --json "authenticate" src/
```

"Give me the best results that fit in 500 tokens." Hypergrep ranks by relevance and fills the budget with the top results. The agent gets maximum information density within its context constraints.

No other search tool has this concept. grep returns everything. You deal with the overflow. Hypergrep optimizes for the consumer.

---

## Real Benchmarks (No Hand-Waving)

Every number here is from a real run. I benchmarked against ripgrep's own source code (208 Rust files, 52K lines) because I wanted a real project, and also because benchmarking a search tool against the search tool's own code felt appropriately meta.

### Speed

```
Query latency (warm index, median of 20 runs):

"fn search"        hypergrep  4.5ms  ||||
                   ripgrep   31.0ms  |||||||||||||||||||||||||||||||

"Searcher"         hypergrep  3.7ms  |||
                   ripgrep   31.0ms  |||||||||||||||||||||||||||||||

"TODO"             hypergrep  0.5ms  |
                   ripgrep   31.0ms  |||||||||||||||||||||||||||||||

"Result<"          hypergrep  4.9ms  ||||
                   ripgrep   31.0ms  |||||||||||||||||||||||||||||||
```

Hypergrep is **7x faster** for warm queries. ripgrep is constant at ~31ms because it scans every file every time. Hypergrep varies from 0.5ms to 7.5ms depending on how many candidates the trigram filter produces.

### 50-Query Agent Session

```
Cumulative time:

Queries:     1     5    10    20    50   100
ripgrep:    31   155   310   620  1550  3100 ms
hypergrep:   4    22    44    88   220   440 ms
                                   ^^^
                                  7x faster
```

Over a 50-query session: ripgrep takes 1,550ms. Hypergrep takes 220ms. The gap widens linearly because Hypergrep pays the index cost once.

### Token Savings

```
Investigation task: "How does Matcher work?"

                          Tokens
ripgrep + file reads:     ████████████████████░  20,580
hypergrep (L1 + budget):  ██░                     2,814

                          87% reduction
```

### The Full Numbers Table

| Metric | ripgrep | Hypergrep | |
|--------|---------|-----------|--|
| Warm text search | 31ms | **4.4ms** | 7x faster |
| 50-query session | 1,550ms | **220ms** | 7x faster |
| Tokens (3-query task) | 20,580 | **2,814** | 87% less |
| Callers query | impossible | **2.5us** | new capability |
| Existence check | 31ms | **291ns** | 100,000x faster |
| Codebase summary | N/A | **699 tokens** | new capability |
| Correctness | baseline | **5/5 match** | zero false negatives |

All numbers from warm index. Cold start (first-ever run, no cache) is 40ms for text search, 800ms including tree-sitter + graph. The index is cached to disk (`.hypergrep/index.bin`, ~581 KB) and loads in 25ms on subsequent runs.

---

## The Feature That Changes Everything

Impact analysis.

```bash
$ hypergrep --impact "hash_password" src/
```

```
Impact analysis for 'hash_password' (depth 3):

  [depth 1] WILL BREAK   src/auth.rs:authenticate
  [depth 2] MAY BREAK    src/api.rs:login_handler
  [depth 3] REVIEW        src/main.rs:setup_routes
```

```
Call graph:

  setup_routes -----> login_handler -----> authenticate -----> hash_password
   [REVIEW]            [MAY BREAK]         [WILL BREAK]        [you change this]
```

No AI agent today checks blast radius before editing. They just edit and hope. "I changed the return type of `hash_password` from `String` to `Vec<u8>`" -- and three files break.

With `--impact`, the agent sees the damage before it happens. In 2.5 microseconds.

This alone is worth the install. Everything else is a bonus.

---

## Everything Else It Does

### Codebase Mental Model

```bash
$ hypergrep --model "" src/
```

Generates a ~700 token structural summary of your entire codebase:

```
# Codebase Mental Model

## Languages
- Rust: 100+ files, TypeScript: 8 files

## Structure
- src/auth/ (3 files) -- 5 functions, 2 structs
- src/api/ (6 files) -- 12 functions, 3 structs

## Key Abstractions
- function search (main.rs) -- 16 callers, 8 callees
- struct Searcher (searcher/mod.rs) -- 12 callers

## Entry Points
- src/main.rs

## Hot Spots
- src/api/handlers.rs (15 symbols, 340 lines)
```

Load this once at session start. The agent immediately knows where everything is. Replaces 10-20 exploratory searches.

### Bloom Filter Existence Checks

"Does this project use Redis?"

Every agent eventually asks this. With ripgrep, it's a full codebase scan that returns zero results. 31ms to learn nothing.

With Hypergrep:

```bash
$ hypergrep --exists "redis" src/
NO: 'redis' is definitely not in this codebase    # 291 nanoseconds
```

291 nanoseconds. That's not a typo. It's a bloom filter lookup.

The bloom filter parses Cargo.toml, package.json, go.mod, requirements.txt, and pyproject.toml for real dependency names. "NO" is guaranteed correct (zero false negatives). "YES" means "probably" (~2% false positive rate).

### Daemon Mode

For heavy sessions (50+ queries), the daemon keeps the index in memory:

```bash
$ hypergrep-daemon --background src/
Daemon started (PID 18067)

$ hypergrep-daemon --status src/
Running
  PID:    18067
  Memory: 8.5 MB
  Socket: /tmp/hypergrep-f983e88f.sock
```

**Safety features** (because nobody wants a rogue process eating their RAM):
- Auto-stops after 30 min idle
- Hard memory cap at 500 MB
- 0% CPU when idle
- PID file prevents duplicates
- `--stop` to kill it manually

8.5 MB for 100 files. Less than a Chrome tab.

---

## What I Got Wrong

I want to be honest about the limitations because too many project READMEs aren't.

**Cold start is slower than ripgrep.** On first run with no cache, Hypergrep takes 40ms for text search vs ripgrep's 23ms. With tree-sitter + graph, it's 800ms. The index pays for itself after ~40 queries. If you're doing one search and leaving, use ripgrep.

**The call graph is incomplete.** Static analysis can't see dynamic dispatch (`getattr(obj, "method")()`), reflection, callbacks passed as arguments, or macro-generated code. The `--impact` results will miss things. It's useful for orientation, not for guarantees. If you need 100% accuracy, use your language's type checker.

**Binary is 29 MB.** 16 tree-sitter grammars embedded. It's a chunky boy. ripgrep is 5 MB. The tradeoff is structural understanding for 16 languages vs text matching.

**Large codebases (>10K files) need daemon mode.** The CLI cold start with tree-sitter parsing is too slow. The daemon builds once, serves forever (well, for 30 minutes of idle time, then politely exits).

**Bloom filter has ~2% false positives.** If it says "YES, this project uses graphql", it might be because "graphql" appears in a test fixture string, not because GraphQL is actually used. "NO" is always correct though.

---

## The Prior Art

I didn't invent most of the techniques. Here's what existed before and what Hypergrep actually adds:

| System | Year | Text search | Code graph | Structural | Token compression |
|--------|------|-------------|------------|------------|-------------------|
| Google Code Search | 2006 | Trigram index | - | - | - |
| livegrep | 2015 | Suffix array | - | - | - |
| Zoekt | 2016 | Positional trigram | - | ctags | - |
| GitHub Blackbird | 2023 | Sparse n-grams | - | - | - |
| ast-grep | 2023 | Pattern (no index) | - | Tree-sitter | - |
| Axon | 2025 | - | Call graph | Tree-sitter | - |
| codebase-memory-mcp | 2025 | - | Call graph | Tree-sitter | - |
| Cursor indexed search | 2025 | Client-side n-gram | - | - | - |
| **Hypergrep** | **2026** | **Trigram index** | **Call graph** | **Tree-sitter** | **L0/L1/L2 + budget** |

Text search tools can't do graph queries. Graph tools can't do regex search. Nobody does semantic compression with token budgets. Hypergrep is the first to combine all four.

The full research document (42 references, including the theoretical foundations from Cox 2012, GitHub Blackbird 2023, and the speculative execution papers): [RESEARCH.md](https://github.com/marjoballabani/hypergrep/blob/main/RESEARCH.md)

---

## Install and Try It

30 seconds:

```bash
# macOS / Linux (downloads pre-built binary)
curl -sSfL https://github.com/marjoballabani/hypergrep/releases/latest/download/hypergrep-installer.sh | sh

# Or from source
git clone https://github.com/marjoballabani/hypergrep.git
cd hypergrep && ./install.sh
```

Try it on your project:

```bash
# See the codebase overview
hypergrep --model "" src/

# Search with semantic compression
hypergrep --layer 1 --budget 500 --json "your_function" src/

# Check blast radius before refactoring
hypergrep --impact "your_function" src/

# Does your project use something?
hypergrep --exists "redis" src/
```

### Set Up Your AI Agent

One command configures Claude Code, Cursor, Copilot, and Windsurf:

```bash
./hypergrep-setup.sh /path/to/your/project
```

Your agents will automatically use Hypergrep for code search. No other setup needed.

### Uninstall

If it's not for you:

```bash
rm -f $(which hypergrep) $(which hypergrep-daemon)
find ~ -name ".hypergrep" -type d -exec rm -rf {} + 2>/dev/null
```

Clean. Nothing left behind.

---

## What's Next

The daemon needs real-world battle testing with actual agent sessions. The call graph needs type-aware resolution (not just name matching). And I want to build an MCP server so agents can use Hypergrep as a native tool, not just a Bash command.

But the core thesis is proven: **agents don't need faster text search. They need smarter results.** Return function signatures instead of lines. Return call graphs instead of file contents. Return impact analysis instead of making the agent guess.

87% token reduction. Measured, not projected. On a real codebase. With 120 tests.

---

**Links:**
- GitHub: [github.com/marjoballabani/hypergrep](https://github.com/marjoballabani/hypergrep)
- Docs: [marjoballabani.github.io/hypergrep](https://marjoballabani.github.io/hypergrep)
- Research (42 refs): [RESEARCH.md](https://github.com/marjoballabani/hypergrep/blob/main/RESEARCH.md)
- Benchmarks: [BENCHMARKS.md](https://github.com/marjoballabani/hypergrep/blob/main/BENCHMARKS.md)

MIT License. 120 tests. 16 languages. Built with Rust.

If you use AI coding agents and burn tokens on navigation, give it a try. Star the repo if it saves you money. Open an issue if it doesn't.

---

**Tags:** #rust #cli #ai #devtools #opensource #codesearch #programming #rustlang
