const fs = require("node:fs");
const path = require("node:path");
const TOML = require("@iarna/toml");

/** Finds the nearest manifest and resolves its project paths. */
function projectFor(filePath) {
  let current = path.dirname(filePath);
  while (true) {
    const manifestPath = path.join(current, "orni.toml");
    if (fs.existsSync(manifestPath)) {
      if (fs.statSync(manifestPath).size > 65536) throw new Error("Project manifest exceeds 65536 bytes.");
      const manifest = TOML.parse(fs.readFileSync(manifestPath, "utf8"));
      if (!manifest.project || typeof manifest.project !== "object") throw new Error("Manifest requires a [project] table.");
      for (const section of Object.keys(manifest)) {
        if (!["project", "runtime"].includes(section)) throw new Error(`Unknown manifest section '${section}'.`);
      }
      for (const setting of Object.keys(manifest.project)) {
        if (!["name", "source", "entry"].includes(setting)) throw new Error(`Unknown project setting '${setting}'.`);
      }
      const resolve = (name, fallback) => {
        const value = manifest.project[name] ?? fallback;
        if (typeof value !== "string" || path.isAbsolute(value)) throw new Error(`project.${name} must be a relative path.`);
        const resolved = path.resolve(current, value);
        const relative = path.relative(current, resolved);
        if (value.split(/[\\/]/).includes("..") || relative === ".." || relative.startsWith(`..${path.sep}`)) throw new Error(`project.${name} must stay inside the project directory.`);
        if (fs.existsSync(resolved)) {
          const canonicalRelative = path.relative(fs.realpathSync(current), fs.realpathSync(resolved));
          if (canonicalRelative === ".." || canonicalRelative.startsWith(`..${path.sep}`) || path.isAbsolute(canonicalRelative)) {
            throw new Error(`project.${name} resolves outside the project directory.`);
          }
        }
        return resolved;
      };
      const source = resolve("source", ".");
      const entry = resolve("entry", "main.orni");
      if (!fs.existsSync(source) || !fs.statSync(source).isDirectory()) throw new Error("project.source must name an existing directory.");
      if (path.extname(entry) !== ".orni") throw new Error("project.entry must name an .orni file.");
      return { root: current, source, entry, manifestPath };
    }
    const parent = path.dirname(current);
    if (parent === current) return undefined;
    current = parent;
  }
}

/** Resolves imports using the same relative/root ambiguity rule as the compiler. */
function resolveModule(filePath, segments, sourceRoot) {
  const project = projectFor(filePath);
  const local = path.resolve(path.dirname(filePath), ...segments) + ".orni";
  const rooted = path.resolve(sourceRoot ?? project?.source ?? path.dirname(filePath), ...segments) + ".orni";
  if (local !== rooted && fs.existsSync(local) && fs.existsSync(rooted)) {
    throw new Error(`Module import is ambiguous between '${local}' and '${rooted}'.`);
  }
  return fs.existsSync(local) || !fs.existsSync(rooted) ? local : rooted;
}

/** Returns whether a file belongs to the project's executable test suite. */
function isTestFile(filePath, project) {
  if (!project) return false;
  const relative = path.relative(project.root, filePath);
  return relative !== "" && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative) && relative.split(path.sep)[0] === "tests";
}

module.exports = { projectFor, resolveModule, isTestFile };
