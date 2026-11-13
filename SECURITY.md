# Security policy

DiskWiz deletes files and can ask for administrator rights, so safety bugs matter.

## Reporting a vulnerability

Please report problems privately through GitHub:
**Security → Report a vulnerability** on <https://github.com/violetes631/diskwiz/security/advisories/new>.
Do not open a public issue for something that could cause data loss or privilege escalation.

Useful details: your OS and version, the DiskWiz version (`diskwiz --version`), and the exact
steps or command that triggers it. I will answer as soon as I can.

## What counts

- A cleanup item removing something outside the folders it documents.
- The administrator helper (`--root`) doing more than running the known administrator items.
- A path-safety bypass (`is_dangerous_target`, `caches.txt` handling).

Only the latest release is supported.
