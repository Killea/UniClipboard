# Security Policy

UniClipboard is a security-oriented, end-to-end-encrypted clipboard sync tool.
This document explains how to report vulnerabilities and how to verify the
integrity and authenticity of the binaries we publish.

## Supported Versions

UniClipboard is pre-1.0 and ships from a single active release line. Security
fixes land on the latest released minor; older builds are not maintained.
Please update to the latest release before reporting an issue.

| Version         | Supported          |
| --------------- | ------------------ |
| Latest `0.14.x` | :white_check_mark: |
| Older `0.x`     | :x:                |

## Reporting a Vulnerability

Please report security issues **privately** — do **not** open a public issue for
an unfixed vulnerability.

- Preferred: open a private report through GitHub Security Advisories on this
  repository (the **"Report a vulnerability"** button under the **Security**
  tab). This keeps the disclosure private until a fix is available.

We aim to acknowledge new reports within a few business days and will keep you
updated through triage, the fix, and coordinated disclosure. Thank you for
helping keep UniClipboard users safe.

## Verifying Release Downloads

This self-maintained build ships **without** an in-app auto-updater: the
application never downloads or installs updates on its own. Upgrades are
performed by manually installing a newer release, so the release-artifact
signature below is the single mechanism for verifying downloads.

> On macOS, release builds are additionally Apple-notarized and code-signed, so
> Gatekeeper (`spctl --assess --type execute`) validates the `.app` directly.

### Release artifacts (`SHA256SUMS`)

Starting with the first signed release, every GitHub release includes:

- `SHA256SUMS.txt` — SHA-256 checksums of every release artifact, and
- `SHA256SUMS.txt.minisig` — a minisign signature over that checksum file.

The release-artifact public key is:

```
untrusted comment: minisign public key: 0659AAD44E7EB54C
RWRMtX5O1KpZBhZHfGaa4gqlbwnzJMINb65be0QNzl8RKwK7VOwkMvO8
```

To verify a download:

```sh
# 1. Authenticate the checksum list against the release key
minisign -Vm SHA256SUMS.txt -P 'RWRMtX5O1KpZBhZHfGaa4gqlbwnzJMINb65be0QNzl8RKwK7VOwkMvO8'

# 2. Check your download's integrity against the (now-trusted) list
sha256sum --ignore-missing -c SHA256SUMS.txt          # Linux
# macOS: brew install coreutils, then:
# gsha256sum --ignore-missing -c SHA256SUMS.txt
```

If both checks pass, the file you downloaded is authentic and untampered.
