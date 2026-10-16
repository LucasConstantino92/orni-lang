import http from "node:http";

const port = Number(process.env.PORT ?? 3001);

http.createServer((_request, response) => {
  response.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
  response.end("OK");
}).listen(port, "127.0.0.1", () => {
  console.log(`Node health server listening on http://127.0.0.1:${port}/health`);
});
