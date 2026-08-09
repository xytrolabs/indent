// indent-gui — native HTML window helper for Indent's gui_show_html builtin.
// Reads HTML from stdin, displays in a WebKitGTK window.
// Usage: indent-gui <title> --stdin <width> <height>
#include <gtk/gtk.h>
#include <webkit2/webkit2.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static char *read_all_stdin(void) {
    size_t cap = 1 << 20, len = 0;
    char *buf = malloc(cap);
    if (!buf) return NULL;
    size_t n;
    while ((n = fread(buf + len, 1, cap - len - 1, stdin)) > 0) {
        len += n;
        if (len >= cap - 1) {
            cap *= 2;
            char *nb = realloc(buf, cap);
            if (!nb) { free(buf); return NULL; }
            buf = nb;
        }
    }
    buf[len] = '\0';
    return buf;
}

int main(int argc, char **argv) {
    if (argc < 5) {
        fprintf(stderr, "usage: indent-gui <title> --stdin <width> <height>\n");
        return 1;
    }
    const char *title = argv[1];
    int width = atoi(argv[3]);
    int height = atoi(argv[4]);

    gtk_init(&argc, &argv);

    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), title);
    gtk_window_set_default_size(GTK_WINDOW(window), width, height);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);

    GtkWidget *webview = webkit_web_view_new();
    gtk_container_add(GTK_CONTAINER(window), webview);

    char *html = read_all_stdin();
    if (!html) { fprintf(stderr, "no html\n"); return 1; }
    webkit_web_view_load_html(WEBKIT_WEB_VIEW(webview), html, NULL);

    gtk_widget_show_all(window);
    gtk_main();
    free(html);
    return 0;
}
