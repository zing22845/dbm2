> **Unsigned binaries** — the macOS and Windows builds in this release are not
> code-signed, so the operating system warns on first launch:
>
> - **macOS**: right-click the `dbm` binary and choose *Open*, or run
>   `xattr -d com.apple.quarantine ./dbm`
> - **Windows**: on the SmartScreen prompt choose *More info* → *Run anyway*
>
> Linux builds are static (musl) and unaffected. Verify downloads against the
> `.sha256` files.

Full change history: [CHANGELOG.md](https://github.com/zing22845/dbm2/blob/main/CHANGELOG.md)
