// A Sciter-style bootstrap script: `nile examples/bootstrap/run.js` (or pack the folder and run the result).
// It runs before any window of its own, then opens the real ones.
import * as env from "@env";
import * as sys from "@sys";

console.log("running on", env.PLATFORM, "from", __DIR__);
const hasStyle = sys.fs.statSync(__DIR__ + "hello.htm") !== null;
console.log("hello.htm found:", hasStyle);

application.start("gpu"); // the graphics backend is the shell's choice here; accepted and ignored

const mainWindow = new Window({
  url: __DIR__ + "hello.htm",
  parameters: { greeting: "Hello from run.js" },
  width: 460,
  height: 240,
});
mainWindow.on("close", () => application.quit(0));

const quitValue = application.run();
console.log("run() returned", quitValue);
