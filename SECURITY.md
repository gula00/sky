# Security

`sky` can inspect windows and send input to the desktop. Treat it
as privileged local automation software.

## Reporting

Please report security issues privately to the project maintainers before
opening a public issue.

## Safety Model

The helper:

- requests app approval before side-effecting window/app actions in the
  observed protocol flow
- filters common credential, UAC, logon, and Windows Security surfaces
- supports turn interruption markers
- enforces request-budget timeouts

The filter bypass environment variable is only for tests:

```powershell
$env:ComputerUseAllowForbiddenTargets = "true"
```

Do not enable this in normal use.

