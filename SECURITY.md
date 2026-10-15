# Security Policy

## Supported versions

Yata Cloud is in early development and has no release yet.
Once releases exist, only the latest release receives security fixes.

## Reporting a vulnerability

Please do not report security vulnerabilities through public issues, pull requests, or discussions.

Report them privately through GitHub instead:
open the [Security tab](https://github.com/morryyrrom/yata-cli/security) of this repository and select **Report a vulnerability**
(see [Privately reporting a security vulnerability](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)).

Include as much of the following as you can:

- The affected component (CLI, policy engine, AWS provider, report output) and version or commit
- Steps to reproduce, or a proof of concept
- The impact you expect (for example, credential exposure, write access to a cloud account, or code execution through a policy directory)

Do not include real account IDs, credentials, or scan results from your environment; use dummy values.

The maintainer will acknowledge the report, investigate it, and coordinate a fix and disclosure with you.

## Scope

- This repository (`yata` CLI and its crates).
- Vulnerabilities in the official policy set are reported in the same way through the [yata-rules](https://github.com/morryyrrom/yata-rules) repository.
- Issues in third-party dependencies should be reported to their maintainers. If one affects `yata`, a report here is welcome.
