# steamapps-scan

<img src="assets/steamapps-scan-profile.png" alt="steamapps-scan logo" width="128">

Find games installed with Steam, and the exact `.exe` inside
each one on Linux, Mac, and Windows. A command-line tool and Rust library. 

Useful for MCP servers, CLI tools, launchers, mod managers, etc. It only reads: it never changes files or
starts programs. Or just tell your agent to download and use the CLI. Who needs an MCP server anyways?

## Note from the human

I wrote and designed most of this manually for an MCP server I was building. I wanted something a bit more exhaustive with better error handling than `steamlocate`. I got really bored (as one tends to with dealing with multi-platform file searching systems) and decided to try out vibe-coding for a 'real' project, and quickly finished a v1. After a few months working on the MCP server and game project I came back much more experienced with vibe-coding and finished this properly. Yay. So probably everything here is written by Codex or Claude with the exception of this statement. If that bothers you, I understand. That said, I did my due diligence with testing as I would with any other project. 

## What it does

- Finds Steam, even when it is installed somewhere unusual.
- Finds games on every drive and folder Steam uses, not just the main one.
- Lists each installed game's name, folder, size, and build number.
- Give it a game's Steam app ID and `Bin/Game.exe`; get back the full path,
  checked to exist.
- Reads the version number stored inside an `.exe`, on any OS.
- Keeps going when something is broken, and tells you what and where.
- Scans folders you point it at, including SteamCMD server installs.

## Platforms

- **Windows**: finds Steam through the registry, a running Steam, or the
  usual Program Files folders.
- **Linux**: finds regular, Flatpak, and Snap installs, including ones in
  custom locations.
- **macOS**: finds Steam in its standard location,
  `~/Library/Application Support/Steam`.
- **Anything else**: cannot find Steam on its own, but scans any Steam folder
  you name.

Reading `.exe` versions works the same on every system, so a Windows game
installed through Proton on Linux reports its version too.

## Compared with steamlocate

