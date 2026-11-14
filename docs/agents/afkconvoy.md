# Landing config

What `guild land forge` reads of this repo, as afkconvoy reads it: the gate every pull request passes before it lands on `main`.

```yaml
base_branch: main
gate_command: guild exclusive -- sh gate.sh
gate_seconds: 1800
```

Before review, run `guild forge check .`: it checks each tool's `tool.toml` as `guild forge publish` reads it, and the gate runs it too, so a tool publish would refuse never lands.
