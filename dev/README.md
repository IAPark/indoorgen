# dev/: one-off probes and comparison tools

Not built by default. Build one with `cargo build --profile fast -p indoorgen-dev --bin <name>`.
These were used while porting Infinigen assets and mostly read reference dumps from a local Python
reference checkout (see `INDOORGEN_REF` in the top-level README); several expect hard-coded local
data files and are kept for reference only. `attic/` holds two tools (`toicmp`, `opdump`) that no
longer compile against the current API.
