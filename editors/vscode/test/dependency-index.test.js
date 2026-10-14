const assert = require("node:assert/strict");
const test = require("node:test");
const { DependencyIndex } = require("../dependency-index");

test("indexes a thousand files but invalidates only transitive importers", () => {
  const index = new DependencyIndex();
  for (let file = 0; file < 1000; file++) index.set(`module${file}`, `func get${file}() -> int { return 1; }`);
  index.set("main", "import module0;", ["module0"]);
  index.set("other", "import module999;", ["module999"]);
  index.set("module0", "import module1;", ["module1"]);
  assert.deepEqual([...index.affected(["module1"])].sort(), ["main", "module0", "module1"]);
  assert.equal(index.snapshot("main").files.size, 3);
});

test("unrelated edits and repeated identical buffers preserve the graph cache identity", () => {
  const index = new DependencyIndex();
  index.set("main", "main", ["library"]);
  index.set("library", "original");
  const before = index.snapshot("main").fingerprint;
  index.set("unrelated", "changed");
  assert.equal(index.set("library", "original"), false);
  assert.equal(index.snapshot("main").fingerprint, before);
  index.set("library", "unsaved edit");
  assert.notEqual(index.snapshot("main").fingerprint, before);
});

test("handles cyclic imports and cleans replaced outbound edges", () => {
  const index = new DependencyIndex();
  index.set("a", "a", ["b"]);
  index.set("b", "b", ["a"]);
  assert.equal(index.snapshot("a").files.size, 2);
  assert.deepEqual([...index.affected(["a"])].sort(), ["a", "b"]);
  index.set("a", "fixed");
  assert.deepEqual([...index.affected(["b"])], ["b"]);
});

test("a missing or removed dependency invalidates its importers", () => {
  const index = new DependencyIndex();
  index.set("main", "main", ["library"]);
  const missing = index.snapshot("main").fingerprint;
  index.set("library", "created");
  assert.notEqual(index.snapshot("main").fingerprint, missing);
  index.delete("library");
  assert.equal(index.snapshot("main").fingerprint, missing);
  assert.deepEqual([...index.affected(["library"])], ["library", "main"]);
  index.clear();
  assert.equal(index.importers.size, 0);
});
