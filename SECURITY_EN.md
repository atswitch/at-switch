# Security Policy

[简体中文](./SECURITY.md) | English

## Security and privacy boundaries

- **Credential source storage:** AT-Switch stores Provider API keys and local
  routing tokens in macOS Keychain or Windows Credential Manager. Direct mode also
  writes the selected Provider API key to the target agent's native configuration;
  whether that copy is plaintext depends on the agent. Use local proxy mode to
  avoid this extra copy.
- **No collection of request content:** AT-Switch does not send prompts, model
  responses, tool arguments, or per-request logs to the project maintainers, and
  does not collect or persist them through telemetry. Local proxy mode processes
  requests and responses in memory and forwards requests to the Provider selected
  by the user.
- **Local-first operation:** Configuration management runs locally, and the
  optional proxy listens only on `127.0.0.1`. Model requests go directly or through
  the local proxy to the selected Provider; the project does not operate a central
  relay service.

Providers and agents selected by users may process, log, or retain request data
under their own terms. Those third-party practices are outside AT-Switch's privacy
commitments; review the applicable service policies before use.

## Supported versions

| Version | Security updates |
| --- | --- |
| Latest public release | Supported |
| Earlier releases | Not supported |

## Report a vulnerability

Do not disclose a suspected vulnerability in a public GitHub Issue. Email the
maintainers privately at **atswitchcn@163.com** and include, when possible:

1. The vulnerability type and affected component.
2. Reproduction steps or a proof of concept.
3. Any suggested mitigation or fix.

The maintainers aim to acknowledge reports within 48 hours and begin assessment.
