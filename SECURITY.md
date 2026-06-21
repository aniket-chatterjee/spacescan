# Security Policy

## Reporting

Please report security issues privately by opening a GitHub security advisory once the repository is public.

Avoid posting exploit details in public issues until a fix is available.

## Scope

`spacescan` reads filesystem metadata and can delete files from the TUI. Security-sensitive areas include path validation, delete guards, symlink/reparse-point handling, export paths, and platform file-manager launching.
