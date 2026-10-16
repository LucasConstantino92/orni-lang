# Local HTTP benchmark

This folder provides a small, repeatable baseline for comparing the current
native Orni development server with a minimal Node.js HTTP server.

Run the isolated Orni benchmark server in one terminal:

```powershell
cargo run -- serve examples/benchmark/orni-health.orni
```

Run the Node baseline in a second terminal:

```powershell
node examples/benchmark/node-health-server.mjs
```

Then run the same workload against each process in separate commands:

```powershell
node tools/http-bench.mjs http://127.0.0.1:3002/health --requests 10000 --concurrency 50 --warmup 500
node tools/http-bench.mjs http://127.0.0.1:3001/health --requests 10000 --concurrency 50 --warmup 500
```

Keep the route response, request count, concurrency, machine, and power mode
the same for each run. Compare throughput together with p95 and p99 latency;
a high request rate is not useful if non-2xx responses or network errors grow.
Keep the Node server terminal open while its benchmark runs. A result marked
`INVALID` means that at least one request did not reach a server and must not
be compared with another run.

The Orni server intentionally suppresses per-request terminal logs during this
measurement. Set `ORNI_HTTP_LOG=1` only when inspecting individual requests,
not when collecting benchmark numbers.
