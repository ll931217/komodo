import { exec } from "child_process";
import { readFileSync, writeFileSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

console.log("generating typescript types...");

const gen_command =
  "RUST_BACKTRACE=1 typeshare . --lang=typescript --output-file=./client/core/ts/src/types.ts";

exec(gen_command, (error, _stdout, _stderr) => {
  if (error) {
    console.error(error);
    // Exit non-zero, or `make gen-client` carries on to `yarn build`,
    // which compiles the PREVIOUS types.ts, copies it into
    // ui/public/client, and reports success. A stale generated client
    // still typechecks, so nothing downstream notices that it no
    // longer matches the Rust API.
    process.exit(1);
  }
  console.log("generated types using typeshare");
  fix_types();
  console.log("finished.");
});

function fix_types() {
  const types_path = __dirname + "/src/types.ts";
  const contents = readFileSync(types_path);
  const fixed = contents
    .toString()
    // Replace Variants
    .replaceAll("ResourceTargetVariant", 'ResourceTarget["type"]')
    .replaceAll("AlerterEndpointVariant", 'AlerterEndpoint["type"]')
    .replaceAll("AlertDataVariant", 'AlertData["type"]')
    .replaceAll("ServerTemplateConfigVariant", 'ServerTemplateConfig["type"]')
    .replaceAll("UserConfigVariant", 'UserConfig["type"]')
    // Add '| string' to env vars
    .replaceAll("EnvironmentVar[]", "EnvironmentVar[] | string")
    .replaceAll("IndexSet", "Array")
    .replaceAll(
      ": PermissionLevelAndSpecifics",
      ": PermissionLevelAndSpecifics | PermissionLevel"
    )
    .replaceAll(
      ", PermissionLevelAndSpecifics",
      ", PermissionLevelAndSpecifics | PermissionLevel"
    )
    .replaceAll("IndexMap", "Record");
  writeFileSync(types_path, fixed);
}
