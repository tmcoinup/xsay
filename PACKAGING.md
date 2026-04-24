# Packaging & distribution

This doc covers how xsay ships as a multi-variant release on GitHub and
how to publish to Ubuntu's Snap Store (which is what Ubuntu Software /
GNOME Software surfaces by default).

---

## 1. GitHub Releases (multi-variant binaries)

Release builds are automated in [`.github/workflows/release.yml`](.github/workflows/release.yml).
A push of any `v*` tag triggers a matrix job that builds:

| variant                       | runner         | features           |
|-------------------------------|----------------|--------------------|
| `xsay-linux-x64-cpu`          | ubuntu-22.04   | default (SenseVoice) |
| `xsay-linux-x64-vulkan`       | ubuntu-22.04   | `vulkan`           |
| `xsay-macos-arm64-metal`      | macos-14 (ARM) | `metal`            |

To cut a release:

```bash
# Bump the version in Cargo.toml, commit.
git tag v0.1.0
git push origin v0.1.0
```

Artifacts upload to the matching GitHub Release page automatically.

### Building variants locally

```bash
./build.sh cpu                  # only the default CPU variant
./build.sh cpu vulkan           # CPU + Vulkan
./build.sh all                  # everything the host toolchain supports
```

Output lands in `dist/`.

---

## 2. Debian package (.deb)

Out of the box via `cargo-deb`:

```bash
cargo install cargo-deb
cargo deb
sudo dpkg -i target/debian/xsay_0.1.0_amd64.deb
```

The generated .deb already declares runtime deps (libx11-6, libxtst6,
libasound2, libgtk-3-0, libayatana-appindicator3-1, libxdo3) from
`[package.metadata.deb]` in `Cargo.toml`.

For a Launchpad PPA workflow:

1. Sign the .deb (`dpkg-sig --sign builder foo.deb`) with a GPG key
   registered to Launchpad.
2. Upload via `dput ppa:<you>/<ppa> <file>.source.changes` after
   building a source package with `debuild -S`.
3. Users install with `sudo add-apt-repository ppa:<you>/<ppa>` +
   `sudo apt install xsay`.

A PPA doesn't show up in Ubuntu Software by default, but it's the
easiest way to get auto-updates on Debian/Ubuntu.

---

## 3. Snap Store (what Ubuntu Software surfaces)

### Prerequisites

```bash
sudo snap install snapcraft --classic
sudo snap install lxd      # multipass also works, LXD is lighter
sudo lxd init --auto
sudo usermod -aG lxd $USER # log out/in after this
```

### Build

```bash
cd /path/to/xsay
snapcraft                  # uses snap/snapcraft.yaml in the repo
```

First build takes ~10 minutes (compiles whisper.cpp + pulls the Rust
toolchain into an LXD container). Subsequent builds are much faster.

Output: `xsay_0.1.0_amd64.snap` in the repo root.

### Local test

```bash
sudo snap install --dangerous ./xsay_*_amd64.snap
xsay &                     # runs inside the snap sandbox
```

The snap requests confinement interfaces (`audio-record`, `desktop`,
`x11`, `wayland`, `home`, `network-observe`). Users are prompted to
approve these on first launch.

### Publish

One-time:

```bash
snapcraft login            # opens a browser to snapcraft.io
snapcraft register xsay    # reserve the name
```

Each release:

```bash
snapcraft upload --release=edge ./xsay_*_amd64.snap
# After testing in the edge channel, promote to stable:
snapcraft status xsay
snapcraft release xsay <revision> stable
```

Canonical's automated store review runs on upload. If xsay needs
classic confinement (it probably does, to access /dev/uinput and
system hotkeys), the review will flag it and a human will check. Be
ready to explain why `--classic` is required.

### What happens next

Once published to `stable`, the snap appears in:

- **Ubuntu Software** (snap-store package, bundled with Ubuntu 22.04+)
- **GNOME Software** on distros with the snap plugin installed
- Direct: `sudo snap install xsay`

Updates propagate automatically — Snap Store pushes new revisions to
all installs within ~24 hours of upload.

---

## 4. Flathub (alternative cross-distro store)

Flatpak is the Red Hat / Fedora / KDE community's preferred path, and
Ubuntu users often enable it for apps that aren't in Snap. The
submission process is a PR against
[flathub/flathub](https://github.com/flathub/flathub) with a manifest
file. Because xsay needs uinput + global hotkey access, it's not a
great Flatpak fit — the sandbox is stricter than Snap classic
confinement. Recommendation: ship Snap first, Flatpak only if there's
demand.

---

## Decision guide

| Goal                                 | Use this                    |
|--------------------------------------|-----------------------------|
| "Just put it on my Ubuntu"           | Snap (shows up in Ubuntu Software) |
| Quick .deb for a friend              | `cargo deb` + send the file |
| Auto-updates, no store review        | PPA                         |
| Fedora / Arch / any distro           | Static binary from Releases |
| Apple Silicon Mac                    | `xsay-macos-arm64-metal` from Releases |
| Windows                              | Not packaged yet — build from source |
