# Security Policy

## Reporting a vulnerability

If you find a security issue in Hypergrep, please report it responsibly.

**Do NOT open a public issue.** Instead, email: marjo@ballabani.com

Include:
- Description of the issue
- Steps to reproduce
- Impact assessment

I will respond within 48 hours and work with you on a fix before public disclosure.

## Scope

Security-relevant areas in Hypergrep:

- **Daemon Unix socket**: The daemon listens on a Unix socket and returns file contents. Socket permissions are set to 0600 (owner-only). If you find a way to bypass this, that is a security issue.

- **Path traversal**: The search engine reads files within the specified root directory. If you find a way to make it read files outside the root, that is a security issue.

- **Regex denial of service**: The regex engine (Rust's `regex` crate) is designed to be safe against catastrophic backtracking. If you find a pattern that causes excessive CPU usage, report it to the `regex` crate as well.

## Not in scope

- The bloom filter returning false positives (this is by design, not a vulnerability)
- The call graph missing dynamic dispatch calls (this is a known limitation)
- Memory usage growing with large codebases (this is a performance issue, not security)
