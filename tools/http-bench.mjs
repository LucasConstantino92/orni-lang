#!/usr/bin/env node

import http from "node:http";
import https from "node:https";
import { performance } from "node:perf_hooks";

const usage = `Usage: node tools/http-bench.mjs <url> [options]

Options:
  --requests <count>      Measured requests to make. Default: 10000
  --concurrency <count>   Simultaneous requests. Default: 50
  --warmup <count>        Requests before measurement. Default: 500
  --timeout <milliseconds> Per-request timeout. Default: 5000
`;

function readPositiveInteger(value, name) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 1) {
    throw new Error(`${name} must be a positive integer.`);
  }
  return number;
}

function readArguments(values) {
  const [url, ...options] = values;
  if (!url || url === "--help" || url === "-h") {
    return { help: true };
  }

  const settings = { url, requests: 10000, concurrency: 50, warmup: 500, timeout: 5000 };
  for (let index = 0; index < options.length; index += 2) {
    const option = options[index];
    const value = options[index + 1];
    if (!value) throw new Error(`Missing value for ${option}.`);
    if (option === "--requests") settings.requests = readPositiveInteger(value, option);
    else if (option === "--concurrency") settings.concurrency = readPositiveInteger(value, option);
    else if (option === "--warmup") settings.warmup = readPositiveInteger(value, option);
    else if (option === "--timeout") settings.timeout = readPositiveInteger(value, option);
    else throw new Error(`Unknown option: ${option}`);
  }
  return settings;
}

function percentile(values, ratio) {
  if (values.length === 0) return 0;
  const position = Math.min(values.length - 1, Math.ceil(values.length * ratio) - 1);
  return values[position];
}

function requestOnce(url, agent, timeout) {
  return new Promise((resolve) => {
    const startedAt = performance.now();
    const transport = url.protocol === "https:" ? https : http;
    const request = transport.request(url, { method: "GET", agent }, (response) => {
      response.resume();
      response.on("end", () => resolve({ status: response.statusCode ?? 0, duration: performance.now() - startedAt }));
    });
    request.setTimeout(timeout, () => request.destroy(new Error("request timed out")));
    request.on("error", (error) => resolve({ status: 0, duration: performance.now() - startedAt, error }));
    request.end();
  });
}

async function runBatch({ url, requests, concurrency, timeout }, collect) {
  const agent = url.protocol === "https:"
    ? new https.Agent({ keepAlive: true, maxSockets: concurrency })
    : new http.Agent({ keepAlive: true, maxSockets: concurrency });
  const results = {
    completed: 0,
    successful: 0,
    nonSuccess: 0,
    networkErrors: 0,
    durations: [],
    statusCounts: new Map(),
    firstNetworkError: undefined,
  };
  let nextRequest = 0;

  async function worker() {
    while (nextRequest < requests) {
      nextRequest += 1;
      const result = await requestOnce(url, agent, timeout);
      results.completed += 1;
      if (result.error) {
        results.networkErrors += 1;
        results.firstNetworkError ??= result.error.message;
      } else if (result.status >= 200 && result.status < 300) {
        results.successful += 1;
        results.statusCounts.set(result.status, (results.statusCounts.get(result.status) ?? 0) + 1);
      } else {
        results.nonSuccess += 1;
        results.statusCounts.set(result.status, (results.statusCounts.get(result.status) ?? 0) + 1);
      }
      if (collect) results.durations.push(result.duration);
    }
  }

  await Promise.all(Array.from({ length: Math.min(concurrency, requests) }, worker));
  agent.destroy();
  return results;
}

function formatMilliseconds(value) {
  return `${value.toFixed(2)} ms`;
}

async function main() {
  const settings = readArguments(process.argv.slice(2));
  if (settings.help) {
    console.log(usage);
    return;
  }

  const url = new URL(settings.url);
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("The URL must use http or https.");
  }
  settings.url = url;

  if (settings.warmup > 0) {
    await runBatch({ ...settings, requests: settings.warmup }, false);
  }

  const startedAt = performance.now();
  const result = await runBatch(settings, true);
  const elapsed = performance.now() - startedAt;
  result.durations.sort((left, right) => left - right);

  console.log(`\nHTTP benchmark: ${url.href}`);
  console.log(`Requests: ${settings.requests} | Concurrency: ${settings.concurrency} | Warmup: ${settings.warmup}`);
  console.log(`Completed: ${result.completed} | 2xx: ${result.successful} | Non-2xx: ${result.nonSuccess} | Network errors: ${result.networkErrors}`);
  if (result.statusCounts.size > 0) {
    const statuses = [...result.statusCounts.entries()].map(([status, count]) => `${status}: ${count}`).join(", ");
    console.log(`HTTP statuses: ${statuses}`);
  }
  console.log(`Attempt rate: ${(result.completed / (elapsed / 1000)).toFixed(2)} requests/second`);
  console.log(`Latency: p50 ${formatMilliseconds(percentile(result.durations, 0.50))} | p95 ${formatMilliseconds(percentile(result.durations, 0.95))} | p99 ${formatMilliseconds(percentile(result.durations, 0.99))}`);
  if (result.networkErrors > 0) {
    console.log(`Result: INVALID — ${result.networkErrors} request(s) did not reach the server. First error: ${result.firstNetworkError}`);
  } else if (result.nonSuccess > 0) {
    console.log("Result: DEGRADED — the server answered every request, but some responses were not successful.");
  } else {
    console.log("Result: VALID — every measured request received a successful response.");
  }
}

main().catch((error) => {
  console.error(`Benchmark error: ${error.message}`);
  console.error(usage);
  process.exitCode = 1;
});
