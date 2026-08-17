# Contributing

Thanks for helping make disk cleanup safer and faster.

## Scope

Keep the project macOS-native, evidence-first, and conservative about deletion. New classifications should improve interpretation without replacing the dynamic extension table. New cleanup paths must preserve the exact-path confirmation boundary and document regeneration or data-loss consequences.

## Local checks

```sh
make build
```

Run a focused read-only scan against a temporary directory and exercise the cleaner’s refusal paths for `/`, `/System/Volumes/Data`, relative paths, and cross-device roots. Do not test destructive cleanup against real personal data.

Changes to the agent skill should keep `SKILL.md` concise and move detailed stack-specific material into `references/`.

## Pull requests

Describe the macOS version, hardware, command used, scan root, and before/after measurements for performance changes. Never include personal scan reports, home-directory paths, credentials, or application databases in a pull request.
