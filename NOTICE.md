# Third-Party Notices

WiseCortex itself is released under the MIT License (see [LICENSE](LICENSE)).

This product bundles skills authored by third parties. Each is used under its
own license, reproduced in full alongside the skill assets. The notices below
satisfy the attribution requirements of those licenses.

Bundled skills live in `crates/core/assets/skills/` and are embedded into the
binary at build time, so they are redistributed with every release.

---

## superpowers

- **Copyright**: Copyright (c) 2025 Jesse Vincent
- **License**: MIT — full text in [`crates/core/assets/skills/LICENSE-superpowers.txt`](crates/core/assets/skills/LICENSE-superpowers.txt)
- **Upstream**: https://github.com/obra/superpowers

Skills used:

`brainstorming`, `dispatching-parallel-agents`, `executing-plans`,
`finishing-a-development-branch`, `receiving-code-review`,
`requesting-code-review`, `skill-creator`, `subagent-driven-development`,
`systematic-debugging`, `test-driven-development`, `using-git-worktrees`,
`using-superpowers`, `verification-before-completion`, `writing-plans`,
`writing-skills`

Note: `writing-skills/anthropic-best-practices.md` reproduces Anthropic's
public "Skill authoring best practices" documentation. It is kept verbatim
because the skill reads it from disk at runtime — an external link would not
work offline. Copyright remains with Anthropic.

## ui-ux-pro-max

- **Copyright**: Copyright (c) 2024 Next Level Builder
- **License**: MIT — full text in [`crates/core/assets/skills/LICENSE-ui-ux-pro-max.txt`](crates/core/assets/skills/LICENSE-ui-ux-pro-max.txt)
- **Upstream**: https://github.com/nextlevelbuilder/ui-ux-pro-max-skill

Skills used: `ui-ux-pro-max`

## ClaudeKit

- **Copyright**: Copyright (c) ClaudeKit
- **License**: MIT — full text in [`crates/core/assets/skills/LICENSE-claudekit.txt`](crates/core/assets/skills/LICENSE-claudekit.txt)

Skills used (declared as `ckm:*` in their front matter, `metadata.author: claudekit`):

`banner-design`, `brand`, `design`, `slides`

---

## Skills original to this project

`generate-image` was written for WiseCortex and is covered by the project's own
MIT license.

---

## Rust and npm dependencies

Third-party crates and npm packages are declared in `Cargo.toml` / `web/package.json`
and are not vendored into this repository. Their licenses apply as published on
crates.io and npm respectively. To review them:

```sh
cargo install cargo-license && cargo license   # Rust
npm --prefix web ls --all                      # npm
```
