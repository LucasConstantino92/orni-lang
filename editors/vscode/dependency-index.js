const { createHash } = require("node:crypto");

/** Tracks import ownership and content identities without retaining source text. */
class DependencyIndex {
  constructor() {
    this.nodes = new Map();
    this.importers = new Map();
  }

  /** Replaces a file's direct imports and returns whether its content changed. */
  set(file, source, dependencies = []) {
    const previous = this.nodes.get(file);
    const hash = createHash("sha256").update(source).digest("hex");
    const imports = new Set(dependencies);
    for (const dependency of previous?.imports || []) {
      const owners = this.importers.get(dependency);
      owners?.delete(file);
      if (!owners?.size) this.importers.delete(dependency);
    }
    for (const dependency of imports) {
      if (!this.importers.has(dependency)) this.importers.set(dependency, new Set());
      this.importers.get(dependency).add(file);
    }
    this.nodes.set(file, { hash, imports });
    return previous?.hash !== hash;
  }

  /** Removes a file while retaining inbound edges needed to invalidate importers. */
  delete(file) {
    for (const dependency of this.nodes.get(file)?.imports || []) {
      const owners = this.importers.get(dependency);
      owners?.delete(file);
      if (!owners?.size) this.importers.delete(dependency);
    }
    this.nodes.delete(file);
  }

  /** Includes every transitive importer, even in an invalid cyclic graph. */
  affected(files) {
    const affected = new Set(files);
    for (const file of affected) {
      for (const owner of this.importers.get(file) || []) affected.add(owner);
    }
    return affected;
  }

  /** Returns whether another indexed file imports this file as a module. */
  hasImporters(file) {
    return Boolean(this.importers.get(file)?.size);
  }

  /** Returns a cycle-safe graph identity, including absent imported files. */
  snapshot(entry) {
    const files = new Set([entry]);
    for (const file of files) {
      for (const dependency of this.nodes.get(file)?.imports || []) files.add(dependency);
    }
    const identity = [...files].sort().map((file) => [file, this.nodes.get(file)?.hash ?? null]);
    return { files, fingerprint: createHash("sha256").update(JSON.stringify(identity)).digest("hex") };
  }

  clear() {
    this.nodes.clear();
    this.importers.clear();
  }
}

module.exports = { DependencyIndex };
