# Engineering rules

- Keep native domain logic in `crates/core`, serializable contracts in `crates/contracts`, desktop integration in `src-tauri`, and Leptos UI in `crates/ui`.
- Validate all IPC inputs. Custom Tauri commands must be listed in the app manifest and capabilities.
- UI code must not access the filesystem directly. Pass opaque entry IDs, not reconstructed user paths, to native operations.
- Never infer content equality from matching timestamps. Read errors, unverified contents, symbolic links and type conflicts remain distinct.
- Synchronization requires a preview, source/target revalidation, verified backups and an operation journal. Do not silently delete destination-only files.
- Keep tests focused on observable behavior and failure modes. Run relevant Rust tests, formatting and Clippy; build WASM and desktop when those layers change.
- Use two compilation jobs on memory-constrained machines. Do not start services or watchers without authorization.
- Do not commit build products, secrets, local filesystem paths, real comparison data or credentials. Release assets are uploaded separately.
- Keep README and the feature matrix honest: planned capabilities are not supported capabilities.
- MIT applies to original code; preserve third-party notices and licenses.
