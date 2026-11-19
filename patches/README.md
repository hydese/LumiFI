# Patched dependencies

- **xattr 0.2.3** (via gpui 0.2.2 → gpui_http_client → zed-async-tar). Recent `libc` releases
  removed `ENOATTR` on Linux and Android, where it has always been the same value as `ENODATA`.
  The only change is in `src/sys/mod.rs`. macOS and Windows build the same as upstream.
  Remove this patch when gpui moves to a newer xattr.