[`steamlocate`](https://github.com/WilliamVenner/steamlocate-rs/tree/d7a249667ceeaadffba285547106e8d809507ae9)
is the other crate for finding Steam games. Against its version 2.1.0:

- **Gives you the `.exe`.** steamlocate stops at the game's folder, without
  checking that it exists.
- **Survives broken files.** In steamlocate, one bad entry in Steam's library
  list fails the whole list; here you keep the rest and get a report.
- **Looks in more places.** On Windows, steamlocate reads only the
  machine-wide registry entry; this also checks the per-user one, a running
  Steam, and Program Files.
- **Reads `.exe` versions and SteamCMD server folders.** steamlocate does
  neither.
- **Choose steamlocate** if you need non-Steam games added to Steam, or which
  compatibility tool (such as a Proton version) each game is set to use.

## Limitations

- **macOS**: tested only with Steam in its default location. Games on
  external drives have not been tested yet.
- **Case-sensitive Mac drives**: two game folders whose names differ only by
  capital letters are treated as one.
- **Old Steam folders**: a library list in Steam's pre-2021 format, or a
  capitalized `SteamApps` folder on Linux, is not read.
- **SteamCMD**: a forced install is found only when its folder is not also a
  Steam library. A game forced into SteamCMD's own folder, for example, shows
  up as a missing install.
- **Very old programs**: 16-bit `.exe` files, from the DOS and Windows 3.1
  days, report no version.

## Usage

Every Steam game has an app ID: the number in its store page address,
`store.steampowered.com/app/<app ID>/`.

### Find a game's `.exe`

```rust,no_run
use steamapps_scan::{Discovery, ExecutableQuery};

// Find Steam and everything it has installed.
let discovery = Discovery::discover();

// The game's app ID, then where its .exe sits inside the game's folder.
// When you list several, the first one that exists is used.
let editor = ExecutableQuery::any_of(123_456, [
    "Editor/EditorDiagnostic.exe",
    "Editor/Editor.exe",
])?;

// `unique` fails when the game is not installed, or is installed twice.
let report = discovery.resolve_executable(&editor);
match report.unique() {
    Ok(found) => println!("{}", found.executable()),
    Err(err) => eprintln!("{err}"),
}

# Ok::<(), steamapps_scan::QueryErr>(())
```

### List installed games, and anything that went wrong

```rust,no_run
use steamapps_scan::Discovery;

let discovery = Discovery::discover();

// Complete, Partial (something failed, but you still get the rest),
// NotFound, or Unsupported.
println!("{:?}", discovery.state());

for app in discovery.inventory().apps() {
    println!("{} {}", app.app_id(), app.name().unwrap_or("unnamed"));
}

for problem in discovery.diagnostics() {
    eprintln!("{}", problem.message);
}
```

### Scan a folder you choose

```rust,no_run
use steamapps_scan::Discovery;

// Skip the search and scan these Steam folders instead.
let discovery = Discovery::discover_from(["/mnt/games/Steam"]);
println!("{} apps", discovery.inventory().apps().len());
```

### Read an `.exe` version

```rust,no_run
# use steamapps_scan::ExecutablePath;
# fn example(exe: &ExecutablePath) -> Result<(), steamapps_scan::VersionErr> {
// `None` when the .exe carries no version at all, which is common.
if let Some(version) = exe.version()? {
    // Exactly as the developer wrote it, such as "1.7.0.54" or "1, 0, 0, 1".
    println!("{:?}", version.product_version);
}
# Ok(())
# }
```

### Command-line tool

```text
cargo install steamapps-scan --features cli
```

```text
steamapps-scan scan                 # find Steam and print everything it found
steamapps-scan apps                 # list installed games; --app-id 123 for one game
steamapps-scan resolve 123 Game.exe # print a game's .exe path; --version adds its version
steamapps-scan signal 123           # print a fingerprint of the installed build
```

The fingerprint from `signal` changes whenever Steam updates the game. Add
`--root <DIR>` (repeatable) to scan folders you choose, and `--json` for
machine-readable output.

## Technical details

You do not need this section to use the crate. It describes exactly how
discovery, reporting, and parsing behave.

### Why inventory first?

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

### Where Steam is looked for

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
without a process probe.

On macOS, automatic discovery checks `~/Library/Application Support/Steam`,
the only location the client uses for its data tree.

Explicit-root scans use only portable `std::fs` operations and support
Steam-compatible layouts on Windows, Linux, and macOS.

### Reporting

Discovery is non-lossy. The report includes:

- every accepted Steam root, library, and valid app manifest, with one app
  entry per installation directory: another manifest for the same directory,
  reached through a directory link, leaves a `duplicate` warning instead;
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

On Windows and macOS, roots, libraries, and installations are deduplicated
case-insensitively. On a case-sensitive APFS volume, two that differ only by
case merge; the attempts ledger and the duplicate warnings still record both.

### steamcmd `force_install_dir` folders

An explicit root can be a folder filled by steamcmd's `force_install_dir`,
which commits every app straight into the folder. A root that no
`libraryfolders.vdf` registers and that holds none of the files Steam keeps in
its own libraries (`steamapps/libraryfolders.vdf`, `libraryfolder.vdf`,
`steamapps/common`) reports the `force_install_dir` layout. Each of its apps is
installed in the folder itself once the folder holds anything besides
`steamapps` and what file managers leave in the folders they show
(dot-prefixed entries such as `.DS_Store`, `desktop.ini`, and `Thumbs.db`); a
manifest alone is reported as a missing installation. In JSON, each library is
an object holding its `path` and `layout` (`common` or `force_install_dir`).

An install forced into a folder that does hold Steam's library files, such as
steamcmd's own root, is not supported: its manifests are reported as missing
`steamapps/common` installations.

### App metadata

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

### Executable versions

The version is read from the Windows `VS_VERSIONINFO` resource: the same data
the Explorer property sheet shows. It is a plain file read; nothing is
executed.

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

### CLI exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | Success |
| 1 | Nothing found: no Steam, no apps, or no match |
| 2 | Usage error, including Steam app ID zero |
| 3 | Ambiguous: more than one match where exactly one is needed |
| 4 | Partial: a result was produced, but some evidence could not be read |
| 5 | Automatic discovery is unsupported on this platform |
| 6 | No result produced: a content signal could not be derived, or the output could not be serialized or written |

An unsupported platform takes precedence over every other code, and partial
evidence then takes precedence over a subcommand's own result. A failed
version read counts as partial evidence.

## License

Licensed under either the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT license](LICENSE-MIT), at your option.
