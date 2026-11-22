# steamapps-scan

A generic Rust crate and CLI for locating Steam installations, loading a
validated inventory of installed apps, and finding executable paths by app ID
plus exact relative path. Automatic discovery covers Windows, Linux, and
macOS; explicit-root scans work on all three. macOS has been validated against
a default-location install; libraries on external volumes have not yet been
exercised.

```rust,no_run
use steamapps_scan::{Discovery, ExecutableQuery};

let discovery = Discovery::discover();

let editor = ExecutableQuery::any_of(123_456, [
    "Editor/EditorDiagnostic.exe",
    "Editor/Editor.exe",
])?;

let report = discovery.resolve_executable(&editor);
if let Ok(found) = report.unique() {
    println!("{}", found.executable());
}

# Ok::<(), steamapps_scan::QueryErr>(())
```

## Why inventory first?

One Steam app can ship several programs. Looking up only an app ID cannot say
which executable the caller wants. `steamapps-scan` loads manifests once,
then evaluates independent queries such as:

```rust,no_run
# use steamapps_scan::ExecutableQuery;
let game = ExecutableQuery::new(123_456, "Game.exe")?;
let server = ExecutableQuery::new(123_456, "DedicatedServer.exe")?;
# Ok::<(), steamapps_scan::QueryErr>(())
```

Each match carries the exact `AppInstall` that supplied it, so duplicate app
installs across libraries cannot be mixed together.

## Reporting

Discovery is non-lossy. The report includes:

- every accepted Steam root, library, and valid app manifest;
- every root lookup attempt and its source/outcome;
- structured diagnostics with severity, stage, kind, path, source, and app ID;
- an overall `complete`, `partial`, `not_found`, or `unsupported` state.

Executable reports separately describe match cardinality (`none`, `one`, or
`many`) and whether the result is complete or partial. State, cardinality, and
completeness are derived from the retained evidence, so they cannot disagree
with the inventory, matches, or diagnostics. The same derived values remain in
serialized reports.

Failed root attempts link to their structured diagnostic by index rather than
copying the failure message. `Discovery::attempts_with_diagnostics` resolves
those links directly. Executable reports share immutable scan diagnostics and
keep query-specific diagnostics separately; `MatchReport::diagnostics` iterates
over both in reporting order.

On Windows, automatic discovery checks:

- `HKCU\Software\Valve\Steam\SteamPath`;
- `HKLM\SOFTWARE\Wow6432Node\Valve\Steam\InstallPath`;
- `HKLM\SOFTWARE\Valve\Steam\InstallPath`;
- running `steam.exe` processes, enumerated directly through the Tool Help
  API rather than by spawning a shell;
- environment-derived and conventional install paths.

On Linux, automatic discovery checks `$XDG_DATA_HOME/Steam`, the
`~/.local/share/Steam` default, the client-maintained `~/.steam/steam` and
`~/.steam/root` pointers, `~/.steam/debian-installation`, and the Flatpak and
Snap locations under `~/.var/app/com.valvesoftware.Steam` and
`~/snap/steam/common`. The `~/.steam` pointers are written by the Steam
client wherever it actually runs from, so custom install locations are found
without a process probe. Only the modern lowercase `steamapps` layout is
supported.

On macOS, automatic discovery checks `~/Library/Application Support/Steam`,
the only location the client uses for its data tree. This has been validated
against a default-location install; libraries on external volumes have not
yet been exercised.

Compared with [`steamlocate` 2.1.0](https://github.com/WilliamVenner/steamlocate-rs/tree/d7a249667ceeaadffba285547106e8d809507ae9),
this crate is focused on broader root discovery, validated executable path
resolution, and retaining partial results plus all subordinate diagnostics.
`steamlocate` additionally parses non-Steam shortcuts (`shortcuts.vdf`, a
binary format) and Proton compat-tool mappings; both are out of scope here.
Explicit-root scans with this crate use only portable `std::fs` operations
and support Steam-compatible layouts on Windows, Linux, and macOS.

## App metadata

Each inventoried app carries advisory manifest metadata parsed leniently:
install-state flags, build ID, size on disk, last-updated time, selected
beta branch, and installed depots. A missing or malformed value is `None`
rather than an inventory error, and a manifest whose optional metadata
cannot be parsed at all still enters the inventory with a warning
diagnostic. `AppInstall::content_signal` remains the strict reader for
callers that need the same fields verified. Depot keys that collide after
numeric parsing are omitted as ambiguous and retain a warning diagnostic.
A value that is present but malformed, including a zero build, depot,
manifest, or DLC ID, and a depot key that is not a depot ID each leave a
warning diagnostic naming what was omitted; a missing value leaves none.

## Executable versions

A resolved `ExecutablePath` can report the version its file advertises,
read from the Windows `VS_VERSIONINFO` resource: the same data the
Explorer property sheet shows. It is a plain file read; nothing is
executed.

```rust,no_run
# use steamapps_scan::ExecutablePath;
# fn example(exe: &ExecutablePath) -> Result<(), steamapps_scan::VersionErr> {
if let Some(version) = exe.version()? {
    println!("{:?}", version.product_version);
}
# Ok(())
# }
```

The resource is optional, so an executable carrying none reports
`Ok(None)` rather than failing. It holds two independent records that
routinely disagree, so both are reported and neither is parsed:
`VS_FIXEDFILEINFO` is four integers, while the string block is free text
its author chose. Observed in the wild: `1.7.0.54`, `1, 0, 0, 1`,
`01.00.00.02`, and ARC Raiders, whose string is
`pioneer_2.03.x-CL-1450810` while its fixed record reads `5.7.4.0`.
Vendors also leave one record empty while populating the
other, so an absent fixed record implies nothing about the strings.

The resource is parsed from the file itself rather than through a Windows
API, so every target reads it the same way, including Windows games
installed through Proton on Linux, and callers need no `cfg`.

## Manual roots

A caller can bypass automatic location and scan known roots on any platform
with a Steam-compatible filesystem layout:

```rust,no_run
use steamapps_scan::Discovery;

let discovery = Discovery::discover_from(["/mnt/games/Steam"]);
println!("{} apps", discovery.inventory().apps().len());
```

## CLI

The `cli` feature builds a `steamapps-scan` binary over the same public
API:

```text
steamapps-scan scan                 # full report; --json for the serialized form
steamapps-scan apps --app-id 123    # list inventoried installations
steamapps-scan resolve 123 Game.exe # resolve executables; --version reads versions
steamapps-scan signal 123          # content signal for a unique installation
```

Every subcommand accepts repeated `--root <DIR>` to scan explicit roots
instead of automatic discovery, and `--json` to emit the serialized report.
Exit codes: 0 success, 1 nothing found, 2 usage error, 3 ambiguous result,
4 partial evidence, 5 platform unsupported. Discovery errors or an unsupported
state take precedence over a subcommand's match result; a requested version
read or JSON serialization failure is also partial. Steam app ID zero is a
usage error.
