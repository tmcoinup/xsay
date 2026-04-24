# Windows packaging

`installer.nsi` is the NSIS script that produces
`xsay-<version>-setup.exe` — a per-machine installer for Windows.

## Invoked from CI

`.github/workflows/release.yml` copies `target/release/xsay.exe` into
this directory, then runs:

```
makensis /DAPP_VERSION=0.1.0 windows/installer.nsi
```

`makensis` is pre-installed on the `windows-latest` GitHub runner.

## Building locally

On a Windows machine:

```powershell
# 1. install NSIS
winget install -e --id NSIS.NSIS

# 2. build the xsay binary
cargo build --release

# 3. stage + run
copy target\release\xsay.exe windows\xsay.exe
cd windows
makensis /DAPP_VERSION=0.1.0 installer.nsi
```

Output: `windows/xsay-0.1.0-setup.exe`.

## What the installer does

- Copies `xsay.exe`, `LICENSE`, `README.md` into
  `C:\Program Files\xsay\`
- Adds a Start Menu group with launcher + uninstaller shortcuts
- Registers an entry in **Control Panel → Programs and Features** so
  users can uninstall through the standard Windows UI
- Offers a "Launch xsay now" checkbox on the final installer page

The installer itself doesn't touch audio/keyboard permissions — first
run of xsay may trigger Windows' "allow microphone access" prompt.
