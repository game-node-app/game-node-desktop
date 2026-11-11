# Plugin execution environment detection

`gamenode_platform::plugin_environment::detect()` asynchronously reports OS facts visible
to the current host process. It schedules filesystem and OS probes on Tokio's blocking pool.
It never starts a plugin, package entrypoint, bundled runtime, package command, or installer hook.

On Linux, the detector reads `/etc/os-release` (falling back to `/usr/lib/os-release`) for
the effective userspace distribution ID and version, reads `uname` for the visible kernel
version and machine, and derives the current process architecture and ABI from the running
process and mapped libc. It distinguishes glibc from musl where possible and reports a libc
version only when the mapped library exposes a recognized version string. Unrecognized
`os-release` fields such as `ID_LIKE` are preserved as diagnostics; ancestry does not mean
that a package declaring a different distribution is compatible.

On Windows, the detector uses `RtlGetVersion` to avoid compatibility-manifest version
virtualization, reports the Windows version/build and client/server product type, obtains
the product SKU, and uses `IsWow64Process2` to compare process and native architectures.
Unknown values stay unknown. Unsupported platforms return an explicit unknown result.

The result distinguishes userspace facts from visible kernel/native-host facts. In a container,
`os-release` describes the container userspace while `uname` usually describes the host kernel.
If GameNode later launches plugins inside another namespace or compatibility layer, detection
must run from that same execution context before evaluating that plugin. Linux architecture
comparison detects common foreign-architecture emulation when the kernel reports a different
machine, but cannot identify every emulator or container boundary. Windows architecture
comparison identifies WOW64 process translation; it does not certify arbitrary emulators.

Callers must apply an explicit `EmulationPolicy`: `RequireNative`, `AllowEmulated`, or
`AllowUnknown`. Detection itself never enables emulation or treats missing facts as compatible.
This module only reports facts for later compatibility evaluation. It does not guarantee that
all shared libraries, interpreters, GPU drivers, or plugin-specific dependencies are present,
and it does not make a safety claim about plugin code.
