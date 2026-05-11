# time-changer

GUI tool written in Go (Fyne) to change the Windows system time from the desktop.

> **Note:** Must be run as Administrator — Windows requires elevated privileges to set the system clock.

## Quick start

```bash
go build -o time-changer.exe .
# Right-click → Run as Administrator:
time-changer.exe
```

## Usage

The app presents a calendar and time picker. Select the target date and time, then confirm to apply it as the new system time.

Supported date selection: year/month/day via calendar widget; hours, minutes, seconds via dropdowns.

## Related

- [time-mocker](https://github.com/tiennm99/time-mocker) — injects fake time into a running process without changing the system clock (different approach to the same problem of time-dependent testing).

## License

Apache-2.0 — see [LICENSE](LICENSE).
