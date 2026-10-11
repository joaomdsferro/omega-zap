# Security

Omega Zap holds the keys to your WhatsApp and Signal accounts and your message history, so security
problems matter a great deal.

## Reporting a vulnerability

Please report privately, not in a public issue: use
[GitHub's private vulnerability reporting](https://github.com/joaomdsferro/omega-zap/security/advisories/new).
Include what an attacker could do, how to reproduce it, and the version affected.

You will get an answer within a week. Once a fix is released, the report can be made public, with
credit if you want it.

## Supported versions

Omega Zap is in early testing. Only the latest release gets fixes.

## In scope

- Reading stored messages, media or keys without the user's keyring
- Anything that sends data somewhere the user didn't ask for
- Running code through a message, a link card or a file someone sends
- Weaknesses in how the app encrypts what it stores

Problems in WhatsApp, Signal, `whatsapp-rust` or `presage` themselves should go to those projects.
