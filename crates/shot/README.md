# shot

Screenshots a web page or local HTML file in headless Chrome with the viewport emulated at a given width and `prefers-color-scheme` emulated dark or light, saves a full-page PNG, and prints the page's document `scrollWidth` beside the width, so horizontal overflow shows.

```
shot <url|file> --width <px> [--dark|--light] [--out <png>]
```

It prints four lines: `png: <path>`, `width: <px>`, `scrollWidth: <n>`, `overflow: yes|no`. Run `shot --help` for every argument, where it looks for a browser and how it fails. Released by forge as a tool of rust-web-coder and ui-tester: `guild tool shot …`. Needs Google Chrome or Chromium on the machine, nothing else. Tests, against the machine's real Chrome on the HTML files in `fixtures/`: `cargo test -p shot` at forge's root.

## How it works

shot starts Chrome as `--headless=new --remote-debugging-pipe` with a fresh profile and speaks the Chrome DevTools Protocol over the pipe Chrome opens on fds 3 and 4 (JSON messages, each ended by a NUL byte): no websocket, no port. On a new page it sends `Emulation.setDeviceMetricsOverride` (the width, scale 1) and `Emulation.setEmulatedMedia` (`prefers-color-scheme`), navigates, waits for the navigation's `load` lifecycle event, reads `document.documentElement.scrollWidth` and the page's height with `Runtime.evaluate`, and captures it with `Page.captureScreenshot` clipped to the asked width and the full height (`captureBeyondViewport`). No `--window-size` or `--force-dark-mode`: the first is clamped to ~500px by a headless window, the second does not set `prefers-color-scheme`.

## Existing tools looked at

Before building (guild ADR 0013), these were weighed against what Guild's machines have: Google Chrome and Rust; no node, no playwright, no Python websocket library.

| Tool | Link | Adopted? | Why |
| --- | --- | --- | --- |
| Chrome's own `--headless --screenshot` | https://developer.chrome.com/docs/chromium/headless | No | Already on every machine, but it sizes the page by `--window-size`, which the evidence showed clamped near 500px, cannot emulate `prefers-color-scheme` (`--force-dark-mode` does not), takes only the viewport rather than the full page, and cannot report `scrollWidth`. |
| headless_chrome (Rust crate, 1.0.22, MIT) | https://github.com/rust-headless-chrome/rust-headless-chrome | No | It would work (its `call_method` reaches any CDP method), but it talks to Chrome over a websocket and brings anyhow, tungstenite, url, regex, rand, tempfile, derive_builder, thiserror, which and log, plus a CDP code generator at build time: dozens of crates for six protocol calls. The pipe transport shot uses needs none of them. |
| chromiumoxide (Rust crate, 0.9.1, MIT OR Apache-2.0) | https://github.com/mattsse/chromiumoxide | No | Async: it needs tokio or async-std, a websocket stack and its generated CDP types, heavier still than headless_chrome, for a tool that makes six calls in order. |
| shot-scraper (Python CLI) | https://github.com/simonw/shot-scraper | No | Does nearly this (`--width`, full page), but runs on Python with Playwright and Playwright's own browser download; the machines have neither, and installing them is what the tool must not need. |
| Playwright's `screenshot` CLI | https://playwright.dev/docs/cli#take-screenshot | No | Has `--viewport-size`, `--color-scheme` and `--full-page`, but needs node and Playwright with its own downloaded browsers, which the machines do not have, and prints no `scrollWidth`. |
| puppeteer | https://github.com/puppeteer/puppeteer | No | node, which the machines do not have. |

None was adopted: shot is built on the machine's Chrome and the DevTools Protocol's pipe transport, with `serde_json` (already forge's) as its only dependency. Its tests decode the PNG with the `png` crate (https://github.com/image-rs/image-png, 0.18.1, MIT OR Apache-2.0) as a dev-dependency only, to check a background pixel and the PNG's size; it is not in the tool.
