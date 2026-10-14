# Security

Clear Compare handles user-selected local files. A flaw in path validation, exclusion rules, IPC or synchronization can affect file integrity.

For a potential vulnerability, use GitHub's private vulnerability reporting if available on this repository; otherwise contact the repository owner privately through their GitHub profile before publishing sensitive details. Do not attach credentials, real project data or private comparison reports to public issues.

For ordinary bugs, provide the app version, operating system and a synthetic reproduction. Preserve the backup journal when reporting a synchronization failure, but redact local paths and file names before sharing it.

The preview release is not a sandbox for adversarial local processes. Local revalidation narrows accidental overwrite risk; it does not provide a multi-file transaction or immunity to arbitrary concurrent filesystem manipulation.
