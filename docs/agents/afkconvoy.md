# Landing config

What `guild land forge` reads of this repo, as afkconvoy reads it: the gate every pull request passes before it lands on `main`.

```yaml
base_branch: main
gate_command: guild exclusive -- sh gate.sh
gate_seconds: 1800
```
