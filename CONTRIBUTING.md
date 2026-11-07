# Contributing

These conventions apply to developers and AI agents contributing to GameNode Desktop.
See [development setup and checks](docs/development.md) for build instructions.

## Commit messages

Use Conventional Commits:

```text
type(scope): imperative summary
```

- Write messages in English, matching the existing history.
- Use a lowercase type and scope. The scope identifies the affected area; omit it only
  when no single area is appropriate.
- Keep the subject concise (prefer 50 characters, at most 72), with no trailing period.
- Use an imperative summary: `add`, `fix`, `remove`, not `added` or `adds`.
- Add a body when the motivation, compatibility impact, or migration instructions are
  not obvious. Explain why rather than repeating the diff.
- Mark breaking changes with `!` before the colon and explain them in a
  `BREAKING CHANGE:` footer. Reference relevant issues when applicable (`Refs #123`
  or `Closes #123`).

### Types

| Type | Purpose | Example |
| --- | --- | --- |
| `feat` | Application functionality | `feat(library): add provider filter` |
| `fix` | Application bug fix | `fix(auth): handle expired sessions` |
| `docs` | Documentation and guides | `docs(linux): explain URI registration` |
| `chore` | Repository maintenance and collaboration tooling | `chore(github): add bug report template` |
| `build` | Build system, packaging, and dependencies | `build(deps): update slint` |
| `ci` | Continuous integration workflows | `ci(github): run workspace tests` |
| `test` | Tests and test fixtures | `test(auth): cover rejected callbacks` |
| `refactor` | Code restructuring without behavior changes | `refactor(core): simplify matching` |
| `perf` | Performance improvements | `perf(library): reduce snapshot allocations` |
| `style` | Formatting-only changes, not UI behavior or appearance | `style(core): apply rustfmt` |
| `revert` | Reverting a previous change | `revert(auth): revert callback change` |

Choose the type by intent, not by the file extension. An issue template is
`chore(github)`, not an application `feat` or a documentation guide. A user-visible
UI fix is `fix`, not `style`. Tests included with a feature or fix do not require a
separate `test` commit.

### Scopes and change boundaries

Prefer existing, recognizable areas: `auth`, `library`, `providers`, `core`,
`platform`, `api`, `ui`, `linux`, `windows`, `deps`, `github`, or `openspec`.
Use a more specific scope when useful; this is a starting vocabulary, not a closed list.

Keep unrelated changes in separate commits. Do not mix an issue template or a guide
with application behavior changes solely because they were edited in the same session.
For related multi-area changes, choose the main area or omit the scope.
Do not rewrite existing history just to enforce this convention.

Examples of filtering history:

```sh
git log --oneline --grep='^chore(github):'
git log --oneline --grep='^docs'
```
