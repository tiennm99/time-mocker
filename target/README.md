# time-mocker-target

Java test target for [time-mocker](https://github.com/tiennm99/time-mocker) — validates injected fake time behavior.

## Quick start

```bash
./gradlew run
```

Run alongside `time-mocker` to verify that system time injection works correctly.

## Expected behavior

**Without time-mocker** — prints actual current time, ticking every second:

```
2025-01-15T08:30:01
2025-01-15T08:30:02
2025-01-15T08:30:03
```

**With time-mocker injecting a fake date** (e.g. 2000-06-15T12:00:00) — the same binary prints the mocked time instead:

```
2000-06-15T12:00:01
2000-06-15T12:00:02
2000-06-15T12:00:03
```

If the output reflects the injected date rather than real system time, the injection is working correctly.

## License

Apache-2.0 — see [LICENSE](LICENSE).
