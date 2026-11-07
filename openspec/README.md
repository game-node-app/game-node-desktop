# OpenSpec documentation status

The stable desktop baseline is Tauri v2 with React and TanStack. Requirements in
`specs/` describe that baseline; they do not describe a transition between UI stacks.

## Authority and lifecycle

- `specs/` is the source of truth for current product requirements.
- Active directories in `changes/` contain proposed deltas. They do not override
  main specs until their changes are accepted and synchronized.
- `changes/archive/` contains historical proposals, designs, tasks and deltas.
  Their instructions and technology choices are not current requirements, even
  when their tasks are marked complete. Consult them only for historical context.
- Guides in `docs/` explain operation and development. They must agree with main
  specs; a guide marked archived is historical and must not direct current work.
- When sources disagree, report the discrepancy. Do not restore removed behavior
  merely to satisfy a historical document or silently treat a proposal as shipped.

Future architecture work must be identified as proposed or accepted future work,
separately from the stable implementation. Superseded plans must not remain in the
current roadmap. Archiving a plan does not retire unrelated features that survived
into the stable baseline.

Compatibility with existing user data remains a current requirement. Ignoring
obsolete settings during upgrade does not make their retired UI features active.

Archive status and superseded material are recorded in
[the historical index](changes/archive/README.md).
