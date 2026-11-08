# features/ports

Layout for the **PC ports** destination. The shell (task 7.1) owns the sidebar
item, `activeDestination` and the single `ports_open` call that starts the first
fetch. `PortsView` reads `Snapshot.ports` and mounts the catalog plus the guided
setup host.

```
features/ports/
├── PortsView.tsx   # exported `PortsView`; toolbar, catalog, detail pane, credit
├── catalog/        # toolbar, virtualized grid/list, art, badges, states, credit
├── detail/         # port detail pane and install progress row
├── setup/          # guided setup FloatingModal (owned by tasks 7.4–7.5)
└── index.ts        # `export * from './PortsView'`
```

Rules:

- Import every type, command, event and hook from `../../api/ports`
  (`usePortsView`, `usePortProgress`, `portArtUrl`, `commands.ports*`). The file
  documents the host contract; do not redeclare payload shapes here.
- Keep `PortsView` as the exported component name and keep `index.ts` exporting it.
- `PortsView` mounts `<PortSetupHost />` exactly once; never inside the detail pane.
- Use Tailwind utilities and the `@tabler/icons-react` set, like the library.
- Use `port-art` (`portArtUrl`) for catalog artwork and keep
  `<img loading="lazy" decoding="async" />` with a titled placeholder fallback.
- The upstream credit "Port catalog by quiverlauncher.com" is text only; never
  use Quiver artwork or logos.
