# Snap packaging assets

This directory is consumed by the `desktop-file` part in
`../snapcraft.yaml`. Contents end up under `$SNAP/share/` inside the
built snap.

## Required files

- `xsay.desktop` — application launcher shown in Ubuntu's Activities.
- `xsay.png` — **256×256 application icon. Not checked in; drop it in
  here before running `snapcraft`**. A generic mic/waveform icon is
  fine. Transparent PNG preferred.

If you don't have an icon handy:

```bash
# Quick placeholder via ImageMagick
convert -size 256x256 xc:transparent \
    -fill '#3DA5FF' -draw 'circle 128,128 128,48' \
    -fill 'white'   -draw 'rectangle 118,80 138,150' \
    -stroke white   -strokewidth 4 -fill none \
    -draw 'arc 78,80 178,190 0,180' \
    xsay.png
```

Or: use any 256×256 PNG you like, save it as `xsay.png` here.
