/*
 * Embedding Nile from C: load a page, call into it, let it call back, type into it, and
 * render it to a PPM image.
 *
 *   cargo build -p nile-capi --release
 *   cc examples/c/render.c -Icrates/nile-capi/include -Ltarget/release -lnile \
 *      -Wl,-rpath,$PWD/target/release -o render
 *   ./render examples/hello.html out.ppm
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "nile.h"

/* A function scripts can call as host.add(1, 2). Arguments arrive as a JSON array. */
static const char *host_add(const char *args_json, void *user) {
  int *calls = user;
  (*calls)++;
  printf("script called host.add(%s)\n", args_json);
  return "42"; /* JSON result */
}

int main(int argc, char **argv) {
  const char *file = argc > 1 ? argv[1] : "examples/hello.html";
  const char *out = argc > 2 ? argv[2] : "out.ppm";
  const uint32_t width = 640, height = 400;

  NileView *view = nile_view_create_file(file, (float)width, (float)height);
  if (!view) return 1;
  printf("Nile %s, title: %s\n", nile_version(), nile_view_title(view));

  int calls = 0;
  nile_view_register_function(view, "add", host_add, &calls);

  /* Run script in the page and read the result back. */
  char *result = NULL;
  if (nile_view_eval(view, "document.title + ' / ' + host.add(1, 2)", &result) == 0)
    printf("eval -> %s\n", result);
  nile_string_free(result);

  /* Input: hover and click somewhere, then type (it only matters if an input has focus). */
  nile_pointer_move(view, 60, 80);
  nile_pointer_button(view, 0, true, 0);
  nile_pointer_button(view, 0, false, 0);
  nile_text_input(view, "hello");

  /* Let timers and animations run for a moment, as a real host would from its event loop. */
  NileTick tick;
  nile_view_tick(view, &tick);

  /* Window requests the page made (close buttons, moveTo, ...). */
  NileWindowCommand cmd;
  while (nile_view_take_window_command(view, &cmd))
    printf("window request %d (%g, %g)\n", cmd.kind, cmd.a, cmd.b);

  /* Render: BGRA, premultiplied, tightly packed. */
  size_t len = (size_t)width * height * 4;
  uint8_t *pixels = malloc(len);
  if (!nile_view_render(view, pixels, len, width, height, 1.0f)) return 2;

  FILE *f = fopen(out, "wb");
  fprintf(f, "P6\n%u %u\n255\n", width, height);
  for (size_t i = 0; i < (size_t)width * height; i++) {
    uint8_t rgb[3] = {pixels[i * 4 + 2], pixels[i * 4 + 1], pixels[i * 4]};
    fwrite(rgb, 1, 3, f);
  }
  fclose(f);
  printf("wrote %s (host.add was called %d time(s))\n", out, calls);

  free(pixels);
  nile_view_destroy(view);
  return 0;
}
