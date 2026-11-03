/*
 * A smoke test of the whole C API surface: it creates a view and calls every group of functions once, so a change to the
 * header that breaks compiling or linking is noticed (CI builds and runs this). It exits 0 when everything answers sensibly.
 *
 *   cargo build -p nile-capi
 *   cc examples/c/abi_check.c -Icrates/nile-capi/include -Ltarget/debug -lnile -Wl,-rpath,$PWD/target/debug -o abi_check
 *   ./abi_check
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "nile.h"

static int failures = 0;
#define CHECK(cond)                                                       \
  do {                                                                    \
    if (!(cond)) {                                                        \
      fprintf(stderr, "FAILED line %d: %s\n", __LINE__, #cond);           \
      failures++;                                                         \
    }                                                                     \
  } while (0)

static const char *clip_get(void *user) { (void)user; return "clipboard text"; }
static void clip_set(const char *text, void *user) { (void)text; (void)user; }

static const char *PAGE =
    "<html window-title=\"ABI\" window-width=\"320\" window-frame=\"solid\"><body>"
    "<menu id=\"t\"><li id=\"a\">One</li></menu>"
    "<button role=\"window-maximize\">m</button>"
    "<script>"
    "Window.this.tray = { icon: 'none.png', tooltip: 'Tip', menu: document.getElementById('t') };"
    "application.badge = 2; application.progress = 0.25;"
    "window.key = application.registerHotkey('Ctrl+Shift+K', () => {});"
    "</script></body></html>";

int main(void) {
  /* Version: the compiled header and the loaded library must match. */
  CHECK(nile_abi_version() == NILE_ABI_VERSION);
  CHECK(nile_version() != NULL);

  NileView *view = nile_view_create_html(PAGE, NULL, 320.0f, 200.0f);
  CHECK(view != NULL);
  if (!view) return 1;

  /* Structs the library fills carry their size; one that does not is refused, not overrun. */
  NileWindowConfig config = {.struct_size = sizeof(NileWindowConfig)};
  CHECK(nile_view_window_config(view, 1000.0f, 800.0f, &config));
  CHECK(config.width == 320.0f && config.frameless);
  NileWindowConfig unset;
  memset(&unset, 0, sizeof unset);
  CHECK(!nile_view_window_config(view, 1000.0f, 800.0f, &unset));

  /* Structs the host fills, by pointer. */
  NileClipboard clipboard;
  memset(&clipboard, 0, sizeof clipboard);
  clipboard.struct_size = sizeof(NileClipboard);
  clipboard.get = clip_get;
  clipboard.set = clip_set;
  nile_view_set_clipboard(view, &clipboard);
  NileDialogs dialogs;
  memset(&dialogs, 0, sizeof dialogs);
  dialogs.struct_size = sizeof(NileDialogs);
  nile_view_set_dialogs(view, &dialogs);

  NileTick tick = {.struct_size = sizeof(NileTick)};
  nile_view_tick(view, &tick);

  /* Rendering. */
  uint8_t pixels[320 * 200 * 4];
  CHECK(nile_view_render(view, pixels, sizeof pixels, 320, 200, 1.0f));

  /* Tray, badge, progress. */
  char *tray = nile_view_tray(view);
  CHECK(tray != NULL && strstr(tray, "\"tooltip\":\"Tip\"") != NULL);
  nile_string_free(tray);
  char *badge = nile_view_badge(view);
  CHECK(badge != NULL && strcmp(badge, "2") == 0);
  nile_string_free(badge);
  float progress = 0.0f;
  CHECK(nile_view_progress(view, &progress) == 2 && progress == 0.25f);
  nile_view_tray_event(view, "trayclick");

  /* Global hotkeys. */
  char *keys = nile_view_hotkeys(view);
  CHECK(keys != NULL && strstr(keys, "Ctrl+Shift+K") != NULL);
  nile_string_free(keys);
  nile_view_hotkey_event(view, 1, NILE_HOTKEY_STATE_REGISTERED);
  nile_view_hotkey_event(view, 1, NILE_HOTKEY_STATE_PRESSED);

  /* Monitors, application events, window chrome. */
  CHECK(nile_view_set_screens(view, "[{\"name\":\"A\",\"x\":0,\"y\":0,\"width\":1920,\"height\":1080,\"scale\":1,\"primary\":true}]", 0));
  nile_view_application_event(view, "open-file", "{\"path\":\"/tmp/x.txt\"}");
  float rect[4];
  CHECK(nile_view_maximize_button_rect(view, rect));
  CHECK(nile_view_window_corner_radius(view) >= 0.0f);

  /* Menus. */
  char *menu = nile_view_menu(view);
  nile_string_free(menu); /* NULL (no application menu) is fine */

  nile_view_destroy(view);
  if (failures) return 1;
  printf("C API check passed (ABI %u, Nile %s)\n", nile_abi_version(), nile_version());
  return 0;
}
