const root = require("path").join(__dirname, "..", "..");

module.exports = require("node-gyp-build")(root);

try {
  module.exports.nodeTypeInfo = require("../../src/node-types.json");
} catch (err) {
  if (err && err.code !== "MODULE_NOT_FOUND") throw err;
}
