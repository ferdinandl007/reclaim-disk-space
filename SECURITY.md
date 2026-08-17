# Security

This project can delete files when explicitly instructed. Treat it as privileged filesystem tooling.

## Safe-use rules

- Run a read-only plan first.
- Review the exact canonical root and close the owning application.
- Never pass a home directory, system root, broad volume root, unresolved variable, or unreviewed glob.
- Do not use it to remove credentials, synchronized data, databases, model checkpoints, datasets, simulator state, Docker volumes, or source trees without application-aware review and a backup.
- Run only trusted copies of the scripts and inspect changes before executing them.

## Reporting a vulnerability

Please report a reproducible safety bypass, path traversal, privilege escalation, or data-loss bug privately to the repository owner before public disclosure. Include the operating system version, command line, expected guard behavior, and observed behavior. Do not attach private scan reports or secrets.
