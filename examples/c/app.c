/* A complete Nile application in C: no window code, no event loop.
 *
 *   cargo build -p nile-app
 *   cc examples/c/app.c -Icrates/nile-app/include -Icrates/nile-capi/include -Ltarget/debug -lnile_app -o app
 *   ./app            (macOS: DYLD_LIBRARY_PATH=target/debug ./app)
 */
#include "nile_app.h"

/* Scripts call this as host.greet("name"). Arguments arrive as a JSON array ("[\"Nile\"]") and the
 * answer is a JSON value: here a JSON string. */
static const char *greet(const char *args_json, void *user) {
  (void)args_json;
  (void)user;
  return "\"Hello from C\"";
}

int main(void) {
  nile_app_register_function("greet", greet, NULL);
  return nile_run_html(
      "<html window-width=\"420\" window-height=\"160\"><body style=\"font:16px sans-serif;margin:24px\">"
      "<h3>Standalone from C</h3><p id=r></p>"
      "<script>document.getElementById('r').textContent = host.greet('Nile');</script>"
      "</body></html>",
      NULL, 0, 0);
}
