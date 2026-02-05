const { readdirSync } = require("fs");
const { execSync } = require("child_process");

const files = readdirSync(".").filter(
  (n) => n.startsWith("taski-") && n.endsWith(".vsix")
);

if (!files.length) {
  console.error("No .vsix file found");
  process.exit(1);
}

files.forEach((name) => {
  console.log("Installing " + name);
  execSync("code --install-extension " + name, { stdio: "inherit" });
});
